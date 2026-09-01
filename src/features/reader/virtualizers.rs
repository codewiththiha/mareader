//! Builds and wires the reader's two virtualizers: the vertical scroll list
//! and the horizontal strip. Extracted out of `ReaderPage` so the page
//! component stays a layout + effect coordinator rather than a setup pile.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use leptos::prelude::*;
use virtual_list::Viewport;
use virtual_list_leptos::{VirtualizerOptions, use_virtualizer};

use pdf_core::layout::RENDER_BUDGET;

use crate::state::ReaderState;
use crate::viewer::zoom::config::{MAX_ZOMBIES, STRIP_SCROLL_GRACE_MS};

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

/// Keeps `css_heights` fully seeded from intrinsic sizes: it is the shared
/// measurement store backing the vertical virtualizer and the zoom commit
/// path, not a second layout model.
///
/// The seed is keyed on CONTENT — page count plus the intrinsic geometry
/// signature — not on any per-frame signal. The zoom scale moves on every
/// slider tick and the zoom coordinator rescales `css_heights` itself per
/// frame, so re-seeding on scale would clobber live measured/zoomed heights
/// (the old length-only guard had the same purpose but ALSO meant a fresh
/// document with the same page count kept the previous book's heights until
/// its first pages happened to re-measure). Keying on the signature re-seeds
/// exactly when the book changes and never during a zoom gesture.
fn seed_css_heights(state: ReaderState) {
    let last_key = StoredValue::new_local(None::<u64>);
    Effect::new(move || {
        let count = state.document.num_pages.get() as usize;
        let scale = state.viewer.zoom.display.get();
        let empty_intrinsic = state.document.metrics.intrinsic.with(|sizes| sizes.is_empty());
        let fallback = state
            .document
            .page1_size
            .get()
            .map(|size| size.height)
            .unwrap_or(0.0);
        if count == 0 || scale <= 0.0 || (empty_intrinsic && fallback <= 0.0) {
            return;
        }
        let key = {
            let mut hasher = DefaultHasher::new();
            count.hash(&mut hasher);
            state.document.metrics.intrinsic.with(|sizes| {
                sizes.len().hash(&mut hasher);
                for size in sizes {
                    size.width.to_bits().hash(&mut hasher);
                    size.height.to_bits().hash(&mut hasher);
                }
            });
            hasher.finish()
        };
        if last_key.with_value(|stored| *stored == Some(key)) {
            return;
        }
        last_key.set_value(Some(key));
        state.document.metrics.css_heights.update(|heights| {
            *heights = state.document.metrics.intrinsic.with(|sizes| {
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
        });
    });
}

pub(crate) fn use_reader_virtualizers(state: ReaderState) -> ReaderVirtualizers {
    seed_css_heights(state);

    let count = Signal::derive(move || state.document.num_pages.get() as usize);
    let estimate = move |index: usize| {
        let measured = state
            .document
            .metrics
            .css_heights
            .with_untracked(|heights| heights.get(index).copied());
        if let Some(height) = measured.filter(|height| *height > 0.0) {
            return height + state.viewer.page_gap.get_untracked();
        }
        let intrinsic = state
            .document
            .metrics
            .intrinsic
            .with_untracked(|sizes| sizes.get(index).map(|size| size.height))
            .filter(|height| *height > 0.0);
        let fallback = state
            .document
            .page1_size
            .get_untracked()
            .map(|size| size.height)
            .unwrap_or(0.0);
        intrinsic.unwrap_or(fallback) * state.viewer.zoom.visual_scale()
            + state.viewer.page_gap.get_untracked()
    };
    let epoch = Signal::derive(move || {
        let count = state.document.num_pages.get() as usize;
        let mut hasher = DefaultHasher::new();
        count.hash(&mut hasher);
        state.document.metrics.intrinsic.with(|sizes| {
            sizes.len().hash(&mut hasher);
            for size in sizes {
                size.width.to_bits().hash(&mut hasher);
                size.height.to_bits().hash(&mut hasher);
            }
        });
        hasher.finish()
    });
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
        state.document.metrics.intrinsic.with_untracked(|sizes| {
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
