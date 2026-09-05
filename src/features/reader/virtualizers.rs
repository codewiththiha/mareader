//! Builds and wires the reader's two virtualizers: the vertical scroll list
//! and the horizontal strip. Extracted out of `ReaderPage` so the page
//! component stays a layout + effect coordinator rather than a setup pile.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use leptos::prelude::*;
use virtual_list::Viewport;
use virtual_list_leptos::{VirtualizerOptions, use_virtualizer};

use reader_core::view::RENDER_BUDGET;

use crate::state::ReaderState;
use crate::zoom::config::{MAX_ZOMBIES, STRIP_SCROLL_GRACE_MS};

/// The handles `ReaderPage` hands to the viewer components and effects. Both
/// virtualizers always exist (they are hooks); a view binds only the one for
/// its axis when it mounts. The `StoredValue`s are the `Clone`-safe wrappers
/// the components pass by value, while the raw handles drive the effects.
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
        .content.metrics.page1_size
        .get_untracked()
        .map_or(0.0, |size| size.height)
}

/// Keeps `css_heights` — the shared measurement store behind the vertical
/// virtualizer and the zoom commit path — filled from the intrinsic sizes.
///
/// The rule is simply "an empty store gets seeded": the open flow empties it
/// for every new document (the same book included), the zoom coordinator
/// rescales it in place, and the pages overwrite entries as they measure.
/// So emptiness is exactly "a book just arrived", and nothing else — not a
/// zoom tick, not a re-measure — can trigger a re-seed that would clobber
/// live heights.
fn seed_css_heights(state: ReaderState) {
    Effect::new(move || {
        let count = state.document.num_pages.get() as usize;
        let filled = state.document.content.metrics.css_heights.with(|heights| !heights.is_empty());
        let scale = state.viewer.zoom.display.get();
        if filled || count == 0 || scale <= 0.0 {
            return;
        }
        // Tracked reads: the store is only worth filling once the sizes are
        // there, and they arrive in the same open that emptied it.
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

/// A fingerprint of the document's geometry: page count plus every
/// intrinsic size. The virtualizers rebuild their layouts when it changes.
fn geometry_epoch(state: ReaderState) -> Signal<u64> {
    Signal::derive(move || {
        let mut hasher = DefaultHasher::new();
        state.document.num_pages.get().hash(&mut hasher);
        state.document.content.metrics.intrinsic.with(|sizes| {
            sizes.len().hash(&mut hasher);
            for size in sizes {
                size.width.to_bits().hash(&mut hasher);
                size.height.to_bits().hash(&mut hasher);
            }
        });
        hasher.finish()
    })
}

pub(crate) fn use_reader_virtualizers(state: ReaderState) -> ReaderVirtualizers {
    seed_css_heights(state);

    let count = Signal::derive(move || state.document.num_pages.get() as usize);
    let estimate = move |index: usize| {
        let gap = state.viewer.page_gap.get_untracked();
        let measured = state
            .document
            .content.metrics
            .css_heights
            .with_untracked(|heights| heights.get(index).copied())
            .filter(|height| *height > 0.0);
        if let Some(height) = measured {
            return height + gap;
        }
        let intrinsic = state
            .document
            .content.metrics
            .intrinsic
            .with_untracked(|sizes| sizes.get(index).map(|size| size.height))
            .filter(|height| *height > 0.0);
        intrinsic.unwrap_or_else(|| fallback_height(state)) * state.viewer.zoom.visual_scale() + gap
    };
    let epoch = geometry_epoch(state);
    let pinned_sig: RwSignal<Option<(usize, usize)>> = RwSignal::new(None);
    let initial_vh = {
        let (_, height) = state.viewer.container_size.get_untracked();
        if height > 1.0 {
            height
        } else {
            800.0
        }
    };
    // Zombie retention: an item that leaves the window mid-fling (or in a
    // zoom's geometry commit — the controller raises the grace for that)
    // keeps its DOM briefly instead of popping out.
    let virtualizer = use_virtualizer(
        VirtualizerOptions::list(count, estimate)
            .gap(0.0)
            .budget(RENDER_BUDGET)
            .initial(Viewport::main_only(initial_vh), 0.0)
            .pinned(pinned_sig.into())
            .epoch(epoch)
            .retention(STRIP_SCROLL_GRACE_MS, MAX_ZOMBIES),
    );

    // Horizontal virtualizer: created unconditionally (hook), bound only when the view mounts.
    // Both strips estimate from the live DISPLAY scale — the scale the
    // layout is relaid out to as a zoom runs — so the two axes can never
    // disagree about how big a page is.
    let h_estimate = move |index: usize| {
        state.document.content.metrics.intrinsic.with_untracked(|sizes| {
            sizes.get(index).map(|s| s.width).unwrap_or(0.0)
        }) * state.viewer.zoom.visual_scale()
            + 2.0 * state.viewer.page_margin.get_untracked()
    };
    let h_virtualizer = use_virtualizer(
        VirtualizerOptions::list(count, h_estimate)
            .axis(virtual_list_leptos::Axis::Horizontal)
            .gap(0.0)
            .budget(RENDER_BUDGET)
            .padding(0.0, 0.0)
            .initial(Viewport::new(1200.0, initial_vh), 0.0)
            .epoch(epoch)
            .retention(STRIP_SCROLL_GRACE_MS, MAX_ZOMBIES),
    );
    let h_virtualizer_view = StoredValue::new_local(h_virtualizer.clone());

    // The engine only sweeps its rasters inside render activity; after a
    // zoom-out or a mode flip nothing renders, so the big rasters would stay
    // pinned until the 30s idle timer. Sweep the moment scrolling settles
    // instead — both virtualizers, registered once (the views rebind the
    // SAME shared virtualizer on every mode flip).
    virtualizer.on_scroll_idle(pdf_engine::api::sweep);
    h_virtualizer.on_scroll_idle(pdf_engine::api::sweep);

    {
        let v = virtualizer.clone();
        Effect::new(move |_| {
            let mut pin = None;
            if state.viewer.zoom.transition.get().is_some() {
                let dominant = v.dominant().get_untracked();
                pin = Some((dominant, dominant));
            }
            if let Some((first, last)) = state.viewer.selected_pages.get() {
                let selected = (first.saturating_sub(1) as usize, last.saturating_sub(1) as usize);
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
