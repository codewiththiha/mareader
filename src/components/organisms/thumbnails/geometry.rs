//! Fixed geometry and timing constants for the thumbnail grid, plus the
//! virtualization maths that depends on them.
//!
//! Split out of `thumbnails_panel.rs` so the numbers that define the grid —
//! and the reasoning behind each one — sit in one small, testable place
//! instead of scrolling past at the top of a 900-line component.

/// Render scale for thumbnails (CSS px per PDF unit).
pub const THUMB_SCALE: f64 = 0.25;
/// Fixed CSS-px width of each thumbnail cell. Fits two abreast in the w-72
/// sidebar: 288 - 2*12 padding - 12 gap = 252; 2 * 120 + 12 = 252.
pub const CELL_W: f64 = 120.0;
/// CSS-px gap between rows (the page-number band lives inside each cell).
pub const ROW_GAP: f64 = 8.0;
/// Extra rows rendered above/below the visible window (pre-render margin).
///
/// Two, not one. With a single buffer row the row that scrolls into view is the
/// one mounted an instant earlier, so on a fast scroll it is still mid-render
/// when the user first sees it — the row appears, then settles. Two rows of lead
/// time means a genuinely new row has finished (and been cached) before it
/// reaches the viewport edge. It costs nothing on revisits: cached rows blit
/// synchronously.
pub const ROW_BUFFER: usize = 2;
/// CSS-px padding on the scroll container (`p-3`). Rows are inset by this and
/// positioned from the content box, so the virtualization math stays exact.
pub const PAD: f64 = 12.0;
/// Debounce for the auto-center glide: the scroll fires once this long after
/// page writes have settled. In continuous mode the reader writes
/// `viewer.page` at every row boundary, so an immediate smooth scroll per
/// write would keep re-starting the in-flight glide and churn the
/// virtualization window; cancel-and-reschedule (debounce) yields exactly one
/// glide, shortly after the reader pauses.
pub const GLIDE_DEBOUNCE_MS: u64 = 150;
/// User-drive grace window: while the user has interacted with the thumb grid
/// within this many ms, auto-center defers (and re-checks) instead of yanking
/// the panel away from the row they are browsing. A page change that lands
/// inside the grace is NOT dropped — it is centered once the grace lapses.
pub const GRACE_MS: f64 = 1500.0;
/// Delay (ms) before the skeleton pulse is removed after a thumbnail render
/// resolves. The cover's opacity fade runs `duration-300` (300 ms); this sits
/// just past it so the pulse keeps running through the whole fade and the
/// class is only dropped once the cover is fully transparent — removing it
/// earlier would CANCEL the running `background-color` animation and snap the
/// cover back to base mid-fade (the flicker this code eliminates). Bounded
/// one-shot, not a forever-animation, so idle cells don't pulse indefinitely.
pub const PULSE_STOP_MS: u64 = 400;

/// Height of one grid row (thumbnail + the gap beneath it) for a page whose
/// aspect ratio is `aspect` (height / width).
pub fn row_height(aspect: f64) -> f64 {
    CELL_W * aspect + ROW_GAP
}

/// Number of 2-column rows needed for `pages` pages.
pub fn row_count(pages: usize) -> usize {
    pages.div_ceil(2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_geometry_holds() {
        // Two cells per row, rounding up.
        assert_eq!(row_count(0), 0);
        assert_eq!(row_count(1), 1);
        assert_eq!(row_count(4), 2);
        assert_eq!(row_count(5), 3);
        // Two cells and the gap fit the w-72 sidebar's content box.
        assert!(2.0 * CELL_W + ROW_GAP + 4.0 <= 288.0 - 2.0 * PAD);
        // Row height follows the page aspect: portrait is taller than landscape.
        assert!(row_height(842.0 / 595.0) > row_height(612.0 / 792.0));
        assert_eq!(row_height(1.0), CELL_W + ROW_GAP);
    }
}
