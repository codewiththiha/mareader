//! Fixed geometry for the thumbnail grid: cells, gaps, buffer
//! rows.

/// Render scale for thumbnails (CSS px per PDF unit).
pub const THUMB_SCALE: f64 = 0.25;
/// Fixed CSS-px width of each thumbnail cell. Fits two abreast in the w-72
/// sidebar.
pub const CELL_W: f64 = 120.0;
/// CSS-px gap between columns (`grid-cols-2 gap-3`).
pub const GAP_CROSS: f64 = 12.0;
/// CSS-px gap between rows (the page-number band lives inside each cell).
const ROW_GAP: f64 = 8.0;
/// Extra rows rendered above/below the visible window.
pub const ROW_BUFFER: usize = 2;
/// Fallback viewport height used before the bound container has reported its
/// real size.
pub const MIN_VIEWPORT_H: f64 = 720.0;
/// CSS-px padding on the scroll container (`p-3`).
pub const PAD: f64 = 12.0;

/// Height of one grid row for a page of this aspect ratio.
pub fn row_height(aspect: f64) -> f64 {
    CELL_W * aspect + ROW_GAP
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_geometry_holds() {
        const {
            assert!(2.0 * CELL_W + GAP_CROSS <= 288.0 - 2.0 * PAD);
        }
        assert!(row_height(842.0 / 595.0) > row_height(612.0 / 792.0));
        assert_eq!(row_height(1.0), CELL_W + ROW_GAP);
    }
}
