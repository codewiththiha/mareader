//! The reader's two virtualizers: one pair per pane, built at mount and
//! ended at dispose.

use std::hash::Hash;

use leptos::prelude::*;
use virtual_list::{Budget, Viewport};
use virtual_list_leptos::{RetentionPolicy, VirtualizerOptions, use_virtualizer};

use crate::state::ReaderState;
use crate::zoom::config::MAX_ZOMBIES;
use app_ui::epoch::epoch_signal;

/// 1.5 screenfuls (capped at 8) mounts the viewport plus one fling frame.
pub(crate) const RENDER_BUDGET: Budget = Budget::screenfuls(1.5, 8);

/// Initial fill-latency estimate (ms) before measured p95 samples arrive.
#[cfg(feature = "pdf")]
pub(crate) const EST_FILL_LATENCY_MS: f64 = 180.0;

/// Timed 450ms grace bridge so short reversals reuse warm page hosts.
const STRIP_RETENTION: RetentionPolicy = RetentionPolicy::Grace {
    ms: 450,
    max: MAX_ZOMBIES,
};

/// Both virtualizers always exist; a view binds the one for its axis.
pub(crate) struct ReaderVirtualizers {
    pub virtualizer: virtual_list_leptos::Virtualizer,
    pub h_virtualizer: virtual_list_leptos::Virtualizer,
    pub virtualizer_view: StoredValue<virtual_list_leptos::Virtualizer, LocalStorage>,
    pub h_virtualizer_view: StoredValue<virtual_list_leptos::Virtualizer, LocalStorage>,
}

/// Page-1 height, the estimate for any page whose own size is unknown.
fn fallback_height(state: ReaderState) -> f64 {
    state
        .document
        .content
        .metrics
        .page1_size
        .try_get_untracked()
        .flatten()
        .map_or(0.0, |size| size.height)
}

/// Only the open flow empties the store, so seeding when empty is safe.
fn seed_css_heights(state: ReaderState) {
    Effect::new(move || {
        let count = state.document.num_pages.get() as usize;
        let filled = state
            .document
            .content
            .metrics
            .css_heights
            .with(|heights| !heights.is_empty());
        let scale = state.viewer.zoom.display.get();
        if filled || count == 0 || scale <= 0.0 {
            return;
        }
        // Tracked: the sizes arrive in the same open that emptied the store.
        let fallback = state
            .document
            .content
            .metrics
            .page1_size
            .get()
            .map_or(0.0, |size| size.height);
        let seeded: Vec<f64> = state.document.content.metrics.intrinsic.with(|sizes| {
            (0..count)
                .map(|index| {
                    sizes
                        .get(index)
                        .map(|size| size.height)
                        .filter(|height| *height > 0.0)
                        .unwrap_or(fallback)
                        * scale
                })
                .collect()
        });
        if seeded.iter().all(|height| *height <= 0.0) {
            return; // nothing measured yet; keep the store empty for the next run
        }
        state.document.content.metrics.css_heights.set(seeded);
    });
}

/// Page count plus every intrinsic size; a change rebuilds the layouts.
fn geometry_epoch(state: ReaderState) -> Signal<u64> {
    epoch_signal(move |hasher| {
        state.document.num_pages.get().hash(hasher);
        state.document.content.metrics.intrinsic.with(|sizes| {
            sizes.len().hash(hasher);
            for size in sizes {
                size.width.to_bits().hash(hasher);
                size.height.to_bits().hash(hasher);
            }
        });
    })
}

/// Compute the p95 of `samples`, or `EST_FILL_LATENCY_MS` when empty.
#[cfg(feature = "pdf")]
fn p95_fill_ms(samples: &[f64]) -> f64 {
    if samples.is_empty() {
        return EST_FILL_LATENCY_MS;
    }
    let mut sorted: Vec<f64> = samples
        .iter()
        .copied()
        .filter(|v| v.is_finite() && *v > 0.0)
        .collect();
    if sorted.is_empty() {
        return EST_FILL_LATENCY_MS;
    }
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let idx = ((sorted.len() as f64 * 0.95).ceil() as usize)
        .saturating_sub(1)
        .min(sorted.len() - 1);
    sorted[idx]
}

/// Report p95 settled page fill cost and lane width to the virtualizer.
#[cfg(feature = "pdf")]
fn note_fill_profile(
    pane: &crate::pane::handle::PaneHandle,
    strip: &virtual_list_leptos::Virtualizer,
    samples: &std::rc::Rc<std::cell::RefCell<Vec<f64>>>,
    est_fill_ms: &std::rc::Rc<std::cell::Cell<f64>>,
) {
    let Some(stats) = pane.pdf().stats() else {
        return;
    };
    // 0 is "not measured", not "instant": a zero never engages the band.
    if stats.fill_ms <= 0.0 {
        return;
    }
    let p95 = {
        let mut ring = samples.borrow_mut();
        if ring.len() >= 20 {
            ring.remove(0);
        }
        ring.push(stats.fill_ms);
        p95_fill_ms(&ring)
    };
    if (p95 - est_fill_ms.get()).abs() <= 1.0 {
        return;
    }
    est_fill_ms.set(p95);
    strip.set_fill_profile(p95, stats.page_limit.max(1) as usize);
}

/// Prefetch the predicted landing page neighborhood into the thumbnail cache.
#[cfg(feature = "pdf")]
fn prefetch_neighborhood(
    pane: crate::pane::handle::PaneHandle,
    state: ReaderState,
    center_page: u32,
) {
    let total = state.document.num_pages.try_get_untracked().unwrap_or(0);
    if total == 0 {
        return;
    }
    let stamp = pane.generation();
    let pdf = pane.pdf();
    let scale = crate::components::shell::sidebar::panels::thumbnails::geometry::THUMB_SCALE;
    let center = center_page.clamp(1, total);
    let candidates = [
        center,
        center.saturating_sub(1).max(1),
        (center + 1).min(total),
    ];
    wasm_bindgen_futures::spawn_local(async move {
        for p in candidates {
            if !pane.owns_generation(stamp) || !pdf.still_current(&pane) {
                return;
            }
            if !pdf.has_thumb(p, scale) {
                pdf.prefetch_thumb(p, scale).await;
            }
        }
    });
}

pub(crate) fn use_reader_virtualizers(
    state: ReaderState,
    pane: crate::pane::handle::PaneHandle,
) -> ReaderVirtualizers {
    seed_css_heights(state);

    let count = Signal::derive(move || state.document.num_pages.get() as usize);
    let estimate = move |index: usize| {
        // Teardown can call this; `page_gap` gone means every read below
        // is gone.
        let Some(gap) = state.viewer.page_gap.try_get_untracked() else {
            return 0.0;
        };
        let measured = state
            .document
            .content
            .metrics
            .css_heights
            .with_untracked(|heights| heights.get(index).copied())
            .filter(|height| *height > 0.0);
        if let Some(height) = measured {
            return height + gap;
        }
        let intrinsic = state
            .document
            .content
            .metrics
            .intrinsic
            .with_untracked(|sizes| sizes.get(index).map(|size| size.height))
            .filter(|height| *height > 0.0);
        intrinsic.unwrap_or_else(|| fallback_height(state)) * state.viewer.zoom.visual_scale() + gap
    };
    // The live display scale, so neither axis disagrees about a page's size.
    let h_estimate = move |index: usize| {
        let Some(margin) = state.viewer.page_margin.try_get_untracked() else {
            return 0.0;
        };
        state
            .document
            .content
            .metrics
            .intrinsic
            .with_untracked(|sizes| sizes.get(index).map(|s| s.width).unwrap_or(0.0))
            * state.viewer.zoom.visual_scale()
            + 2.0 * margin
    };
    let epoch = geometry_epoch(state);
    let pinned_sig: RwSignal<Option<(usize, usize)>> = RwSignal::new(None);
    let initial_vh = {
        let (_, height) = state.viewer.container_size.get_untracked();
        if height > 1.0 { height } else { 800.0 }
    };
    // Opens on the resume page, summed under the layout's own estimates.
    let resume_index = {
        let count0 = state.document.num_pages.get_untracked() as usize;
        ((state.viewer.page.get_untracked().max(1) as usize) - 1).min(count0)
    };
    let mut v_off = 0.0;
    let mut h_off = 0.0;
    for index in 0..resume_index {
        v_off += estimate(index);
        h_off += h_estimate(index);
    }
    // Milliseconds, not frames: a commit's relayouts are wall-clock paced.
    let virtualizer = use_virtualizer(
        VirtualizerOptions::list(count, estimate)
            .gap(0.0)
            .budget(RENDER_BUDGET)
            .initial(Viewport::main_only(initial_vh), v_off)
            .pinned(pinned_sig.into())
            .epoch(epoch)
            .retention(STRIP_RETENTION),
    );

    let h_virtualizer = use_virtualizer(
        VirtualizerOptions::list(count, h_estimate)
            .axis(virtual_list_leptos::Axis::Horizontal)
            .gap(0.0)
            .budget(RENDER_BUDGET)
            .padding(0.0, 0.0)
            .initial(Viewport::new(1200.0, initial_vh), h_off)
            .epoch(epoch)
            .retention(STRIP_RETENTION),
    );
    let h_virtualizer_view = StoredValue::new_local(h_virtualizer.clone());

    // A zoom-out or mode flip renders nothing: sweep outside the keepalive
    // window when scrolling settles.
    #[cfg(feature = "pdf")]
    {
        let vertical = virtualizer.clone();
        let horizontal = h_virtualizer.clone();
        let samples_v = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let samples_h = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let est_v = std::rc::Rc::new(std::cell::Cell::new(EST_FILL_LATENCY_MS));
        let est_h = std::rc::Rc::new(std::cell::Cell::new(EST_FILL_LATENCY_MS));
        let est_v_hint = est_v.clone();
        virtualizer.on_scroll_idle(move || {
            if let Some(keep) = vertical.window().get_untracked().map(|w| w.expanded(1)) {
                pane.pdf().sweep_keepalive(keep.first, keep.last);
            } else {
                pane.pdf().sweep();
            }
            if state.viewer.page_gap.try_get_untracked().is_some() {
                note_fill_profile(&pane, &vertical, &samples_v, &est_v);
            }
        });
        h_virtualizer.on_scroll_idle(move || {
            if let Some(keep) = horizontal.window().get_untracked().map(|w| w.expanded(1)) {
                pane.pdf().sweep_keepalive(keep.first, keep.last);
            } else {
                pane.pdf().sweep();
            }
            if state.viewer.page_gap.try_get_untracked().is_some() {
                note_fill_profile(&pane, &horizontal, &samples_h, &est_h);
            }
        });
        let v_for_hint = virtualizer.clone();
        let last_hint = std::rc::Rc::new(std::cell::Cell::new(0u32));
        Effect::new(move |_| {
            let _ = v_for_hint.scroll_offset().get();
            if state.reflowable() {
                return;
            }
            let Some(idx) = v_for_hint.landing_hint(est_v_hint.get()) else {
                return;
            };
            let page = (idx + 1) as u32;
            if last_hint.replace(page) != page {
                prefetch_neighborhood(pane, state, page);
            }
        });
    }

    // The registry never outlives an owner: cleanup drops these entries.
    crate::diagnostics::track_virtualizer(&virtualizer);
    crate::diagnostics::track_virtualizer(&h_virtualizer);
    // The pane disposes them explicitly; component cleanups are the net.
    pane.track_virtualizer(&virtualizer);
    pane.track_virtualizer(&h_virtualizer);
    let tracked_v = StoredValue::new_local(virtualizer.clone());
    let tracked_h = StoredValue::new_local(h_virtualizer.clone());
    on_cleanup(move || {
        tracked_v.with_value(crate::diagnostics::untrack_virtualizer);
        tracked_h.with_value(crate::diagnostics::untrack_virtualizer);
        pane.untrack_virtualizer(&tracked_v.get_value());
        pane.untrack_virtualizer(&tracked_h.get_value());
    });

    {
        let v = virtualizer.clone();
        Effect::new(move |_| {
            let mut pin = None;
            if state.viewer.zoom.transition.get().is_some() {
                let dominant = v.dominant().get_untracked();
                pin = Some((dominant, dominant));
            }
            if let Some((first, last)) = state.viewer.selected_pages.get() {
                let selected = (
                    first.saturating_sub(1) as usize,
                    last.saturating_sub(1) as usize,
                );
                pin = Some(match pin {
                    Some((a, b)) => (a.min(selected.0), b.max(selected.1)),
                    None => selected,
                });
            }
            pinned_sig.set(pin);
        });
    }

    ReaderVirtualizers {
        virtualizer: virtualizer.clone(),
        h_virtualizer: h_virtualizer.clone(),
        virtualizer_view: StoredValue::new_local(virtualizer),
        h_virtualizer_view,
    }
}
