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

    /// `visible_range` over the fixture column [100, 200, 100] with gap 24
    /// (page tops 0 / 124 / 348). Covers the plain window, the buffer's
    /// expand-and-clamp, both empty cases and the strict top edge: a viewport
    /// lying entirely inside a gap sees nothing.
    #[test]
    fn visible_range_windows() {
        // (scroll_top, viewport_h, buffer, expected)
        let cases: &[(f64, f64, usize, Option<(usize, usize)>)] = &[
            (0.0, 100.0, 0, Some((0, 0))),
            (124.0, 200.0, 0, Some((1, 1))),
            (100.0, 300.0, 0, Some((1, 2))),
            (124.0, 200.0, 1, Some((0, 2))),
            (348.0, 100.0, 2, Some((0, 2))),
            // A viewport 100..115 sits wholly inside the gap below page 0.
            (100.0, 15.0, 0, None),
            (9999.0, 100.0, 0, None),
        ];
        for &(st, vh, buf, want) in cases {
            assert_eq!(visible_range(st, vh, &H, 24.0, buf), want, "st={st} vh={vh} buf={buf}");
        }
        assert_eq!(visible_range(0.0, 500.0, &[], 24.0, 0), None);
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

    /// `visible_grid_rows` over a 3-row grid of 120px rows (the thumbnail
    /// panel's virtualization). Covers the plain window, buffer expand/clamp,
    /// both no-overlap directions, exact row boundaries (a row ending exactly
    /// at scroll_top is scrolled out) and a zero-height viewport, which still
    /// yields the row containing scroll_top.
    #[test]
    fn visible_grid_rows_windows() {
        // (scroll_top, viewport_h, rows, buffer, expected)
        let cases: &[(f64, f64, usize, usize, Option<(usize, usize)>)] = &[
            (0.0, 100.0, 3, 0, Some((0, 0))),
            (120.0, 200.0, 3, 0, Some((1, 2))),
            (0.0, 240.0, 3, 0, Some((0, 2))),
            (120.0, 200.0, 3, 1, Some((0, 2))),
            (240.0, 100.0, 3, 2, Some((0, 2))),
            // Exact boundaries: the row above has strictly scrolled out.
            (120.0, 100.0, 3, 0, Some((1, 1))),
            (240.0, 100.0, 3, 0, Some((2, 2))),
            // Zero-height viewport still resolves to the row under scroll_top.
            (50.0, 0.0, 3, 0, Some((0, 0))),
            (200.0, 0.0, 3, 0, Some((1, 1))),
            // No overlap: past the end, or entirely above the grid.
            (9999.0, 100.0, 3, 0, None),
            (-100.0, 50.0, 3, 0, None),
            // Single row, including its buffer clamp and its past-the-end case.
            (0.0, 100.0, 1, 0, Some((0, 0))),
            (50.0, 200.0, 1, 1, Some((0, 0))),
            (120.0, 100.0, 1, 0, None),
            // No rows at all.
            (0.0, 500.0, 0, 0, None),
            (0.0, 0.0, 0, 0, None),
        ];
        for &(st, vh, rows, buf, want) in cases {
            assert_eq!(
                visible_grid_rows(st, vh, rows, 120.0, buf),
                want,
                "st={st} vh={vh} rows={rows} buf={buf}"
            );
        }
    }
}

#[cfg(test)]
mod anchor_tests {
    use super::*;

    /// The spec's hand-computed case, exercising the centre-anchor arithmetic:
    /// H=[100,200], gap=24, vh=100, f=2, anchor at the viewport centre.
    ///
    /// Posed with st=1 rather than the spec's st=0 so it measures the ANCHOR
    /// rather than the top-pin shortcut (at st=0 the answer is 0 by
    /// definition). doc_y = 1 + 50 = 51, i.e. 51% down page 0; after doubling
    /// that point sits at 102, and keeping it at screen y=50 means 52.
    #[test]
    fn spec_hand_case() {
        let h = [100.0, 200.0];
        let got = anchored_scroll(1.0, 100.0, &h, 24.0, 2.0, 50.0).unwrap();
        assert!((got - 52.0).abs() < 1e-9, "expected 52.0, got {got}");
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

    /// Both clamps: zooming out near the end lands exactly on the new
    /// max_scroll, and a document shorter than the viewport lands on 0 rather
    /// than going negative.
    #[test]
    fn clamps_to_the_new_scrollable_extent() {
        let h = [1000.0, 1000.0];
        let bottom = 1000.0 + 1000.0 + 24.0 - 100.0;
        let got = anchored_scroll(bottom, 100.0, &h, 24.0, 0.5, 50.0).unwrap();
        let new_max = 500.0 + 500.0 + 24.0 - 100.0;
        assert!((got - new_max).abs() < 1e-9, "expected {new_max}, got {got}");
        // Shorter than the viewport: clamps to 0. Posed away from the top so
        // the top-pin shortcut isn't what's being measured.
        assert_eq!(anchored_scroll(600.0, 500.0, &[1000.0], 24.0, 0.1, 250.0), Some(0.0));
    }

    /// Nothing measured yet, or a degenerate factor => nothing to anchor to.
    #[test]
    fn empty_heights_is_none() {
        assert!(anchored_scroll(0.0, 100.0, &[], 24.0, 2.0, 50.0).is_none());
        for f in [0.0, -1.0, f64::NAN] {
            assert!(anchored_scroll(0.0, 100.0, &[100.0], 24.0, f, 50.0).is_none(), "factor {f}");
        }
    }

    /// An anchor that lands inside a GAP resolves to the page above it, and the
    /// gap does NOT scale — so the anchor stays put rather than drifting by the
    /// gap's growth. This one test kills three separate mutations of the
    /// page-location loop (gap excluded from the span, the overshoot dropped,
    /// and the fraction clamp), so keep it exact.
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
        // ...and the mapping stays monotonic across the page/gap boundary.
        let a = anchored_scroll(49.0, 100.0, &h, 24.0, 2.0, 50.0).unwrap();
        let b = anchored_scroll(75.0, 100.0, &h, 24.0, 2.0, 50.0).unwrap();
        assert!(b > a, "monotonic across the page/gap boundary");
    }

    /// The page under the anchor is still the page under the anchor afterwards:
    /// the property that actually kills the "teleports to another page" bug.
    #[test]
    fn anchored_page_is_preserved() {
        let h = vec![800.0; 6];
        let (gap, vh) = (24.0, 900.0);
        for &st in &[0.0, 500.0, 1650.0, 3300.0, 4000.0] {
            for &f in &[1.25, 1.5, 2.0, 0.8, 0.5] {
                let before = page_from_scroll(st + vh * 0.5, &h, gap);
                let scaled: Vec<f64> = h.iter().map(|x| x * f).collect();
                let after_st = anchored_scroll(st, vh, &h, gap, f, vh * 0.5).unwrap();
                let after = page_from_scroll(after_st + vh * 0.5, &scaled, gap);
                // The CLAMPED extremes may differ: there is physically no
                // content left to scroll to. Everywhere else the page holds.
                let total_new: f64 = scaled.iter().sum::<f64>() + gap * 5.0;
                let clamped = after_st <= 1e-6 || after_st >= (total_new - vh).max(0.0) - 1e-6;
                assert!(before == after || clamped, "st={st} f={f}: page {before} -> {after}");
            }
        }
    }
}

#[cfg(test)]
mod dominant_page_tests {
    use super::*;

    /// The basic contract: the page covering most of the viewport wins, a tall
    /// page filling it wins outright, and degenerate inputs fall back to
    /// `page_from_scroll` instead of guessing.
    #[test]
    fn most_visible_page_wins() {
        // Straddling the boundary at 1000 with an 800-tall viewport.
        let h = [1000.0, 1000.0];
        // scroll 700 -> page 1 covers 300, page 2 covers 476 (after the gap).
        assert_eq!(dominant_page(700.0, 800.0, &h, 24.0), 2);
        // scroll 400 -> page 1 covers 600, page 2 covers 176.
        assert_eq!(dominant_page(400.0, 800.0, &h, 24.0), 1);
        // One tall page fills the viewport outright (page 2 spans 2024..4024).
        assert_eq!(dominant_page(2500.0, 800.0, &[2000.0; 3], 24.0), 2);
        // Nothing measured -> 1. No viewport height yet -> top-edge answer.
        assert_eq!(dominant_page(0.0, 800.0, &[], 24.0), 1);
        assert_eq!(dominant_page(900.0, 0.0, &[800.0, 800.0], 24.0), 2);
    }

    /// A jump that aligns page P's top with the viewport top reports P, even
    /// when several shorter pages are visible below it.
    #[test]
    fn jump_to_page_top_reports_that_page() {
        let h = vec![400.0; 10];
        for target in 1..=8u32 {
            let top = page_top_css(target as usize - 1, &h, 24.0);
            assert_eq!(dominant_page(top, 800.0, &h, 24.0), target, "target {target}");
        }
    }

    /// Zooming out must not walk the counter: the reader holds still (the
    /// anchor is preserved by `anchored_scroll`), so the reported page must not
    /// change as everything shrinks.
    #[test]
    fn zoom_out_does_not_walk_the_counter() {
        let base = vec![800.0; 20];
        let (gap, vh) = (24.0, 752.0);
        // Reading page 11: park the viewport centre in the middle of it.
        let idx = 10usize;
        let mut st = page_top_css(idx, &base, gap) + base[idx] / 2.0 - vh / 2.0;
        let start = dominant_page(st, vh, &base, gap);
        assert_eq!(start, 11);
        let mut heights = base;
        for f in [0.93, 0.857, 0.833, 0.8] {
            st = anchored_scroll(st, vh, &heights, gap, f, vh * 0.5).unwrap();
            heights = heights.iter().map(|x| x * f).collect();
            assert_eq!(
                dominant_page(st, vh, &heights, gap),
                start,
                "counter drifted while zooming out"
            );
        }
    }

    /// Regression: a full zoom round-trip must land
    /// on the page it started on, in a document whose pages are NOT all the
    /// same height. This is the arithmetic half of the fix; the other half was
    /// a scroll write clamped by a not-yet-grown spacer, which lives in the DOM.
    #[test]
    fn zoom_round_trip_keeps_the_same_page_in_a_mixed_size_document() {
        // 300 pages: mostly letter, with legal / A4 / landscape mixed in — the
        // shape of a real book with plates and inserts.
        let intrinsic: Vec<f64> = (0..300)
            .map(|i| match i {
                _ if i % 37 == 0 => 612.0,
                _ if i % 13 == 0 => 842.0,
                _ if i % 7 == 0 => 1008.0,
                _ => 792.0,
            })
            .collect();
        let vh = 800.0;
        let anchor = vh * 0.5;
        let at = |scale: f64| -> Vec<f64> { intrinsic.iter().map(|h| h * scale).collect() };

        let mut scale = 1.0_f64;
        let mut heights = at(scale);
        // Park the viewport centre inside page 256.
        let mut scroll = page_top_css(255, &heights, PAGE_GAP) + heights[255] * 0.5 - anchor;
        let start_page = dominant_page(scroll, vh, &heights, PAGE_GAP);
        assert_eq!(start_page, 256, "test setup should start on page 256");

        // Out to 25%, back in to 175%, then home — the user's gesture.
        for target in [0.5_f64, 0.25, 0.5, 1.0, 1.75, 1.0] {
            let factor = target / scale;
            scroll = anchored_scroll(scroll, vh, &heights, PAGE_GAP, factor, anchor)
                .expect("anchored_scroll should produce a position");
            scale = target;
            heights = at(scale);
        }

        assert_eq!(
            dominant_page(scroll, vh, &heights, PAGE_GAP),
            start_page,
            "a zoom round-trip must not move the reader off their page"
        );
    }

    /// The height column must be derived from each page's OWN size. Seeding
    /// every page from page 1 mislocates every page below the first odd one.
    #[test]
    fn page_offsets_follow_each_pages_own_height() {
        let mixed = [792.0, 1008.0, 792.0, 842.0];
        assert_ne!(
            page_top_css(3, &mixed, PAGE_GAP),
            page_top_css(3, &[792.0; 4], PAGE_GAP),
            "uniform seeding hides real offsets in a mixed-size document"
        );
        assert_eq!(page_top_css(3, &mixed, PAGE_GAP), 792.0 + 1008.0 + 792.0 + 3.0 * PAGE_GAP);
    }
}
