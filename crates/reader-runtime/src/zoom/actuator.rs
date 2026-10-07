//! `ZoomActuator`: the one owner of the virtualized scroll geometry.

use leptos::prelude::*;
use reader_core::view::{ViewMode, anchored_position};
use virtual_list_leptos::{ScrollMode, Virtualizer};

use crate::pane::dom::PaneDom;
use crate::state::ReaderState;

/// The reader's two strip virtualizers and their one relayout path.
#[derive(Clone)]
pub struct ZoomActuator {
    pub vertical: Virtualizer,
    pub horizontal: Virtualizer,
    /// The pane whose strips these are; extents are looked up inside it.
    dom: PaneDom,
}

impl ZoomActuator {
    pub fn new(vertical: Virtualizer, horizontal: Virtualizer, dom: PaneDom) -> Self {
        Self {
            vertical,
            horizontal,
            dom,
        }
    }

    /// Rescale both strips by `factor`, holding the point under the viewport
    /// centre still.
    pub fn relayout_to(&self, state: &ReaderState, factor: f64) {
        if let Some(pending) = self.relayout(state, factor, Surface::Now) {
            self.write_scroll(pending);
        }
    }

    /// `relayout_to` minus the DOM: the offsets come back for
    /// `write_scroll`.
    pub(crate) fn relayout_detached(
        &self,
        state: &ReaderState,
        factor: f64,
    ) -> Option<PendingScroll> {
        self.relayout(state, factor, Surface::Later)
    }

    /// Put the strips' extents and scroll offsets where a relayout left them.
    pub(crate) fn write_scroll(&self, pending: PendingScroll) {
        if let Some((total, top)) = pending.vertical {
            // The spacer's extent patches after `rescale` returns, so bring
            // it to the new total first.
            apply_extent(self.dom, StripExtent::Vertical, total);
            self.vertical.scroll_to_offset(top, ScrollMode::Instant);
        }
        if let Some((total, left)) = pending.horizontal {
            apply_extent(self.dom, StripExtent::Horizontal, total);
            self.horizontal.scroll_to_offset(left, ScrollMode::Instant);
        }
    }

    fn relayout(
        &self,
        state: &ReaderState,
        factor: f64,
        surface: Surface,
    ) -> Option<PendingScroll> {
        if factor <= 0.0 || !factor.is_finite() || (factor - 1.0).abs() < 1e-12 {
            return None; // already at this geometry; nothing to move
        }

        let vertical = self.relayout_vertical(state, factor, surface);

        // Only scroll-horizontal mounts it; rebuilding widths elsewhere is dead
        // work.
        if state.viewer.mode.get_untracked() != ViewMode::ScrollHorizontal {
            return Some(PendingScroll {
                vertical,
                horizontal: None,
            });
        }

        // Widths are rebuilt from the intrinsic sizes at the landing scale, so
        // no drift.
        let margin = state.viewer.page_margin.get_untracked();
        let widths = state
            .document
            .content
            .metrics
            .intrinsic
            .with_untracked(|sizes| sizes.iter().map(|s| s.width).collect::<Vec<f64>>());
        let new_scale = state.viewer.zoom.visual_scale() * factor;
        let mut horizontal = None;
        if !widths.is_empty() {
            let sizes = move |index: usize| {
                widths.get(index).copied().unwrap_or(0.0) * new_scale + 2.0 * margin
            };
            match surface {
                // The rescale's write goes out against the OLD width,
                // so the pending write re-issues it.
                Surface::Now => self.horizontal.rescale(factor, sizes),
                Surface::Later => self.horizontal.rescale_detached(factor, sizes),
            }
            horizontal = Some((
                self.horizontal.total_size().get_untracked(),
                self.horizontal.scroll_offset().get_untracked(),
            ));
        }
        Some(PendingScroll {
            vertical,
            horizontal,
        })
    }

    /// Rescale the vertical strip, putting the point under the viewport
    /// centre back.
    fn relayout_vertical(
        &self,
        state: &ReaderState,
        factor: f64,
        surface: Surface,
    ) -> Option<(f64, f64)> {
        let gap = state.viewer.page_gap.get_untracked();
        let (_, vh) = state.viewer.container_size.get_untracked();
        let scroll_top = self.vertical.scroll_offset().get_untracked();

        // The content starts at the scroller's origin: the mid-window point is
        // half the height.
        let centre_in_viewport = vh / 2.0;
        let centre_y_doc = (scroll_top + centre_in_viewport).max(0.0);

        // Resolve the page under the centre in one borrow of the pre-scale
        // store.
        let anchored = state
            .document
            .content
            .metrics
            .css_heights
            .with_untracked(|heights| {
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
        let new_centre_y_doc = anchored?; // nothing measured yet; no layout to hold still

        // Scale the store, then rebuild the strip's layout from it.
        state.document.content.metrics.css_heights.update(|store| {
            for height in store.iter_mut() {
                *height *= factor;
            }
        });
        let sizes = state.document.content.metrics.strip_sizes(gap);
        match surface {
            Surface::Now => self.vertical.rescale(factor, sizes),
            Surface::Later => self.vertical.rescale_detached(factor, sizes),
        }

        // Scroll the anchored point back under the middle of the window.
        let total = self.vertical.total_size().get_untracked();
        let max_scroll = (total - vh).max(0.0);
        let new_scroll_top = (new_centre_y_doc - centre_in_viewport).clamp(0.0, max_scroll);

        if (new_scroll_top - state.viewer.scroll_top.get_untracked()).abs() >= 0.5 {
            state.viewer.scroll_top.set(new_scroll_top);
        }

        // `write_scroll` writes it: this tick for a tween, after the DOM
        // patches for a landing.
        Some((total, new_scroll_top))
    }
}

/// Where a relayout left the strips: `(extent, scroll offset)` per axis, for
/// [`ZoomActuator::write_scroll`].
#[derive(Clone, Copy, Debug)]
pub(crate) struct PendingScroll {
    vertical: Option<(f64, f64)>,
    horizontal: Option<(f64, f64)>,
}

/// When a relayout touches the scroll surface.
#[derive(Clone, Copy)]
enum Surface {
    /// In the relayout's own tick (tween frames, container follows).
    Now,
    /// The caller writes the returned offsets after the DOM patches.
    Later,
}

/// The element that gives a strip its scroll extent (strip.rs marks it).
#[derive(Clone, Copy)]
enum StripExtent {
    /// The vertical strip's spacer: its height is the strip's total.
    Vertical,
    /// The horizontal strip's track: its width is the strip's total.
    Horizontal,
}

/// Write a strip's scroll extent ahead of Leptos' patch of the same
/// value.
fn apply_extent(dom: PaneDom, extent: StripExtent, total: f64) {
    let (selector, property) = match extent {
        StripExtent::Vertical => ("[data-strip-extent=\"vertical\"]", "height"),
        StripExtent::Horizontal => ("[data-strip-extent=\"horizontal\"]", "width"),
    };
    let Some(el) = dom
        .select(selector)
        .and_then(|el| wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlElement>(el).ok())
    else {
        return;
    };
    // Called through the inherent method: the Leptos prelude's `ElementExt`
    // also names a `style`.
    let _ = web_sys::HtmlElement::style(&el).set_property(property, &format!("{total}px"));
}
