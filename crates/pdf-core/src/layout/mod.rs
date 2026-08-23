//! Pure layout math shared by both view modes (single page + continuous).
//! No wasm deps — unit-testable on the host.
//!
//! The continuous layout is a vertical column of pages separated by `gap` px:
//!   * `document`      — the cached [`DocumentLayout`] (structure + queries)
//!   * `anchor`        — the anchored-scroll zoom math
//!   * `viewport`      — which thumbnail-grid rows overlap the viewport
//!   * here            — shared constants/types plus the one-shot
//!     [`page_top_css`] / [`anchored_scroll`] wrappers for cold callers

mod anchor;
mod document;
mod viewport;

pub use document::DocumentLayout;
pub use virtual_list::{Budget, Strip};
pub use viewport::visible_grid_rows;

pub const PAGE_GAP: f64 = 24.0;

/// Height of the glass toolbar, in CSS px. The viewer scrollport spans the
/// full window height so pages travel under the translucent header; each view
/// offsets its content by this inset.
///
/// MUST stay in sync with Tailwind `h-12` / `mt-12` on the title bar
/// (`src/components/layout/title_bar.rs`) and the page-list offset
/// (`src/components/pdf/page_list.rs`).
pub const TOOLBAR_H: f64 = 48.0;

/// Read-ahead budget in screenfuls, not pages: a fixed page count means a
/// modest read-ahead when zoomed out and several screens of wasted rasters
/// when zoomed in. `look_frac` is the read-ahead each way; `max_items` the ceiling.
pub type RenderBudget = Budget;

/// Comfortable read-ahead: half a screenful each way, up to 3 mounted pages
/// total (visible + ~1 above + ~1 below). Each mounted page at 2× DPR plus
/// its raw is ~64MB worst case, so the ceiling is what keeps idle RAM sane.
pub const RENDER_BUDGET: RenderBudget = RenderBudget {
    look_frac: 0.5,
    max_items: 3,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Single,
    Continuous,
}

pub fn page_top_css(page0: usize, heights: &[f64], gap: f64) -> f64 {
    DocumentLayout::new(heights, gap).page_top(page0)
}

/// Scroll offset that keeps the document point currently at `anchor_screen_y`
/// (viewport coordinates, from the top of the scrollport) pinned to the same
/// screen position after every page height is multiplied by `factor`.
///
/// Page heights are linear in scale, so a scale change can be applied to the
/// whole layout in one synchronous step without waiting for a render; gaps are
/// chrome and deliberately NOT scaled. Returns `None` when there is nothing to
/// anchor (no measured heights, or a nonsensical factor).
pub fn anchored_scroll(
    scroll_top: f64,
    viewport_h: f64,
    heights: &[f64],
    gap: f64,
    factor: f64,
    anchor_screen_y: f64,
) -> Option<f64> {
    // One-shot wrapper; the hot path goes through
    // `DocumentLayout::anchored_scroll` on a cached layout.
    DocumentLayout::new(heights, gap).anchored_scroll(scroll_top, viewport_h, factor, anchor_screen_y)
}

#[cfg(test)]
mod tests {
    use super::*;

    const H: [f64; 3] = [100.0, 200.0, 100.0];

    #[test]
    fn page_top_and_total() {
        assert_eq!(page_top_css(0, &H, 24.0), 0.0);
        assert_eq!(page_top_css(1, &H, 24.0), 124.0);
        assert_eq!(page_top_css(2, &H, 24.0), 348.0);
        assert_eq!(DocumentLayout::new(&H, 24.0).total(), 448.0);
        assert_eq!(DocumentLayout::new(&[], 24.0).total(), 0.0);
    }
}

