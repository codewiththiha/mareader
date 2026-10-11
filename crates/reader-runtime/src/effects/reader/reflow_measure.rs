//! The measurement pipeline: DOM block heights reach the cut,
//! re-seeded when typography moves.

use std::sync::Arc;
use std::time::Duration;

use leptos::prelude::*;

use app_chrome::hooks::use_timeout::{Debouncer, use_debounce};
use reflow_core::geometry::{PageGeometry, geometry};
use reflow_core::pager::estimate_heights;
use reflow_core::typography::TextSettings;

use crate::state::TypographySignal;
use crate::state::document::reflow::estimate_metrics;

/// Jitter gate: two pixels, so rounding noise never bumps the epoch.
const INGEST_EPSILON: f64 = 2.0;

/// A landed batch waits this long for company before flushing.
const INGEST_DEBOUNCE_MS: u64 = 120;

/// What a mounted row was showing when its box was measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowBox {
    /// The block's own content, rendered and measured.
    Content,
    /// The motion band's placeholder: a box sized by the layout's estimate.
    Placeholder,
}

/// One measured row: which block it is, what it was showing, and its
/// scale-1 height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RowMeasurement {
    /// The block's index in document order.
    pub index: usize,
    /// What the row held when the height was read.
    pub box_kind: RowBox,
    /// The measured height at scale 1.
    pub height: f64,
}

/// The reports that may become canonical heights: real content only, one per
/// block, the last measurement of that block winning.
///
/// A placeholder box is the layout's own estimate drawn in the DOM. Handing
/// it back as a measurement closes the loop — the estimate starts
/// confirming itself — and every recut it provokes moves the text under a
/// reader who is trying to read it.
pub fn content_only(rows: &[RowMeasurement]) -> Vec<(usize, f64)> {
    let mut merged: Vec<(usize, f64)> = Vec::with_capacity(rows.len());
    for row in rows {
        if row.box_kind != RowBox::Content || row.height <= 0.0 {
            continue;
        }
        match merged.last_mut() {
            Some(last) if last.0 == row.index => last.1 = row.height,
            _ => merged.push((row.index, row.height)),
        }
    }
    merged
}

/// The session and scale a batch was measured against, beside its
/// `(index, scale-1 height)` reports.
type PendingBatch = (u64, f64, Vec<(usize, f64)>);

/// The pane's measurement inbox: its waiting batch and the flush that
/// lands it.
#[derive(Clone, Copy)]
pub struct MeasureInbox {
    /// The waiting batch, tagged with its document and display scale.
    pending: StoredValue<PendingBatch, LocalStorage>,
    /// The installed flush; `None` when the pane has no mounted lifetime.
    flusher: StoredValue<Option<Debouncer>, LocalStorage>,
}

impl Default for MeasureInbox {
    fn default() -> Self {
        Self {
            pending: StoredValue::new_local((0, 1.0, Vec::new())),
            flusher: StoredValue::new_local(None),
        }
    }
}

impl MeasureInbox {
    /// Hand measured SCALE-1 heights to the store; the caller divides the
    /// live scale out first.
    ///
    /// Placeholder boxes are dropped here rather than at the call sites: this
    /// is the one door to the canonical heights, and a placeholder's height
    /// is the estimate the store already holds.
    pub fn ingest(&self, session: u64, scale: f64, rows: &[RowMeasurement]) {
        let batch = content_only(rows);
        if batch.is_empty() {
            return;
        }
        // `try_`: a report from a disposed pane is owed to nobody.
        let parked = self.pending.try_update_value(|pending| {
            if pending.0 != session || pending.1 != scale {
                pending.2.clear();
                pending.0 = session;
                pending.1 = scale;
            }
            pending.2.extend_from_slice(&batch);
        });
        if parked.is_none() {
            return;
        }
        if let Some(debouncer) = self.flusher.try_get_value().flatten() {
            debouncer.trigger();
        }
    }

    /// The waiting batch, taken.
    fn take(&self) -> PendingBatch {
        self.pending
            .try_update_value(std::mem::take)
            .unwrap_or_default()
    }

    fn install(&self, debouncer: Option<Debouncer>) {
        let _ = self.flusher.try_set_value(debouncer);
    }
}

/// Install the debounced flush and the re-estimate, once per reader
/// mount.
pub fn install_reflow_measure(state: crate::context::ReaderContext) {
    let debouncer = use_debounce(Duration::from_millis(INGEST_DEBOUNCE_MS), move || {
        flush(state);
    });
    let inbox = state.reader.measure;
    inbox.install(Some(debouncer));
    on_cleanup(move || inbox.install(None));

    // The re-estimate, tracked on the typography and the two width dials.
    let typography = use_context::<TypographySignal>()
        .expect("TypographySignal must be provided by app bootstrap");
    let last: StoredValue<Option<(TextSettings, f64, f64)>, LocalStorage> =
        StoredValue::new_local(None);
    Effect::new(move |_| {
        // Ink is paint, not layout: zero it so its slider cannot reach the
        // estimate.
        let mut settings = typography.get();
        settings.ink_contrast = 0.0;
        let margin = state.reader.viewer.page_margin.get();
        let pct = state.reader.viewer.column_width_pct.get();
        // The tuple copies `settings`; the estimate below still needs it.
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
        // A correction survives only when the block's own estimate survives.
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
            // Same heights, but a dial may have moved the sheet: still re-cut.
            recut_and_publish(&state, reflow, geo);
            return;
        }
        reflow.heights.set(Arc::new(merged));
        // The geometry moved wholesale: a measurement batch's rows applied
        // their own numbers already.
        reflow
            .estimate_generation
            .update(|generation| *generation += 1);
        recut_and_publish(&state, reflow, geo);
    });
}

/// Land the waiting batch in the shared store, then let the cut follow.
fn flush(state: crate::context::ReaderContext) {
    let (session, scale, batch) = state.reader.measure.take();
    if batch.is_empty() {
        return;
    }
    let reflow = state.reader.document.content.reflow;
    // The document may have closed while the batch waited: drop it.
    if !state.pane.admits_reflow(session) || reflow.block_count() == 0 {
        return;
    }
    let next = reflow
        .heights
        .with_untracked(|heights| applied_heights(heights, &batch, scale));
    let Some(next) = next else {
        // All within the jitter gate: no write, no epoch, no re-cut.
        return;
    };
    reflow.heights.set(Arc::new(next));
    let geo = reflow.geometry.get_untracked();
    recut_and_publish(&state, reflow, geo);
}

/// Re-cut from the store and publish, holding the reader's block.
fn recut_and_publish(
    state: &crate::context::ReaderContext,
    reflow: crate::state::document::ReflowContent,
    geo: PageGeometry,
) {
    if let Some(cut) = reflow.recut(state.reader, geo) {
        state.reader.document.publish_cut(&cut);
        state.reader.viewer.page.set(cut.page);
    }
}

/// The geometry the width dials resolve to, shared by estimate and
/// re-cut.
fn dialled_geometry(settings: &TextSettings, margin: f64, column_pct: f64) -> PageGeometry {
    geometry(settings.book_layout)
        .with_extra_inline(margin)
        .with_column_pct(column_pct)
}

/// Apply a batch to the standing heights; `None` when nothing moved.
fn applied_heights(current: &[f64], batch: &[(usize, f64)], scale: f64) -> Option<Vec<f64>> {
    let mut next = current.to_vec();
    let mut moved = false;
    let epsilon = INGEST_EPSILON / scale;
    for &(index, height) in batch {
        if let Some(slot) = next.get_mut(index)
            && (*slot - height).abs() > epsilon
        {
            *slot = height;
            moved = true;
        }
    }
    moved.then_some(next)
}

/// A block keeps its measured height when its estimate did not move.
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

/// Whether any height moved past the jitter gate.
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

    fn row(index: usize, kind: RowBox, height: f64) -> RowMeasurement {
        RowMeasurement {
            index,
            box_kind: kind,
            height,
        }
    }

    /// The blanking contract, enforced at the only door to the heights: a
    /// placeholder's box is the layout's own estimate, and feeding it back
    /// is how the stream starts jumping under a reader.
    #[test]
    fn a_placeholder_measurement_never_reaches_the_canonical_heights() {
        let rows = vec![
            row(0, RowBox::Content, 180.0),
            row(1, RowBox::Placeholder, 140.0),
            row(2, RowBox::Content, 260.0),
            row(3, RowBox::Placeholder, 90.0),
        ];
        assert_eq!(content_only(&rows), vec![(0, 180.0), (2, 260.0)]);

        // A blank batch changes nothing at all.
        let heights = vec![300.0, 300.0];
        let blank = vec![
            row(0, RowBox::Placeholder, 100.0),
            row(1, RowBox::Placeholder, 100.0),
        ];
        assert!(content_only(&blank).is_empty());
        assert_eq!(applied_heights(&heights, &content_only(&blank), 1.0), None);
    }

    /// One block measured twice in a batch (a zoom landing mid-pass) keeps
    /// the last report, not a sum and not the first.
    #[test]
    fn repeated_reports_of_one_block_coalesce_to_the_last() {
        let rows = vec![
            row(7, RowBox::Content, 210.0),
            row(7, RowBox::Content, 240.0),
            row(8, RowBox::Placeholder, 999.0),
            row(8, RowBox::Content, 260.0),
        ];
        assert_eq!(content_only(&rows), vec![(7, 240.0), (8, 260.0)]);
    }

    #[test]
    fn a_correction_above_the_gate_moves_the_cut_and_its_map() {
        // Three blocks of 300px into 500px pages: one block per page, three
        // pages.
        let heights = vec![300.0, 300.0, 300.0];
        let before = paginate(&heights, 500.0);
        let before_map = block_page_index(&before, heights.len());
        assert_eq!(before.len(), 3);
        // A measurement 110px short of the estimate: real text ran shorter.
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
        // An index beyond the store cannot grow it or panic.
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
        // Block 0 measured, block 1 at estimate, block 2 measured but moved.
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
        // The long paragraph is several lines; the short one, one line.
        assert!(est[0] > est[1]);
        // And the margin dial narrows the column, which grows the heights.
        let margin_geo = dialled_geometry(&settings, 64.0, 100.0);
        let margin_est = estimate_heights(&blocks, &estimate_metrics(&settings, &margin_geo));
        assert!(margin_est[0] > est[0]);
    }
}
