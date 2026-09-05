//! Fixed geometry for the thumbnail grid: cell size, gaps, buffer rows.
//!
//! This stays small and testable on purpose: the panel reads the numbers here,
//! while row windowing and scroll bookkeeping now live in
//! `virtual-list-leptos`.
//!
//! Timings are deliberately NOT here. A number that only one module reads is
//! that module's business — the glide's debounce and grace window live in
//! `auto_center`, the skeleton pulse's stop delay in `thumbnail_cell` — so the
//! constant sits next to the code that explains why it has that value.

/// Render scale for thumbnails (CSS px per PDF unit).
pub const THUMB_SCALE: f64 = 0.25;
/// Fixed CSS-px width of each thumbnail cell. Fits two abreast in the w-72
/// sidebar.
pub const CELL_W: f64 = 120.0;
/// CSS-px gap between columns (`grid-cols-2 gap-3`). Distinct from
/// [`ROW_GAP`]: cross and main gaps are different knobs in the adapter.
pub const GAP_CROSS: f64 = 12.0;
/// CSS-px gap between rows (the page-number band lives inside each cell).
const ROW_GAP: f64 = 8.0;
/// Extra rows rendered above/below the visible window (pre-render margin).
///
/// Two, not one. A row entering the viewport is already mounted, and a truly
/// new row shows its skeleton for the one render it takes; cached rows still
/// blit synchronously. The second buffer row means a fast grid fling meets warm
/// rows instead of skeletons.
pub const ROW_BUFFER: usize = 2;
/// Fallback viewport height used before the bound container has reported its
/// real size.
pub const MIN_VIEWPORT_H: f64 = 720.0;
/// CSS-px padding on the scroll container (`p-3`).
pub const PAD: f64 = 12.0;

/// Height of one grid row (thumbnail + the gap beneath it) for a page whose
/// aspect ratio is `aspect` (height / width).
pub fn row_height(aspect: f64) -> f64 {
    CELL_W * aspect + ROW_GAP
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_geometry_holds() {
        const { assert!(2.0 * CELL_W + GAP_CROSS <= 288.0 - 2.0 * PAD); }
        assert!(row_height(842.0 / 595.0) > row_height(612.0 / 792.0));
        assert_eq!(row_height(1.0), CELL_W + ROW_GAP);
    }
}
