//! Pure layout math shared by both view modes (single page + continuous).
//! No wasm deps — unit-testable on the host.
//!
//! The continuous layout is a vertical column of pages separated by `gap` px.
//! `heights` holds each page's rendered CSS-px height (0-based index), filled
//! lazily as pages report their geometry.

pub use virtual_list::{Budget, Strip};

pub const PAGE_GAP: f64 = 24.0;

/// Height of the glass toolbar, in CSS px.
///
/// The viewer scrollport spans the FULL window height so pages travel under the
/// translucent header (that is what the backdrop-filter refracts). The visible
/// reading area is therefore the container minus this inset, and each view
/// offsets its own content by the same amount — `mt-12` on the continuous
/// column, `pt-18` (12 + the page's own 6) on the single-page sheet.
///
/// Keep this in sync with `h-12` on `#toolbar-row`: 12 * 0.25rem = 3rem = 48px.
pub const TOOLBAR_H: f64 = 48.0;

/// How many pages may be mounted at once, and how far ahead to look.
///
/// WHY THIS EXISTS. The read-ahead used to be a fixed count of PAGES
/// (`SCROLL_BUFFER = 3`), which silently means two completely different
/// things depending on zoom. Zoomed out, a page is a
/// few hundred px tall and three of them is a modest read-ahead. Zoomed in,
/// one page is several screens tall, so buffering three of them mounts six
/// off-screen pages that the reader cannot reach for many seconds of
/// scrolling — and at high zoom each one costs a full `PAGE_MAX_PIXELS`
/// raster (8MP => ~32MB), so the window alone can hold ~224MB of canvas.
///
/// The read-ahead is therefore expressed in SCREENFULS, not pages. One
/// screenful ahead is one screenful ahead at any zoom: at 100% it still pulls
/// in the neighbours, and at 500% it mounts the next page only once the
/// reader is genuinely near it.
///
/// TO TUNE THIS, change these two numbers and nothing else.
/// How many pages may be mounted at once, and how far ahead to look.
///
/// This is [`virtual_list::Budget`] under the project's own name. The rationale
/// for measuring read-ahead in SCREENFULS rather than pages lives on that type;
/// the short version is that a fixed page count means a modest read-ahead when
/// zoomed out and several screens of wasted rasters when zoomed in.
///
/// TO TUNE THIS, change the two numbers in `RenderBudget::default` and nothing
/// else.
pub type RenderBudget = Budget;

/// The project default: one screenful of read-ahead each way, at most 7 pages.
///
/// Zoomed out, many short pages are legitimately on screen at once and 7 is the
/// ceiling for that case; zoomed in it is never the binding constraint because
/// `look_frac` decides first.
pub const RENDER_BUDGET: RenderBudget = RenderBudget {
    look_frac: 1.0,
    max_items: 7,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Single,
    Continuous,
}

/// Vertical offset of the top of page `page0` (0-based) within the column.
pub fn page_top_css(page0: usize, heights: &[f64], gap: f64) -> f64 {
    Strip::new(heights.iter().copied(), gap).offset(page0)
}

/// Total scrollable height of the whole column (pages + gaps).
pub fn total_height_css(heights: &[f64], gap: f64) -> f64 {
    Strip::new(heights.iter().copied(), gap).total()
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
    Strip::new(heights.iter().copied(), gap).dominant(scroll_top, viewport_h) as u32 + 1
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

/// 0-based inclusive range of pages to KEEP MOUNTED.
///
/// This is `visible_range`'s job done in viewport units instead of page
/// counts. The window is everything overlapping
/// `[scroll_top - look, scroll_top + viewport_h + look]`, where
/// `look = budget.look_frac * viewport_h`, then trimmed to
/// `budget.max_items`.
///
/// Two invariants make it safe to tune the budget to anything at all:
///   * every page that is even partly VISIBLE is always in the result, so no
///     setting can blank out something the reader is looking at;
///   * trimming drops the page furthest from the viewport first, and prefers
///     to keep the page BELOW (reading direction), so the next page the
///     reader will reach is the last thing evicted.
///
/// The zoom-dependent behaviour the reader actually feels falls out of the
/// arithmetic rather than being special-cased: when a page is taller than the
/// look-ahead distance, the next page simply does not overlap the window
/// until the reader has scrolled to within one screenful of it.
pub fn render_range(
    scroll_top: f64,
    viewport_h: f64,
    heights: &[f64],
    gap: f64,
    budget: RenderBudget,
) -> Option<(usize, usize)> {
    Strip::new(heights.iter().copied(), gap)
        .window(scroll_top, viewport_h, budget)
        .map(|w| (w.first, w.last))
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

    /// `span_overlapping` over the fixture column [100, 200, 100] with gap 24
    /// (page tops 0 / 124 / 348). Covers the plain window, exact boundaries
    /// (a page ending exactly at the top edge has scrolled out), a viewport
    /// parked wholly inside a gap, and past-the-end.
        /// The buffering `visible_range` used to provide is now `render_range`'s
    /// look-ahead. A whole-viewport look on this tiny column reaches the
    /// neighbours, and the ceiling clamps to the document.
    #[test]
    fn render_range_expands_and_clamps() {
        let b = RenderBudget { look_frac: 1.0, max_items: 7 };
        assert_eq!(render_range(124.0, 200.0, &H, 24.0, b), Some((0, 2)));
        // No look-ahead at all == exactly what is on screen.
        let none = RenderBudget { look_frac: 0.0, max_items: 7 };
        assert_eq!(render_range(124.0, 200.0, &H, 24.0, none), Some((1, 1)));
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

    /// Test-local shorthands for the two `Strip` queries these tests need.
    /// Production code goes through `Strip` directly.
    fn page_from_scroll(scroll_top: f64, heights: &[f64], gap: f64) -> u32 {
        if heights.is_empty() {
            return 1;
        }
        Strip::new(heights.iter().copied(), gap).index_at(scroll_top) as u32 + 1
    }
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

#[cfg(test)]
mod render_range_tests {
    use super::*;

    fn span_overlapping(
        top: f64,
        height: f64,
        heights: &[f64],
        gap: f64,
    ) -> Option<(usize, usize)> {
        Strip::new(heights.iter().copied(), gap)
            .overlapping(top, height)
            .map(|w| (w.first, w.last))
    }


    const GAP: f64 = PAGE_GAP;

    fn uniform(n: usize, h: f64) -> Vec<f64> {
        vec![h; n]
    }

    /// Park the viewport `frac` of the way down page `idx` (0-based).
    fn scroll_into(idx: usize, frac: f64, heights: &[f64], vh: f64) -> f64 {
        page_top_css(idx, heights, GAP) + heights[idx] * frac - vh * 0.5
    }

    /// THE POINT OF THE WHOLE CHANGE: mounted page count must fall as the
    /// pages grow past the viewport, instead of staying pinned at 2*buffer+1.
    #[test]
    fn zoomed_in_mounts_fewer_pages_than_zoomed_out() {
        let vh = 800.0;
        let b = RenderBudget::default();
        let count_at = |page_h: f64| {
            let h = uniform(40, page_h);
            let st = scroll_into(10, 0.5, &h, vh);
            let (f, l) = render_range(st, vh, &h, GAP, b).unwrap();
            l - f + 1
        };
        let zoomed_out = count_at(396.0); // 50%
        let normal = count_at(792.0); // 100%
        let zoomed = count_at(2376.0); // 300%
        let max_zoom = count_at(3960.0); // 500%

        assert!(
            zoomed_out >= normal && normal > zoomed && zoomed >= max_zoom,
            "expected monotonic shrink, got {zoomed_out} / {normal} / {zoomed} / {max_zoom}"
        );
        // Mid-page at 300%+ the neighbours are more than a screenful away.
        assert_eq!(max_zoom, 1, "at max zoom only the page under the eyes");
        // The old constant-buffer behaviour would have been 7 in every case.
        assert!(max_zoom < 7 && zoomed < 7);
    }

    /// The reader nearing the bottom of a tall page pulls the next one in
    /// before they reach it — the "render the one below at ~80%" behaviour,
    /// expressed as a distance rather than a hardcoded percentage.
    #[test]
    fn next_page_mounts_before_the_reader_arrives() {
        let vh = 800.0;
        let h = uniform(40, 3960.0); // 500%
        let b = RenderBudget::default();
        let idx = 10;

        // Mid-page: neighbours are far away, so just this page.
        let mid = scroll_into(idx, 0.5, &h, vh);
        assert_eq!(render_range(mid, vh, &h, GAP, b), Some((idx, idx)));

        // Scrolled so the page bottom is within one screenful: next is mounted
        // and ready, BEFORE any of it is on screen.
        let page_bottom = page_top_css(idx, &h, GAP) + h[idx];
        let near = page_bottom - vh - 10.0; // bottom is 10px beyond the viewport
        let (f, l) = render_range(near, vh, &h, GAP, b).unwrap();
        assert!(l >= idx + 1, "next page should be mounted early, got {f}..={l}");
        // ...and it is genuinely not visible yet, i.e. this is real read-ahead.
        let (vf, vl) = span_overlapping(near, vh, &h, GAP).unwrap();
        assert_eq!((vf, vl), (idx, idx), "next page must not be on screen yet");
    }

    /// Scrolling down evicts the page above once it is a screenful behind.
    #[test]
    fn page_above_is_dropped_once_far_enough_behind() {
        let vh = 800.0;
        let h = uniform(40, 3960.0);
        let b = RenderBudget::default();
        let idx = 10;
        let top = page_top_css(idx, &h, GAP);
        // Just past the top of page 10: page 9 is still within a screenful.
        let just_in = top + 100.0;
        let (f, _) = render_range(just_in, vh, &h, GAP, b).unwrap();
        assert_eq!(f, idx - 1, "the page just behind should still be warm");
        // Well into page 10: page 9 is now more than a screenful behind.
        let deep = top + vh * 1.5;
        let (f2, _) = render_range(deep, vh, &h, GAP, b).unwrap();
        assert_eq!(f2, idx, "the page behind should have been evicted");
    }

    /// INVARIANT: every visible page is always mounted, at any zoom, any
    /// budget — including budgets too small to hold them all.
    #[test]
    fn visible_pages_are_never_evicted() {
        let cases = [(396.0, 800.0), (792.0, 800.0), (3960.0, 420.0), (200.0, 1200.0)];
        for (page_h, vh) in cases {
            let h = uniform(30, page_h);
            for budget in [
                RenderBudget::default(),
                RenderBudget { look_frac: 0.0, max_items: 1 },
                RenderBudget { look_frac: 3.0, max_items: 2 },
            ] {
                for step in 0..40 {
                    let st = step as f64 * page_h * 0.37;
                    let Some((f, l)) = render_range(st, vh, &h, GAP, budget) else {
                        continue;
                    };
                    if let Some((vf, vl)) = span_overlapping(st, vh, &h, GAP) {
                        assert!(
                            f <= vf && l >= vl,
                            "page_h={page_h} vh={vh} st={st} budget={budget:?}: \
                             mounted {f}..={l} does not cover visible {vf}..={vl}"
                        );
                    }
                }
            }
        }
    }

    /// The ceiling is respected whenever it does not fight the invariant above.
    #[test]
    fn never_exceeds_the_ceiling_unless_visibility_demands_it() {
        let vh = 800.0;
        let h = uniform(60, 120.0); // many tiny pages: lots are visible at once
        for max_items in [1usize, 3, 5, 7, 12] {
            let b = RenderBudget { look_frac: 2.0, max_items };
            let st = 1000.0;
            let (f, l) = render_range(st, vh, &h, GAP, b).unwrap();
            let n = l - f + 1;
            let visible_n = span_overlapping(st, vh, &h, GAP)
                .map(|(a, b)| b - a + 1)
                .unwrap_or(0);
            assert!(
                n <= max_items.max(visible_n),
                "max_items={max_items}: mounted {n} pages (visible {visible_n})"
            );
        }
    }

    /// Degenerate inputs resolve to something sane rather than panicking.
    #[test]
    fn degenerate_inputs() {
        let b = RenderBudget::default();
        assert_eq!(render_range(0.0, 800.0, &[], GAP, b), None);
        // Zero-height viewport: still mounts the page under the scroll point.
        let h = uniform(5, 500.0);
        assert!(render_range(1100.0, 0.0, &h, GAP, b).is_some());
        // Past the end of the document: `None`, exactly as `visible_range`
        // answers, so the caller's existing "nothing to render" path is
        // reached rather than a surprise last-page mount.
        assert_eq!(render_range(99_999.0, 800.0, &h, GAP, b), None);
        assert_eq!(span_overlapping(99_999.0, 800.0, &h, GAP), None);
        // A zero max_items is treated as 1 rather than producing an empty range.
        let bad = RenderBudget { look_frac: 0.0, max_items: 0 };
        let (f, l) = render_range(1100.0, 800.0, &h, GAP, bad).unwrap();
        assert!(l >= f);
    }

    /// Parked in the gap between two pages: nothing is strictly visible, and
    /// the window must still resolve to the neighbouring pages.
    #[test]
    fn gap_parking_still_mounts_neighbours() {
        let h = [100.0, 200.0, 100.0];
        // 100..124 is the gap after page 0; a 15px viewport sits inside it.
        let b = RenderBudget { look_frac: 1.0, max_items: 7 };
        let got = render_range(104.0, 15.0, &h, GAP, b);
        assert!(got.is_some(), "a gap position must still mount something");
        let (f, l) = got.unwrap();
        assert!(f == 0 && l >= 1, "expected the pages either side, got {f}..={l}");
    }
}
