//! [`ZoomActuator`]: the single owner of the virtualized scroll geometry.
//!
//! Before this module existed, views, zoom, fit and navigation each reached
//! into the virtualizers and the DOM directly, and zoom duplicating the
//! axis-branching relayout across two code paths was the root of the races.
//! The split that replaced it is the whole subsystem's contract: everything
//! that must MOVE geometry (rescale a strip, hold the anchor, report a
//! rendered size) goes through the actuator, and everything that DECIDES what
//! the zoom should be goes through [`super::ZoomController`]. Nothing outside
//! the two should touch a virtualizer's layout or a zoom scale.
//!
//! The core contract is that the actuator is the *only* place layout is
//! rescaled. Zoom and fit compute a *target* and ask it to apply a scale
//! factor; it owns the relayout so a gesture and a refit cannot diverge along
//! separate code paths. Non-rescale geometry reads (dominant page,
//! scroll-to-page) still go through the virtualizers directly, but only in the
//! per-mode navigation code.
//!
//! It runs on EVERY frame of a zoom: the tween hands the actuator the ratio between
//! the scale it is about to show and the scale the layout currently has, and
//! the layout follows continuously. Scaling the layout for real is what
//! keeps a zoom stable. The alternative — one CSS transform over a surface
//! whose geometry is frozen — cannot work here, because a transform scales
//! the page gaps along with the pages while the layout deliberately does
//! not (see `virtual_list::anchor::rescale_anchor`). Every gap above the
//! reader accumulates error through the tween and the whole sum lands at
//! once when the transform is swapped for real geometry, which reads as the
//! document jumping.
//!
//! Anchoring is therefore explicit and gap-aware: the actuator works out which
//! document point sits under the viewport centre, rescales, finds where that
//! same point lands in the new geometry, and puts it back under the viewport
//! centre. Page interiors scale; the fixed gap between pages does not.
//!
//! Because this runs every frame, the anchored page is resolved in `O(log n)`
//! from the strip's own prefix sums (`index_at`) and the column is never
//! copied: the anchor reads the pre-scale store once, the store is then scaled
//! in place, and the strip rebuild reads the now-scaled values. The anchor
//! cannot be left to the virtualizer either — that one scales item extents,
//! and the strips fold the page gap *into* the item size (their `gap` is `0.0`
//! and `report_size` is handed `height + gap`), so a uniform rescale there
//! scales the chrome too. Nor can the scroll write be deferred a frame: layout
//! and scroll have to move in the same tick.

use leptos::prelude::*;
use reader_core::view::{ViewMode, anchored_position};
use virtual_list_leptos::{ScrollMode, Virtualizer};

use crate::state::reader::ReaderState;

/// Wraps the reader's two strip virtualizers and centralises the reader's one
/// relayout path. The vertical (continuous) and horizontal strips stay as
/// separate virtualizers — they are created as separate hooks in
/// `ReaderPage` — but resizing a strip's items is done only here.
#[derive(Clone)]
pub struct ZoomActuator {
    /// The continuous (vertical) strip's virtualizer.
    pub vertical: Virtualizer,
    /// The horizontal strip's virtualizer.
    pub horizontal: Virtualizer,
}

impl ZoomActuator {
    pub fn new(vertical: Virtualizer, horizontal: Virtualizer) -> Self {
        Self { vertical, horizontal }
    }

    /// Rescale both strips by `factor` — the ratio between the new and the
    /// current layout scale — holding the document point under the viewport
    /// centre exactly where it is.
    pub fn relayout_to(&self, state: &ReaderState, factor: f64) {
        if factor <= 0.0 || !factor.is_finite() || (factor - 1.0).abs() < 1e-12 {
            return; // already at this geometry; nothing to move
        }

        self.relayout_vertical(state, factor);

        // Horizontal strip: only the scroll-horizontal mode mounts it, so in
        // every other mode rebuilding its widths — a per-frame `Vec` collect —
        // would be dead work on every frame of a zoom. Gate it on the one mode
        // that owns the horizontal strip.
        if state.viewer.mode.get_untracked() != ViewMode::ScrollHorizontal {
            return;
        }

        // Widths are exact (intrinsic × scale + margin), so they are rebuilt
        // from the intrinsic sizes at the scale this relayout lands on rather
        // than from a scaled copy of the previous width — that keeps the
        // running product free of drift across the many small factors a single
        // tween applies. One copy of the widths is taken because the
        // virtualizer reads the sizes once per item, and an array read beats a
        // signal read per page. The virtualizer's own rescale anchor holds the
        // cross-axis position.
        let margin = state.viewer.page_margin.get_untracked();
        let widths = state.document.content.metrics.intrinsic.with_untracked(|sizes| {
            sizes.iter().map(|s| s.width).collect::<Vec<f64>>()
        });
        let new_scale = state.viewer.zoom.visual_scale() * factor;
        if !widths.is_empty() {
            self.horizontal.rescale(factor, move |index| {
                widths.get(index).copied().unwrap_or(0.0) * new_scale + 2.0 * margin
            });
            let hv = self.horizontal.clone();
            let h_scroll = self.horizontal.scroll_offset().get_untracked();
            request_animation_frame(move || {
                hv.scroll_to_offset(h_scroll, ScrollMode::Instant);
            });
        }
    }

    /// Rescale the vertical strip and put the document point that was under
    /// the viewport centre back under the viewport centre.
    ///
    /// The anchor is computed here rather than left to the virtualizer's own
    /// rescale anchor because it has to survive a scale the layout applies
    /// unevenly: page heights multiply by the factor, the gap between pages
    /// stays put. The anchored page is resolved in `O(log n)` from the strip's
    /// own prefix sums (`index_at` + `offset_of`) rather than by walking the
    /// column, and the column itself is never copied: the anchor reads the
    /// pre-scale store once, the store is then scaled in place, and the strip
    /// rebuild reads the now-scaled values.
    fn relayout_vertical(&self, state: &ReaderState, factor: f64) {
        let gap = state.viewer.page_gap.get_untracked();
        let (_, vh) = state.viewer.container_size.get_untracked();
        let scroll_top = self.vertical.scroll_offset().get_untracked();

        // The strip's content starts at the scroller's origin — the title bar
        // is an overlay that reveals on hover, not a band the pages sit under
        // — so the content point under the middle of the window is exactly
        // half the height down.
        let centre_in_viewport = vh / 2.0;
        let centre_y_doc = (scroll_top + centre_in_viewport).max(0.0);

        // Resolve the page under the viewport centre and where that point lands
        // once every page has scaled, in a single borrow of the pre-scale
        // store. The strip folds the page gap into each item's size (its own
        // `gap` is 0), so `offset_of(index)` is the extent of the pages above
        // WITH their gaps; subtracting `index * gap` recovers their heights
        // alone — the part that scales.
        let anchored = state.document.content.metrics.css_heights.with_untracked(|heights| {
            if heights.is_empty() {
                return None;
            }
            let index = self.vertical.index_at(centre_y_doc).min(heights.len() - 1);
            let height = heights[index];
            let above_with_gap = self.vertical.offset_of(index);
            let height_sum = above_with_gap - index as f64 * gap;
            Some(anchored_position(
                height,
                above_with_gap,
                height_sum,
                gap,
                centre_y_doc,
                factor,
                index,
            ))
        });
        let Some(new_centre_y_doc) = anchored else {
            return; // nothing measured yet; no layout to hold still
        };

        // Scale the shared measurement store, then rebuild the strip's layout
        // from it. The rebuild reads the now-scaled store, so the column is
        // not copied a second time.
        state.document.content.metrics.css_heights.update(|store| {
            for height in store.iter_mut() {
                *height *= factor;
            }
        });
        self.vertical.rescale(factor, state.document.content.metrics.strip_sizes(gap));

        // Scroll so the anchored point is back under the middle of the window.
        // The ceiling is the virtualizer's own (`total − viewport`):
        // `scroll_to_offset` clamps to it, so adopting a larger range here
        // would leave `viewer.scroll_top` disagreeing with the offset that
        // actually landed at the very end of a document.
        let max_scroll = (self.vertical.total_size().get_untracked() - vh).max(0.0);
        let new_scroll_top = (new_centre_y_doc - centre_in_viewport).clamp(0.0, max_scroll);

        if (new_scroll_top - state.viewer.scroll_top.get_untracked()).abs() >= 0.5 {
            state.viewer.scroll_top.set(new_scroll_top);
        }

        // Synchronous: `rescale` has already updated the virtualizer's layout
        // and signals in this tick, so commanding the offset now lands on the
        // right frame. Deferring it left a one-frame gap where the geometry
        // had moved and the scroll had not.
        self.vertical
            .scroll_to_offset(new_scroll_top, ScrollMode::Instant);

        // Growing content: re-assert one frame later. The spacer that gives
        // the scroller its scroll extent is patched by Leptos only after
        // `rescale` returns, so the browser clamps the write against the
        // still-short old extent — worst at the end of a growing document.
        if factor > 1.0 {
            let v = self.vertical.clone();
            request_animation_frame(move || {
                v.scroll_to_offset(new_scroll_top, ScrollMode::Instant);
            });
        }
    }
}

