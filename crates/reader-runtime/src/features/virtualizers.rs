//! The reader's two virtualizers: one pair per pane, built at mount and
//! ended at dispose.

use std::hash::Hash;

use leptos::prelude::*;
use virtual_list::{Budget, Viewport};
use virtual_list_leptos::{RetentionPolicy, VirtualizerOptions, use_virtualizer};

use crate::state::ReaderState;
use crate::zoom::config::MAX_ZOMBIES;
use app_ui::epoch::epoch_signal;

/// ~64MB per mounted page at 2× DPR: the ceiling is what bounds idle RAM.
pub(crate) const RENDER_BUDGET: Budget = Budget::screenfuls(0.5, 3);

/// Bridged only mid-seek; a zoom commit raises a timed Grace over it.
const STRIP_RETENTION: RetentionPolicy = RetentionPolicy::MotionGated { max: MAX_ZOMBIES };

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

/// What one settled page costs here, and how wide the lane that serves it.
#[cfg(feature = "pdf")]
fn note_fill_profile(
    pane: &crate::pane::handle::PaneHandle,
    strip: &virtual_list_leptos::Virtualizer,
    last: &std::rc::Rc<std::cell::Cell<f64>>,
) {
    let Some(stats) = pane.pdf().stats() else {
        return;
    };
    // 0 is "not measured", not "instant": a zero never engages the band.
    if stats.fill_ms <= 0.0 {
        return;
    }
    // Once per strip: a report re-evaluates every mounted page of the band.
    if last.get() >= 0.0 {
        return;
    }
    last.set(stats.fill_ms);
    strip.set_fill_profile(stats.fill_ms, stats.page_limit.max(1) as usize);
}

pub(crate) fn use_reader_virtualizers(
    state: ReaderState,
    pane: crate::pane::handle::PaneHandle,
) -> ReaderVirtualizers {
    seed_css_heights(state);

    let count = Signal::derive(move || state.document.num_pages.get() as usize);
    let estimate = move |index: usize| {
        // Teardown can call this; `page_gap` gone means every read below is gone.
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

    // A zoom-out or mode flip renders nothing: sweep when scrolling settles.
    #[cfg(feature = "pdf")]
    {
        let vertical = virtualizer.clone();
        let horizontal = h_virtualizer.clone();
        let applied_v = std::rc::Rc::new(std::cell::Cell::new(-1.0));
        let applied_h = std::rc::Rc::new(std::cell::Cell::new(-1.0));
        virtualizer.on_scroll_idle(move || {
            pane.pdf().sweep();
            if state.viewer.page_gap.try_get_untracked().is_some() {
                note_fill_profile(&pane, &vertical, &applied_v);
            }
        });
        h_virtualizer.on_scroll_idle(move || {
            pane.pdf().sweep();
            if state.viewer.page_gap.try_get_untracked().is_some() {
                note_fill_profile(&pane, &horizontal, &applied_h);
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

// only the changed file was rewritten
