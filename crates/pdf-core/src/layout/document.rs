//! The cached column layout: a prebuilt [`Strip`] for the CURRENT page
//! heights, answering every scroll / zoom / search query through the same
//! prefix sums. Rebuild only when the heights change; the hot paths borrow.
//!
//! [`Strip`]: virtual_list::Strip

use super::RenderBudget;
use virtual_list::Strip;

#[derive(Debug, Clone, PartialEq)]
pub struct DocumentLayout {
    // `pub(crate)`: the anchoring math lives in the sibling `anchor` module
    // and reads these directly; the external API stays the methods below.
    pub(crate) strip: Strip,
    pub(crate) gap: f64,
}

impl DocumentLayout {
    pub fn new(heights: &[f64], gap: f64) -> Self {
        Self {
            strip: Strip::new(heights.iter().copied(), gap),
            gap,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.strip.is_empty()
    }

    /// Number of pages in the column.
    pub fn strip_len(&self) -> usize {
        self.strip.len()
    }

    /// Vertical offset of the top of page `page0` (0-based) within the column.
    pub fn page_top(&self, page0: usize) -> f64 {
        self.strip.offset(page0)
    }

    /// Height of page `i`, recovered from the prefix offsets (no per-call
    /// vector walk).
    pub fn height(&self, i: usize) -> f64 {
        if i + 1 < self.strip.len() {
            self.strip.offset(i + 1) - self.strip.offset(i) - self.gap
        } else {
            self.strip.total() - self.strip.offset(i)
        }
    }

    /// Total scrollable height of the whole column (pages + gaps).
    pub fn total(&self) -> f64 {
        self.strip.total()
    }



    /// 1-based page the reader is actually looking at (ties go to the lower
    /// page number). Falls back to 1 for an empty column.
    pub fn dominant(&self, scroll_top: f64, viewport_h: f64) -> u32 {
        self.strip.dominant(scroll_top, viewport_h) as u32 + 1
    }

    /// 0-based inclusive range of pages to keep mounted.
    pub fn window(
        &self,
        scroll_top: f64,
        viewport_h: f64,
        budget: RenderBudget,
    ) -> Option<(usize, usize)> {
        self.strip
            .window(scroll_top, viewport_h, budget)
            .map(|w| (w.first, w.last))
    }

    pub fn window_hinted(
        &self,
        scroll_top: f64,
        viewport_h: f64,
        budget: RenderBudget,
        hint: &mut usize,
    ) -> Option<(usize, usize)> {
        self.strip
            .window_hinted(scroll_top, viewport_h, budget, hint)
            .map(|w| (w.first, w.last))
    }
}


#[cfg(test)]
mod dominant_page_tests {
    use crate::layout::{anchored_scroll, page_top_css, DocumentLayout, PAGE_GAP};

    fn dominant(scroll_top: f64, viewport_h: f64, heights: &[f64], gap: f64) -> u32 {
        DocumentLayout::new(heights, gap).dominant(scroll_top, viewport_h)
    }

    /// The basic contract: the page covering most of the viewport wins, a tall
    /// page filling it wins outright, and degenerate inputs fall back to
    /// `page_from_scroll` instead of guessing.
    #[test]
    fn most_visible_page_wins() {
        // Straddling the boundary at 1000 with an 800-tall viewport.
        let h = [1000.0, 1000.0];
        // scroll 700 -> page 1 covers 300, page 2 covers 476 (after the gap).
        assert_eq!(dominant(700.0, 800.0, &h, 24.0), 2);
        // scroll 400 -> page 1 covers 600, page 2 covers 176.
        assert_eq!(dominant(400.0, 800.0, &h, 24.0), 1);
        // One tall page fills the viewport outright (page 2 spans 2024..4024).
        assert_eq!(dominant(2500.0, 800.0, &[2000.0; 3], 24.0), 2);
        // Nothing measured -> 1. No viewport height yet -> top-edge answer.
        assert_eq!(dominant(0.0, 800.0, &[], 24.0), 1);
        assert_eq!(dominant(900.0, 0.0, &[800.0, 800.0], 24.0), 2);
    }

    /// A jump that aligns page P's top with the viewport top reports P, even
    /// when several shorter pages are visible below it.
    #[test]
    fn jump_to_page_top_reports_that_page() {
        let h = vec![400.0; 10];
        for target in 1..=8u32 {
            let top = page_top_css(target as usize - 1, &h, 24.0);
            assert_eq!(dominant(top, 800.0, &h, 24.0), target, "target {target}");
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
        let start = dominant(st, vh, &base, gap);
        assert_eq!(start, 11);
        let mut heights = base;
        for f in [0.93, 0.857, 0.833, 0.8] {
            st = anchored_scroll(st, vh, &heights, gap, f, vh * 0.5).unwrap();
            heights = heights.iter().map(|x| x * f).collect();
            assert_eq!(
                dominant(st, vh, &heights, gap),
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
        let start_page = dominant(scroll, vh, &heights, PAGE_GAP);
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
            dominant(scroll, vh, &heights, PAGE_GAP),
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
mod tests {
    use super::*;
    use crate::layout::RenderBudget;

    const H: [f64; 3] = [100.0, 200.0, 100.0];

    #[test]
    fn window_expands_and_clamps() {
        let b = RenderBudget { look_frac: 1.0, max_items: 7 };
        assert_eq!(DocumentLayout::new(&H, 24.0).window(124.0, 200.0, b), Some((0, 2)));
        let none = RenderBudget { look_frac: 0.0, max_items: 7 };
        assert_eq!(DocumentLayout::new(&H, 24.0).window(124.0, 200.0, none), Some((1, 1)));
    }
}


#[cfg(test)]
mod render_range_tests {
    use super::*;
    use crate::layout::{page_top_css, PAGE_GAP, RenderBudget, Strip};

    fn window(
        scroll_top: f64,
        viewport_h: f64,
        heights: &[f64],
        gap: f64,
        budget: RenderBudget,
    ) -> Option<(usize, usize)> {
        DocumentLayout::new(heights, gap).window(scroll_top, viewport_h, budget)
    }

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
            let (f, l) = window(st, vh, &h, GAP, b).unwrap();
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
        assert_eq!(window(mid, vh, &h, GAP, b), Some((idx, idx)));

        // Scrolled so the page bottom is within one screenful: next is mounted
        // and ready, BEFORE any of it is on screen.
        let page_bottom = page_top_css(idx, &h, GAP) + h[idx];
        let near = page_bottom - vh - 10.0; // bottom is 10px beyond the viewport
        let (f, l) = window(near, vh, &h, GAP, b).unwrap();
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
        let (f, _) = window(just_in, vh, &h, GAP, b).unwrap();
        assert_eq!(f, idx - 1, "the page just behind should still be warm");
        // Well into page 10: page 9 is now more than a screenful behind.
        let deep = top + vh * 1.5;
        let (f2, _) = window(deep, vh, &h, GAP, b).unwrap();
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
                    let Some((f, l)) = window(st, vh, &h, GAP, budget) else {
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
            let (f, l) = window(st, vh, &h, GAP, b).unwrap();
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
        assert_eq!(window(0.0, 800.0, &[], GAP, b), None);
        // Zero-height viewport: still mounts the page under the scroll point.
        let h = uniform(5, 500.0);
        assert!(window(1100.0, 0.0, &h, GAP, b).is_some());
        // Past the end of the document: `None`, exactly as `visible_range`
        // answers, so the caller's existing "nothing to render" path is
        // reached rather than a surprise last-page mount.
        assert_eq!(window(99_999.0, 800.0, &h, GAP, b), None);
        assert_eq!(span_overlapping(99_999.0, 800.0, &h, GAP), None);
        // A zero max_items is treated as 1 rather than producing an empty range.
        let bad = RenderBudget { look_frac: 0.0, max_items: 0 };
        let (f, l) = window(1100.0, 800.0, &h, GAP, bad).unwrap();
        assert!(l >= f);
    }

    /// Parked in the gap between two pages: nothing is strictly visible, and
    /// the window must still resolve to the neighbouring pages.
    #[test]
    fn gap_parking_still_mounts_neighbours() {
        let h = [100.0, 200.0, 100.0];
        // 100..124 is the gap after page 0; a 15px viewport sits inside it.
        let b = RenderBudget { look_frac: 1.0, max_items: 7 };
        let got = window(104.0, 15.0, &h, GAP, b);
        assert!(got.is_some(), "a gap position must still mount something");
        let (f, l) = got.unwrap();
        assert!(f == 0 && l >= 1, "expected the pages either side, got {f}..={l}");
    }
}
