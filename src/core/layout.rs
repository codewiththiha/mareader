//! Pure layout math shared by both view modes (single page + continuous).
//! No wasm deps — unit-testable on the host.
//!
//! The continuous layout is a vertical column of pages separated by `gap` px.
//! `heights` holds each page's rendered CSS-px height (0-based index), filled
//! lazily as pages report their geometry.

pub const PAGE_GAP: f64 = 24.0;
pub const SCROLL_BUFFER: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Single,
    Continuous,
}

/// Vertical offset of the top of page `page0` (0-based) within the column.
pub fn page_top_css(page0: usize, heights: &[f64], gap: f64) -> f64 {
    heights.iter().take(page0).sum::<f64>() + gap * page0 as f64
}

/// Total scrollable height of the whole column (pages + gaps).
pub fn total_height_css(heights: &[f64], gap: f64) -> f64 {
    if heights.is_empty() {
        0.0
    } else {
        heights.iter().sum::<f64>() + gap * (heights.len().saturating_sub(1)) as f64
    }
}

/// 1-based page index whose vertical span contains `scroll_top` — i.e. the page
/// at the top of the scrollport. Returns the last page for scroll positions past
/// the end of the document, and 1 when no page heights are known yet (so callers
/// can no-op instead of jumping to a wrong page).
pub fn page_from_scroll(scroll_top: f64, heights: &[f64], gap: f64) -> u32 {
    if heights.is_empty() {
        return 1;
    }
    let mut acc = 0.0;
    for (i, &h) in heights.iter().enumerate() {
        if scroll_top < acc + h {
            return (i + 1) as u32;
        }
        acc += h + gap;
    }
    heights.len() as u32
}

/// 0-based inclusive range of ROWS visible in a scrollport
/// `[scroll_top, scroll_top + viewport_h]`, expanded by `buffer` rows on each
/// side. `rows` is the total row count; `row_height` the fixed CSS-px height of
/// each row (all rows equal). Returns `None` when there are no rows or the
/// viewport overlaps nothing.
///
/// Row `i` spans `[i * row_height, (i + 1) * row_height)`. The top boundary is
/// strict (a row that ended exactly at `scroll_top` is fully scrolled out); the
/// bottom boundary is inclusive (a row starting exactly at the viewport bottom
/// edge counts as visible) — mirroring `visible_range`.
pub fn visible_grid_rows(
    scroll_top: f64,
    viewport_h: f64,
    rows: usize,
    row_height: f64,
    buffer: usize,
) -> Option<(usize, usize)> {
    if rows == 0 || row_height <= 0.0 {
        return None;
    }
    let bottom = scroll_top + viewport_h.max(0.0);
    let grid_bottom = rows as f64 * row_height;
    // The viewport overlaps nothing: fully above or fully below the grid.
    if bottom < 0.0 || scroll_top >= grid_bottom {
        return None;
    }
    let mut first = (scroll_top / row_height).floor().max(0.0) as usize;
    let mut last = (bottom / row_height).floor().max(0.0) as usize;
    // Float safety: nudge the bottom boundary up a hair so a viewport ending
    // just short of a row boundary (fp error on `bottom / row_height`) still
    // renders that row instead of leaving a blank strip.
    if bottom > scroll_top {
        last = ((bottom / row_height) + 1e-9).floor().max(0.0) as usize;
    }
    first = first.min(rows - 1);
    last = last.min(rows - 1);
    let first = first.saturating_sub(buffer);
    let last = (last + buffer).min(rows - 1);
    Some((first, last))
}

/// 0-based inclusive range of page indices visible in the scrollport
/// `[scroll_top, scroll_top + viewport_h]`, expanded by `buffer` pages on each
/// side (so renders can start slightly before they scroll into view).
/// Returns `None` when there are no pages or nothing overlaps.
pub fn visible_range(
    scroll_top: f64,
    viewport_h: f64,
    heights: &[f64],
    gap: f64,
    buffer: usize,
) -> Option<(usize, usize)> {
    if heights.is_empty() {
        return None;
    }
    let bottom = scroll_top + viewport_h.max(0.0);
    let mut acc = 0.0;
    let mut first = heights.len();
    let mut last = 0usize;

    for (i, &h) in heights.iter().enumerate() {
        let top = acc;
        let page_bottom = top + h;
        // Strict on the top edge: a page that ended exactly at scroll_top is
        // fully scrolled out and not visible (zero-overlap boundary).
        if page_bottom > scroll_top && top <= bottom {
            first = first.min(i);
            last = last.max(i);
        }
        acc = top + h + gap;
    }

    if last < first {
        return None;
    }
    let first = first.saturating_sub(buffer);
    let last = (last + buffer).min(heights.len() - 1);
    Some((first, last))
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
        assert_eq!(total_height_css(&H, 24.0), 448.0);
        assert_eq!(total_height_css(&[], 24.0), 0.0);
    }

    #[test]
    fn visible_range_basic() {
        // viewport 0..100 sees page 0; with buffer 0 -> (0,0)
        assert_eq!(visible_range(0.0, 100.0, &H, 24.0, 0), Some((0, 0)));
        // viewport 124..324 sees page 1
        assert_eq!(visible_range(124.0, 200.0, &H, 24.0, 0), Some((1, 1)));
        // viewport 100..400 sees pages 1-2
        assert_eq!(visible_range(100.0, 300.0, &H, 24.0, 0), Some((1, 2)));
    }

    #[test]
    fn visible_range_buffer_expands_and_clamps() {
        // page 1 visible, buffer 1 -> (0, 2)
        assert_eq!(visible_range(124.0, 200.0, &H, 24.0, 1), Some((0, 2)));
        // page 2 visible, buffer 2 -> (0, 2) clamped to last
        assert_eq!(visible_range(348.0, 100.0, &H, 24.0, 2), Some((0, 2)));
    }

    #[test]
    fn visible_range_empty() {
        assert_eq!(visible_range(0.0, 500.0, &[], 24.0, 0), None);
    }

    #[test]
    fn visible_range_past_end() {
        assert_eq!(visible_range(9999.0, 100.0, &H, 24.0, 0), None);
    }

    #[test]
    fn visible_range_gap_handling() {
        // top at page1 = 124 (100 + 24 gap). A viewport 100..115 sits in the gap.
        assert_eq!(visible_range(100.0, 15.0, &H, 24.0, 0), None);
    }

    #[test]
    fn page_from_scroll_bounds() {
        assert_eq!(page_from_scroll(0.0, &H, 24.0), 1);
        assert_eq!(page_from_scroll(99.0, &H, 24.0), 1);
        // Bottom edge of page 0: page 1 is now at the top of the scrollport.
        assert_eq!(page_from_scroll(100.0, &H, 24.0), 2);
        assert_eq!(page_from_scroll(124.0, &H, 24.0), 2);
        assert_eq!(page_from_scroll(323.0, &H, 24.0), 2);
        assert_eq!(page_from_scroll(324.0, &H, 24.0), 3);
        // Past the end -> last page, not out of range.
        assert_eq!(page_from_scroll(9999.0, &H, 24.0), 3);
        // No measured heights yet -> 1 (caller no-ops instead of jumping).
        assert_eq!(page_from_scroll(500.0, &[], 24.0), 1);
    }

    #[test]
    fn visible_grid_rows_basic() {
        // 3 rows of 120px. viewport 0..100 sees row 0.
        assert_eq!(visible_grid_rows(0.0, 100.0, 3, 120.0, 0), Some((0, 0)));
        // viewport 120..320 overlaps rows 1 and 2.
        assert_eq!(visible_grid_rows(120.0, 200.0, 3, 120.0, 0), Some((1, 2)));
        // viewport 0..240 sees rows 0..2 (row 2 starts exactly at the bottom edge).
        assert_eq!(visible_grid_rows(0.0, 240.0, 3, 120.0, 0), Some((0, 2)));
    }

    #[test]
    fn visible_grid_rows_buffer_expands_and_clamps() {
        // row 1 visible, buffer 1 -> (0, 2)
        assert_eq!(visible_grid_rows(120.0, 200.0, 3, 120.0, 1), Some((0, 2)));
        // row 2 visible, buffer 2 -> (0, 2) clamped to last
        assert_eq!(visible_grid_rows(240.0, 100.0, 3, 120.0, 2), Some((0, 2)));
    }

    #[test]
    fn visible_grid_rows_empty() {
        assert_eq!(visible_grid_rows(0.0, 500.0, 0, 120.0, 0), None);
    }

    #[test]
    fn visible_grid_rows_no_overlap() {
        // Past the end of the grid -> nothing overlaps.
        assert_eq!(visible_grid_rows(9999.0, 100.0, 3, 120.0, 0), None);
        // Fully above the grid -> nothing overlaps.
        assert_eq!(visible_grid_rows(-100.0, 50.0, 3, 120.0, 0), None);
    }

    #[test]
    fn visible_grid_rows_single_row() {
        assert_eq!(visible_grid_rows(0.0, 100.0, 1, 120.0, 0), Some((0, 0)));
        // Buffer clamps to the single row.
        assert_eq!(visible_grid_rows(50.0, 200.0, 1, 120.0, 1), Some((0, 0)));
        // Past the end of the single row -> None.
        assert_eq!(visible_grid_rows(120.0, 100.0, 1, 120.0, 0), None);
    }

    #[test]
    fn visible_grid_rows_exact_top_boundary() {
        // Row 0 ends exactly at scroll_top=120 -> strictly scrolled out.
        assert_eq!(visible_grid_rows(120.0, 100.0, 3, 120.0, 0), Some((1, 1)));
        // Row 1 ends exactly at scroll_top=240 -> row 2 is the first visible.
        assert_eq!(visible_grid_rows(240.0, 100.0, 3, 120.0, 0), Some((2, 2)));
    }

    #[test]
    fn visible_grid_rows_empty_viewport() {
        // Zero-height viewport still yields the row containing scroll_top.
        assert_eq!(visible_grid_rows(50.0, 0.0, 3, 120.0, 0), Some((0, 0)));
        assert_eq!(visible_grid_rows(200.0, 0.0, 3, 120.0, 0), Some((1, 1)));
        // ... and None when there are no rows.
        assert_eq!(visible_grid_rows(0.0, 0.0, 0, 120.0, 0), None);
    }
}
