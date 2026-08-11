//! Pure layout math shared by both view modes (single page + continuous).
//! No wasm deps — unit-testable on the host.
//!
//! The continuous layout is a vertical column of pages separated by `gap` px.
//! `heights` holds each page's rendered CSS-px height (0-based index), filled
//! lazily as pages report their geometry.

pub const PAGE_GAP: f64 = 24.0;
pub const SCROLL_BUFFER: usize = 3;

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

/// 1-based page the reader is actually looking at: the page occupying the most
/// of the viewport, NOT the one clipping the top edge. Ties go to the lower
/// page number.
///
/// `page_from_scroll` answers a different question — "which page's span
/// contains the top pixel of the scrollport" — and that is the wrong question
/// for a page counter. Zooming out shrinks every page, so more of the PREVIOUS
/// page slides down into the top of the viewport and the top-edge answer keeps
/// changing (1 -> 2 -> 3 ... and back on the way in) even though the reader
/// never moved and the content under their eyes is unchanged. That is the
/// "zoom shifts to unrelated pages" report: the view was correctly anchored all
/// along, the COUNTER was measuring the wrong thing.
///
/// Area-of-viewport is the right metric because it degrades gracefully at both
/// extremes. Zoomed in, one page fills the viewport and trivially wins. Zoomed
/// out, several short pages are visible at once and the one you see most of
/// wins — and after a jump that aligns page P's top with the viewport top, P
/// covers at least as much as any page below it, so a jump still reports the
/// page it jumped to.
///
/// Falls back to the top-edge answer when nothing is measured or the viewport
/// has no height yet.
pub fn dominant_page(scroll_top: f64, viewport_h: f64, heights: &[f64], gap: f64) -> u32 {
    if heights.is_empty() {
        return 1;
    }
    if viewport_h <= 1.0 {
        return page_from_scroll(scroll_top, heights, gap);
    }
    let view_top = scroll_top;
    let view_bottom = scroll_top + viewport_h;
    let mut acc = 0.0;
    let mut best = 1u32;
    let mut best_vis = -1.0;
    for (i, &h) in heights.iter().enumerate() {
        let top = acc;
        let bottom = acc + h;
        if top >= view_bottom {
            break;
        }
        let vis = bottom.min(view_bottom) - top.max(view_top);
        // Strictly greater keeps ties on the LOWER page: after a jump, page P
        // and the pages below it can be equally visible, and the answer the
        // reader expects is the one they jumped to.
        if vis > best_vis {
            best_vis = vis;
            best = (i + 1) as u32;
        }
        acc = bottom + gap;
    }
    if best_vis <= 0.0 {
        // Parked in a gap or past the end — fall back rather than guess.
        return page_from_scroll(scroll_top, heights, gap);
    }
    best
}

/// Scroll offset that keeps the document point currently at `anchor_screen_y`
/// (in viewport coordinates, measured from the top of the scrollport) pinned to
/// the same screen position after every page height is multiplied by `factor`.
///
/// This is the core of flicker-free zooming. Page heights are LINEAR in scale
/// (`h = base * scale`), so a scale change can be applied to the whole layout
/// exactly, in one synchronous step, without waiting for a single render to
/// resolve. Anchoring the scroll in that same step is what stops the viewport
/// silently landing on a different page: at 100% a scroll of 5000px might be
/// page 8, but at 147% every page is 1.47x taller, so the same 5000px is
/// page ~6. Rescaling without re-anchoring IS the "zoom teleports to another
/// page" bug.
///
/// Gaps between pages are chrome, not content: they are a fixed CSS-px value
/// and deliberately NOT scaled, mirroring how `total_height_css` /
/// `page_top_css` lay the column out. An anchor that lands IN a gap therefore
/// carries its offset into that gap over 1:1, which makes the mapping
/// continuous across page boundaries — a point drifting from the bottom of one
/// page to the top of the next never jumps.
///
/// Returns `None` when there is nothing to anchor (no measured heights, or a
/// nonsensical factor), so callers can no-op rather than scroll to a guess.
pub fn anchored_scroll(
    scroll_top: f64,
    viewport_h: f64,
    heights: &[f64],
    gap: f64,
    factor: f64,
    anchor_screen_y: f64,
) -> Option<f64> {
    if heights.is_empty() || !(factor > 0.0) || !factor.is_finite() {
        return None;
    }
    // Sitting at the very top: pin the top. Anchoring the CENTRE here would
    // push the start of the document up off the viewport — you zoom in on
    // page 1 and the top of page 1 disappears, which reads as the view jumping.
    // The bottom edge needs no equivalent case: the `max_scroll` clamp below
    // already keeps the end of the document pinned.
    if scroll_top <= 0.5 {
        return Some(0.0);
    }

    // The document-space y currently under the anchor point.
    let doc_y = scroll_top + anchor_screen_y;

    // Locate the page containing `doc_y`, plus how far down that page it sits.
    // `over` carries any excess past the page bottom (i.e. a position inside
    // the following gap) so it can be re-added unscaled.
    let last = heights.len() - 1;
    let mut acc = 0.0;
    let mut idx = last;
    let mut frac = 1.0;
    let mut over = 0.0;
    for (i, &h) in heights.iter().enumerate() {
        let bottom = acc + h;
        // Inside this page, inside the gap directly below it, or the last page
        // (which absorbs anything past the end of the document).
        if doc_y < bottom + gap || i == last {
            idx = i;
            frac = if h > 0.0 {
                ((doc_y - acc) / h).clamp(0.0, 1.0)
            } else {
                0.0
            };
            // Positive only when doc_y sits past this page's bottom, i.e. in
            // the gap below it (or past the end of the last page).
            over = (doc_y - bottom).max(0.0);
            break;
        }
        acc = bottom + gap;
    }

    // Where that same point lands once every page is `factor` taller.
    let new_page_top: f64 =
        heights.iter().take(idx).map(|h| h * factor).sum::<f64>() + gap * idx as f64;
    let new_doc_y = new_page_top + frac * heights[idx] * factor + over;

    let total_new: f64 =
        heights.iter().sum::<f64>() * factor + gap * last as f64;
    let max_scroll = (total_new - viewport_h).max(0.0);
    Some((new_doc_y - anchor_screen_y).clamp(0.0, max_scroll))
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

#[cfg(test)]
mod anchor_tests {
    use super::*;

    /// The spec's hand-computed case, exercising the centre-anchor arithmetic:
    /// H=[100,200], gap=24, vh=100, f=2, anchor at the viewport centre.
    ///
    /// Posed with st=1 rather than the spec's st=0 so it measures the ANCHOR
    /// rather than the top-pin shortcut above (at st=0 the answer is 0 by
    /// definition — see `top_of_document_stays_pinned`). doc_y = 1 + 50 = 51,
    /// i.e. 51% down page 0; after doubling that point sits at 102, and
    /// keeping it at screen y=50 means scrolling to 52.
    #[test]
    fn spec_hand_case() {
        let h = [100.0, 200.0];
        let got = anchored_scroll(1.0, 100.0, &h, 24.0, 2.0, 50.0).unwrap();
        assert!((got - 52.0).abs() < 1e-9, "expected 52.0, got {got}");
        // The spec's exact st=0 form, under the top-pin rule.
        assert_eq!(anchored_scroll(0.0, 100.0, &h, 24.0, 2.0, 50.0), Some(0.0));
    }

    /// At the top of the document, zooming keeps the top pinned rather than
    /// scrolling the start of page 1 out of view.
    #[test]
    fn top_of_document_stays_pinned() {
        let h = [1000.0, 1000.0];
        assert_eq!(anchored_scroll(0.0, 900.0, &h, 24.0, 2.0, 450.0), Some(0.0));
        assert_eq!(anchored_scroll(0.0, 900.0, &h, 24.0, 0.5, 450.0), Some(0.0));
        // Just below the threshold counts as "at the top" too.
        assert_eq!(anchored_scroll(0.4, 900.0, &h, 24.0, 2.0, 450.0), Some(0.0));
        // But a real offset anchors normally.
        assert!(anchored_scroll(600.0, 900.0, &h, 24.0, 2.0, 450.0).unwrap() > 0.0);
    }

    /// factor 1.0 must be an exact identity: no drift when a "zoom" is a no-op.
    #[test]
    fn factor_one_is_identity() {
        let h = [100.0, 200.0, 340.0, 90.0];
        for &st in &[17.5, 123.0, 400.0] {
            let got = anchored_scroll(st, 100.0, &h, 24.0, 1.0, 50.0).unwrap();
            assert!((got - st).abs() < 1e-9, "st={st} -> {got}");
        }
    }

    /// Zooming out near the end of the document can't leave the scroll past
    /// the new (shorter) content: it clamps to max_scroll, never negative.
    #[test]
    fn clamps_at_end_when_zooming_out() {
        let h = [1000.0, 1000.0];
        // total = 2024, vh = 100 -> parked at the very bottom.
        let bottom = 1000.0 + 1000.0 + 24.0 - 100.0;
        let got = anchored_scroll(bottom, 100.0, &h, 24.0, 0.5, 50.0).unwrap();
        let new_max = 500.0 + 500.0 + 24.0 - 100.0;
        assert!((got - new_max).abs() < 1e-9, "expected {new_max}, got {got}");
        assert!(got >= 0.0);
    }

    /// Zooming out a short document clamps to 0 rather than going negative.
    #[test]
    fn clamps_at_zero() {
        let h = [1000.0];
        // Zooming out a document shorter than the viewport lands at 0, not
        // negative. Posed away from the top so the pin isn't what's tested.
        let got = anchored_scroll(600.0, 500.0, &h, 24.0, 0.1, 250.0).unwrap();
        assert_eq!(got, 0.0);
    }

    /// Nothing measured yet => nothing to anchor to.
    #[test]
    fn empty_heights_is_none() {
        assert!(anchored_scroll(0.0, 100.0, &[], 24.0, 2.0, 50.0).is_none());
        // Degenerate factors are refused too.
        assert!(anchored_scroll(0.0, 100.0, &[100.0], 24.0, 0.0, 50.0).is_none());
        assert!(anchored_scroll(0.0, 100.0, &[100.0], 24.0, -1.0, 50.0).is_none());
        assert!(anchored_scroll(0.0, 100.0, &[100.0], 24.0, f64::NAN, 50.0).is_none());
    }

    /// An anchor that lands inside a GAP resolves to the page above it, and the
    /// gap does NOT scale — so the anchor stays put rather than drifting by the
    /// gap's growth.
    #[test]
    fn anchor_in_gap_uses_page_above_and_gap_is_unscaled() {
        let h = [100.0, 100.0];
        // doc_y = 110 -> 10px into the 24px gap after page 0 (100..124).
        let got = anchored_scroll(60.0, 100.0, &h, 24.0, 2.0, 50.0).unwrap();
        // Page 0 doubles to 200; the 10px gap offset carries over UNSCALED, so
        // the anchored point is at 210 and must stay at screen y=50.
        let total_new = 200.0 + 200.0 + 24.0;
        let expected = (210.0f64 - 50.0).clamp(0.0, total_new - 100.0);
        assert!((got - expected).abs() < 1e-9, "expected {expected}, got {got}");
        // ...and the mapping is continuous across the boundary: a point 1px
        // above the gap and 1px below it stay 1px-ish apart, never jumping.
        let a = anchored_scroll(49.0, 100.0, &h, 24.0, 2.0, 50.0).unwrap();
        let b = anchored_scroll(75.0, 100.0, &h, 24.0, 2.0, 50.0).unwrap();
        assert!(b > a, "monotonic across the page/gap boundary");
    }

    /// The page under the anchor is still the page under the anchor afterwards:
    /// the property that actually kills the "teleports to another page" bug.
    #[test]
    fn anchored_page_is_preserved() {
        let h = vec![800.0, 800.0, 800.0, 800.0, 800.0, 800.0];
        let gap = 24.0;
        let vh = 900.0;
        for &st in &[0.0, 500.0, 1650.0, 3300.0, 4000.0] {
            for &f in &[1.25, 1.5, 2.0, 0.8, 0.5] {
                let before = page_from_scroll(st + vh * 0.5, &h, gap);
                let scaled: Vec<f64> = h.iter().map(|x| x * f).collect();
                let after_st = anchored_scroll(st, vh, &h, gap, f, vh * 0.5).unwrap();
                let after = page_from_scroll(after_st + vh * 0.5, &scaled, gap);
                // Allow the CLAMPED cases to differ: at either extreme the
                // scroll physically cannot keep the anchor (there is no
                // content left to scroll to). Elsewhere the page must hold.
                let total_new: f64 = scaled.iter().sum::<f64>() + gap * 5.0;
                let clamped = after_st <= 1e-6
                    || after_st >= (total_new - vh).max(0.0) - 1e-6;
                assert!(
                    before == after || clamped,
                    "st={st} f={f}: page {before} -> {after}"
                );
            }
        }
    }
}

#[cfg(test)]
mod dominant_page_tests {
    use super::*;

    /// Zoomed in, one tall page fills the viewport and wins outright.
    #[test]
    fn tall_page_filling_viewport_wins() {
        let h = [2000.0, 2000.0, 2000.0];
        // Deep inside page 2 (spans 2024..4024).
        assert_eq!(dominant_page(2500.0, 800.0, &h, 24.0), 2);
    }

    /// THE REGRESSION THIS FIXES: zooming out must not walk the counter.
    /// The reader holds still (the anchor is preserved by `anchored_scroll`),
    /// so the reported page must not change as everything shrinks.
    #[test]
    fn zoom_out_does_not_walk_the_counter() {
        let base = vec![800.0; 20];
        let gap = 24.0;
        let vh = 752.0;
        // Reading page 11: park the viewport centre in the middle of it.
        let idx = 10usize;
        let st0 = page_top_css(idx, &base, gap) + base[idx] / 2.0 - vh / 2.0;
        let start = dominant_page(st0, vh, &base, gap);
        assert_eq!(start, 11);
        let mut st = st0;
        let mut heights = base.clone();
        for f in [0.93, 0.857, 0.833, 0.8] {
            let new_st = anchored_scroll(st, vh, &heights, gap, f, vh * 0.5).unwrap();
            heights = heights.iter().map(|x| x * f).collect();
            st = new_st;
            assert_eq!(
                dominant_page(st, vh, &heights, gap),
                start,
                "counter drifted while zooming out"
            );
        }
    }

    /// A jump that aligns page P's top with the viewport top reports P, even
    /// when several shorter pages are visible below it.
    #[test]
    fn jump_to_page_top_reports_that_page() {
        let h = vec![400.0; 10];
        let gap = 24.0;
        let vh = 800.0;
        for target in 1..=8u32 {
            let top = page_top_css(target as usize - 1, &h, gap);
            assert_eq!(dominant_page(top, vh, &h, gap), target, "target {target}");
        }
    }

    /// Degenerate inputs fall back instead of guessing.
    #[test]
    fn falls_back_when_unmeasurable() {
        assert_eq!(dominant_page(0.0, 800.0, &[], 24.0), 1);
        // No viewport height yet -> top-edge answer.
        let h = [800.0, 800.0];
        assert_eq!(dominant_page(900.0, 0.0, &h, 24.0), 2);
    }

    /// Half-and-half: the page covering more of the viewport wins.
    #[test]
    fn larger_share_wins() {
        let h = [1000.0, 1000.0];
        // Viewport 800 tall, straddling the boundary at 1000.
        // scroll 700 -> page 1 covers 300, page 2 covers 476 (after the gap).
        assert_eq!(dominant_page(700.0, 800.0, &h, 24.0), 2);
        // scroll 400 -> page 1 covers 600, page 2 covers 176.
        assert_eq!(dominant_page(400.0, 800.0, &h, 24.0), 1);
    }
}
