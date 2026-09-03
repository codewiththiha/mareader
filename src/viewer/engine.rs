//! [`ViewerEngine`]: the single owner of the virtualized scroll geometry.
//!
//! The core contract is that the engine is the *only* place layout is
//! rescaled. Zoom and fit compute a *target* and ask the engine to apply a
//! scale factor; the engine owns the relayout so a gesture and a refit
//! cannot diverge along separate code paths. Non-rescale geometry reads
//! (dominant page, scroll-to-page) still go through the virtualizers
//! directly, but only in the per-mode navigation code.
//!
//! It runs on EVERY frame of a zoom: the tween hands it the ratio between
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
//! Anchoring is therefore explicit and gap-aware: the engine works out which
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
use pdf_core::layout::ViewMode;
use virtual_list_leptos::{ScrollMode, Virtualizer};

use crate::state::reader::ReaderState;

/// Wraps the reader's two strip virtualizers and centralises the viewer's one
/// relayout path. The vertical (continuous) and horizontal strips stay as
/// separate virtualizers — they are created as separate hooks in
/// `ReaderPage` — but resizing a strip's items is done only here.
#[derive(Clone)]
pub struct ViewerEngine {
    /// The continuous (vertical) strip's virtualizer.
    pub vertical: Virtualizer,
    /// The horizontal strip's virtualizer.
    pub horizontal: Virtualizer,
}

impl ViewerEngine {
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
        let widths = state.document.metrics.intrinsic.with_untracked(|sizes| {
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
        let anchored = state.document.metrics.css_heights.with_untracked(|heights| {
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
        state.document.metrics.css_heights.update(|store| {
            for height in store.iter_mut() {
                *height *= factor;
            }
        });
        let css_heights = state.document.metrics.css_heights;
        self.vertical.rescale(factor, move |index| {
            css_heights.with_untracked(|heights| heights.get(index).copied().unwrap_or(0.0)) + gap
        });

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

/// Where the document point that was under the viewport centre lands once the
/// page it sits on has been scaled by `factor` and the gaps between pages have
/// been left alone.
///
/// `index` is the anchored page, already resolved in `O(log n)` by the strip's
/// `index_at`; `height` is that page's pre-scale height, `above_with_gap` the
/// extent of the pages above it (gaps included) and `height_sum` their heights
/// alone — the part that scales. Keeping the arithmetic here, divorced from any
/// strip or signal, is what makes the gap-aware anchoring host-testable.
fn anchored_position(
    height: f64,
    above_with_gap: f64,
    height_sum: f64,
    gap: f64,
    centre_y_doc: f64,
    factor: f64,
    index: usize,
) -> f64 {
    // Where the pages above land at the new scale, plus this point's offset
    // inside them. An anchor that fell in the gap keeps the unscaled
    // remainder: the gap is fixed chrome and never scales.
    let above = height_sum * factor + index as f64 * gap;
    let offset_inside = centre_y_doc - above_with_gap;
    above + if offset_inside <= height {
        offset_inside * factor
    } else {
        height * factor + (offset_inside - height)
    }
}

#[cfg(test)]
mod tests {
    use super::anchored_position;

    /// A point inside a page moves with the page: page 0 spans 0..100, so 40
    /// inside it lands at 80 after a 2× zoom.
    #[test]
    fn an_anchor_on_a_page_scales_with_it() {
        assert_eq!(anchored_position(100.0, 0.0, 0.0, 20.0, 40.0, 2.0, 0), 80.0);
    }

    /// The load-bearing case: the gap between pages is fixed chrome, so an
    /// anchor that falls in it is carried along by the pages above it at their
    /// scale and keeps the unscaled remainder of the gap. A uniform rescale of
    /// the whole extent would put it at 110 × 2 = 220 instead.
    #[test]
    fn an_anchor_in_a_gap_keeps_the_gap_unscaled() {
        // Page 0 ends at 100, the gap spans 100..120; 110 is 10 into the gap,
        // so after doubling, page 0 ends at 200 and the gap is still 20.
        assert_eq!(anchored_position(100.0, 0.0, 0.0, 20.0, 110.0, 2.0, 0), 210.0);
    }

    /// Every gap above the reader counts, not just the one it is standing in:
    /// deep in a long document the unscaled sum is what keeps the page still.
    #[test]
    fn gaps_above_the_anchor_hold_the_page_still() {
        // Page 5 starts at 5 * (100 + 20) = 600; +30 into it is 630, which
        // scales to 5 * 200 + 5 * 20 + 60 = 1160.
        assert_eq!(anchored_position(100.0, 600.0, 500.0, 20.0, 630.0, 2.0, 5), 1160.0);
        // Zooming back out by the same factor returns to the exact start.
        let forward = anchored_position(100.0, 600.0, 500.0, 20.0, 630.0, 2.0, 5);
        assert_eq!(anchored_position(200.0, 1100.0, 1000.0, 20.0, forward, 0.5, 5), 630.0);
    }

    /// A centre past the end of a short document still lands at the scaled end,
    /// keeping the overflow beyond the single page exactly as long as it was.
    #[test]
    fn a_centre_past_the_end_keeps_the_overflow_unscaled() {
        // 900 is far past the single page: the page scales to 200 and the 800
        // of overflow beyond it stays exactly as long as it was.
        assert_eq!(anchored_position(100.0, 0.0, 0.0, 20.0, 900.0, 2.0, 0), 1000.0);
    }
}
