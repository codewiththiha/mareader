//! The measurement pipeline: the only way the DOM's real block heights reach
//! the page cut.
//!
//! The old shape rendered the whole document a second time into a hidden
//! twin so it could read every block's height — which put an entire book in
//! RAM twice and restyled the twin on every theme tick. The reader's own
//! rows already render the type; this module is the pipe that carries what
//! they learn back into the shared model:
//!
//! * the STREAM and the PAGE HOSTS measure the blocks they mounted, divide
//!   out the live scale (the store is scale-1 truth), and hand the batch to
//!   [`ingest`];
//! * ingest parks the batch and arms a DEBOUNCED flush — a heights write
//!   bumps the stream's epoch (an `O(n)` layout rebuild), so one frame of a
//!   fling must never cost one rebuild per frame;
//! * the flush applies whatever moved more than [`INGEST_EPSILON`], then
//!   re-cuts through [`crate::state::reader::document::reflow::ReflowContent::recut`]
//!   and publishes the answer, holding the reader on the block they were
//!   reading.
//!
//! Beside the pipe sits the RE-ESTIMATE: typography and width-dial changes
//! move every block's expected height, so the estimate is re-run and the
//! store re-seeded. It is pure Rust math — never DOM — and it keys off a
//! layout-relevance check, so paint-only knobs (the ink dial first among
//! them) can never reach it. Estimates and measurements then converge on the
//! same store: the estimate seeds, each mounted row corrects, and a
//! correction only survives a layout change when the block's own estimate
//! survived it.

use std::cell::RefCell;
use std::sync::Arc;
use std::time::Duration;

use leptos::prelude::*;

use app_chrome::hooks::use_timeout::{use_debounce, Debouncer};
use reflow_core::geometry::{geometry, PageGeometry};
use reflow_core::pager::estimate_heights;
use reflow_core::typography::TextSettings;

use crate::state::reader::document::reflow::estimate_metrics;
use crate::state::reader::TypographySignal;
use crate::state::AppState;

/// A measured height within two pixels of the store's number is jitter, not
/// news: subpixel rounding and font-hinting noise must not bump the stream's
/// epoch (and with it the whole reflowable side) forever. The same gate the
/// gloss measure applies to its own reports (`accepted_height` in
/// `components::ai::gloss::hooks::use_content_size`), and the feedback loop
/// ingest → heights → re-measure → ingest terminates only through it.
pub const INGEST_EPSILON: f64 = 2.0;

/// How long a measurement batch waits for company before it flushes. Short
/// enough that a settled page converges within a blink of its rows landing;
/// long enough that a fling's worth of reports costs one heights write, not
/// one per frame.
const INGEST_DEBOUNCE_MS: u64 = 120;

thread_local! {
    /// The batch the next flush will land, with the identity of the document
    /// and display scale it was measured against: either changing while a
    /// batch waits is a reason to drop the stale reports.
    static PENDING: RefCell<(usize, f64, Vec<(usize, f64)>)> = const { RefCell::new((0, 1.0, Vec::new())) };
    /// The installed flush. `None` outside the reader's lifetime: an ingest
    /// with nobody home is a report nobody owes an answer to.
    static FLUSHER: RefCell<Option<Debouncer>> = const { RefCell::new(None) };
}

/// Hand a batch of measured SCALE-1 heights — `(block index, height)` — to
/// the shared store. The caller divides out the supplied live display scale
/// first: the store is the scale-1 truth the estimate seeds, and a zoomed
/// number written into it would poison every layout that reads it.
///
/// `doc_id` is the block list's `Arc` pointer (see
/// `crate::state::reader::document::reflow::ReflowContent::document_id`):
/// the flush drops a batch whose document has since been swapped out.
pub fn ingest(doc_id: usize, scale: f64, batch: &[(usize, f64)]) {
    if batch.is_empty() {
        return;
    }
    PENDING.with(|pending| {
        let mut pending = pending.borrow_mut();
        if pending.0 != doc_id || pending.1 != scale {
            pending.2.clear();
            pending.0 = doc_id;
            pending.1 = scale;
        }
        pending.2.extend_from_slice(batch);
    });
    FLUSHER.with(|flusher| {
        if let Some(debouncer) = *flusher.borrow() {
            debouncer.trigger();
        }
    });
}

/// Install the pipeline: the debounced flush and the re-estimate effect.
/// Called once per reader mount, beside the other reader effects — it owns
/// nothing format-specific, and stands down on its own when no blocks are
/// open.
pub fn install_reflow_measure(state: AppState) {
    let debouncer = use_debounce(Duration::from_millis(INGEST_DEBOUNCE_MS), move || {
        flush(state);
    });
    FLUSHER.with(|slot| *slot.borrow_mut() = Some(debouncer));
    on_cleanup(|| FLUSHER.with(|slot| *slot.borrow_mut() = None));

    // The re-estimate. Tracked reads: the typography and the two width
    // dials. A layout-relevant change re-runs the pure estimate and re-seeds
    // the store, carrying measured corrections forward where they still
    // apply; a paint-only knob (the ink dial) changes none of the numbers
    // below and costs exactly one comparison.
    let typography = use_context::<TypographySignal>()
        .expect("TypographySignal must be provided by app bootstrap");
    let last: StoredValue<Option<(TextSettings, f64, f64)>, LocalStorage> =
        StoredValue::new_local(None);
    Effect::new(move |_| {
        // Ink is paint, not layout: zero it before comparing so its slider
        // can never reach the estimate. Every other knob in the struct moves
        // a height (or a width the height flows in) and stays in.
        let mut settings = typography.get();
        settings.ink_contrast = 0.0;
        let margin = state.reader.viewer.page_margin.get();
        let pct = state.reader.viewer.column_width_pct.get();
        // The tuple stores its own copy: `settings` stays owned below, where
        // the estimate still needs it.
        let inputs = (settings.clone(), margin, pct);
        if last.with_value(|entry| entry.as_ref() == Some(&inputs)) {
            return;
        }
        let previous = last.with_value(|entry| entry.clone());
        last.set_value(Some(inputs));

        let reflow = state.reader.document.content.reflow;
        let blocks = reflow.blocks.get();
        if blocks.is_empty() {
            return;
        }
        let geo = dialled_geometry(&settings, margin, pct);
        let metrics = estimate_metrics(&settings, &geo);
        let new_est = estimate_heights(&blocks, &metrics);
        // A correction survives the layout change only when the block's own
        // estimate survived it; the first run meets a store the open flow
        // seeded from these exact inputs, and the equality gate below turns
        // that no-op into no write.
        let merged = match previous {
            Some((prev_settings, prev_margin, prev_pct)) => {
                let prev_geo = dialled_geometry(&prev_settings, prev_margin, prev_pct);
                let prev_metrics = estimate_metrics(&prev_settings, &prev_geo);
                let old_est = estimate_heights(&blocks, &prev_metrics);
                merged_heights(&reflow.heights.get_untracked(), &old_est, &new_est)
            }
            None => new_est,
        };
        if !heights_moved(&reflow.heights.get_untracked(), &merged) {
            // Same heights; a dial may still have moved the sheet the cut
            // answers for, so the re-cut gets its say even on a no-op merge.
            if let Some(cut) = reflow.recut(state, geo) {
                state.reader.document.publish_cut(&cut);
                state.reader.viewer.page.set(cut.page);
            }
            return;
        }
        reflow.heights.set(Arc::new(merged));
        if let Some(cut) = reflow.recut(state, geo) {
            state.reader.document.publish_cut(&cut);
            state.reader.viewer.page.set(cut.page);
        }
    });
}

/// Land the waiting batch in the shared store, then let the cut follow.
fn flush(state: AppState) {
    let (doc_id, scale, batch) = PENDING.with(|pending| std::mem::take(&mut *pending.borrow_mut()));
    if batch.is_empty() {
        return;
    }
    let reflow = state.reader.document.content.reflow;
    // The document may have closed or been swapped out between the report
    // and the debounce firing; either way the batch belongs to nobody now.
    let current_doc = reflow.blocks.with_untracked(|blocks| Arc::as_ptr(blocks) as usize);
    if reflow.block_count() == 0 || current_doc != doc_id {
        return;
    }
    let next = reflow
        .heights
        .with_untracked(|heights| applied_heights(heights, &batch, scale));
    let Some(next) = next else {
        // Everything landed within the jitter gate: no write, no epoch, no
        // re-cut — the loop terminates exactly here.
        return;
    };
    reflow.heights.set(Arc::new(next));
    let geo = reflow.geometry.get_untracked();
    if let Some(cut) = reflow.recut(state, geo) {
        state.reader.document.publish_cut(&cut);
        state.reader.viewer.page.set(cut.page);
    }
}

/// The geometry the reader's two width dials resolve to — the one definition
/// the estimate, the re-cut and the open flow share, so a dial move re-cuts
/// through the same numbers everywhere. (The stream's column spends the
/// margin differently — as an inset around the column, not inside it — and
/// composes its own; see `components::formats::reflow::stream`.)
pub(crate) fn dialled_geometry(
    settings: &TextSettings,
    margin: f64,
    column_pct: f64,
) -> PageGeometry {
    geometry(settings.book_layout)
        .with_extra_inline(margin)
        .with_column_pct(column_pct)
}

/// Apply one measurement batch to the standing heights. Answers `None` when
/// nothing moved more than the reporting-scale-adjusted [`INGEST_EPSILON`] —
/// then no `Arc` is built, no epoch bumps, and the loop has nowhere to go.
fn applied_heights(current: &[f64], batch: &[(usize, f64)], scale: f64) -> Option<Vec<f64>> {
    let mut next = current.to_vec();
    let mut moved = false;
    let epsilon = INGEST_EPSILON / scale;
    for &(index, height) in batch {
        if let Some(slot) = next.get_mut(index) {
            if (*slot - height).abs() > epsilon {
                *slot = height;
                moved = true;
            }
        }
    }
    moved.then_some(next)
}

/// The re-estimate's merge: a block keeps its measured height when the new
/// estimate equals the one the measurement corrected (the block's layout did
/// not change, so the measurement is still the truth), and falls back to the
/// fresh estimate when it did not.
fn merged_heights(old: &[f64], old_est: &[f64], new_est: &[f64]) -> Vec<f64> {
    new_est
        .iter()
        .enumerate()
        .map(|(index, est)| {
            let (prev, prev_est) = match (old.get(index), old_est.get(index)) {
                (Some(prev), Some(prev_est)) => (*prev, *prev_est),
                _ => return *est,
            };
            let measured = (prev - prev_est).abs() > INGEST_EPSILON;
            let estimate_moved = (prev_est - est).abs() > 1e-9;
            if measured && !estimate_moved {
                prev
            } else {
                *est
            }
        })
        .collect()
}

/// Whether any height moved more than the jitter gate — the write guard the
/// re-estimate owes the epoch.
fn heights_moved(current: &[f64], candidate: &[f64]) -> bool {
    current.len() != candidate.len()
        || current
            .iter()
            .zip(candidate)
            .any(|(a, b)| (a - b).abs() > INGEST_EPSILON)
}

#[cfg(test)]
mod tests {
    use super::*;

    use reflow_core::block::{BlockKind, TextBlock};
    use reflow_core::pager::{block_page_index, paginate};

    fn text_block(chars: usize) -> TextBlock {
        TextBlock {
            kind: BlockKind::Text,
            text: "x".repeat(chars),
            continuation: false,
        }
    }

    #[test]
    fn a_correction_above_the_gate_moves_the_cut_and_its_map() {
        // Three blocks of 300px into 500px pages: one block per page, three
        // pages.
        let heights = vec![300.0, 300.0, 300.0];
        let before = paginate(&heights, 500.0);
        let before_map = block_page_index(&before, heights.len());
        assert_eq!(before.len(), 3);
        // A measurement lands 110px short of the estimate — real text ran
        // shorter than character counts said.
        let batch = vec![(0usize, 190.0)];
        let next = applied_heights(&heights, &batch, 1.0).expect("110px moves any gate");
        let after = paginate(&next, 500.0);
        let after_map = block_page_index(&after, next.len());
        assert_ne!(before, after, "the correction must re-cut");
        assert_ne!(before_map, after_map, "and the block map must follow");
        // Blocks 0+1 now share the first page (190 + 300 fits the 500).
        assert_eq!(after_map[0], after_map[1]);
        assert_eq!(after_map[0], 0);
        assert_eq!(after_map[2], 1);
    }

    #[test]
    fn a_correction_inside_the_gate_is_a_no_op() {
        let heights = vec![300.0, 300.0, 300.0];
        // 1.5px of rounding noise: nothing downstream may hear about it.
        assert_eq!(applied_heights(&heights, &[(1usize, 301.5)], 1.0), None);
        assert_eq!(applied_heights(&heights, &[(1usize, 298.2)], 1.0), None);
        // A batch that names nothing is also a no-op, not an empty write.
        assert_eq!(applied_heights(&heights, &[], 1.0), None);
        // An index beyond the store (a stale report from a window the
        // outgoing layout still held) cannot grow or panic it.
        assert_eq!(applied_heights(&heights, &[(9usize, 100.0)], 1.0), None);
    }

    #[test]
    fn the_jitter_gate_is_adjusted_to_the_reporting_scale() {
        let heights = vec![300.0];
        assert_eq!(applied_heights(&heights, &[(0, 300.9)], 2.0), None);
        assert_eq!(
            applied_heights(&heights, &[(0, 301.1)], 2.0),
            Some(vec![301.1])
        );
    }

    #[test]
    fn the_merge_keeps_live_corrections_and_drops_stale_ones() {
        // Block 0 was measured (estimate 100, truth 140); block 1 keeps its
        // estimate; block 2 was measured too, but its layout changed.
        let old = vec![140.0, 100.0, 210.0];
        let old_est = vec![100.0, 100.0, 200.0];
        let new_est = vec![100.0, 110.0, 220.0];
        let merged = merged_heights(&old, &old_est, &new_est);
        // Same estimate: the measurement is still the truth.
        assert_eq!(merged[0], 140.0);
        // Never measured: the fresh estimate.
        assert_eq!(merged[1], 110.0);
        // Measured, but the estimate moved: the stale correction goes.
        assert_eq!(merged[2], 220.0);
    }

    #[test]
    fn the_re_estimate_seeds_from_the_blocks_it_is_given() {
        let blocks = vec![text_block(400), text_block(40)];
        let settings = TextSettings::default();
        let geo = dialled_geometry(&settings, 0.0, 100.0);
        let est = estimate_heights(&blocks, &estimate_metrics(&settings, &geo));
        assert_eq!(est.len(), 2);
        // The long paragraph is several lines; the short one is one line
        // plus its paragraph space.
        assert!(est[0] > est[1]);
        // And the margin dial narrows the column, which grows the heights.
        let margin_geo = dialled_geometry(&settings, 64.0, 100.0);
        let margin_est = estimate_heights(&blocks, &estimate_metrics(&settings, &margin_geo));
        assert!(margin_est[0] > est[0]);
    }
}
