//! The zoom anchor math: keep the document point under the reader's eyes
//! at the same screen position when every page height is multiplied by a
//! scale `factor`.
//!
//! Page heights are linear in scale, so a scale change can be applied to
//! the whole layout in one synchronous step without waiting for a render;
//! gaps are chrome and deliberately NOT scaled. Works entirely on the
//! prebuilt prefix sums: the page lookup is `O(log n)`
//! ([`Strip::index_at`]) and every offset/total is `O(1)` — no per-call
//! rebuild of the strip and no linear walk over the heights.

use super::DocumentLayout;

impl DocumentLayout {
    /// Scroll offset that keeps the document point currently at
    /// `anchor_screen_y` (viewport coordinates, from the top of the
    /// scrollport) pinned to the same screen position after every page
    /// height is multiplied by `factor`.
    ///
    /// Page heights are linear in scale, so a scale change can be applied to
    /// the whole layout in one synchronous step without waiting for a render;
    /// gaps are chrome and deliberately NOT scaled. Returns `None` when there
    /// is nothing to anchor (no measured heights, or a nonsensical factor).
    ///
    /// Works entirely on the prebuilt prefix sums: the page lookup is
    /// `O(log n)` (`Strip::index_at`) and every offset/total is `O(1)` — no
    /// per-call rebuild of the strip and no linear walk over the heights.
    pub fn anchored_scroll(
        &self,
        scroll_top: f64,
        viewport_h: f64,
        factor: f64,
        anchor_screen_y: f64,
    ) -> Option<f64> {
        let n = self.strip.len();
        if n == 0 || factor <= 0.0 || !factor.is_finite() {
            return None;
        }
        // Sitting at the very top: pin the top, or the start of page 1 would be
        // pushed up off the viewport. The bottom edge needs no equivalent case:
        // the `max_scroll` clamp below keeps the end of the document pinned.
        if scroll_top <= 0.5 {
            return Some(0.0);
        }

        let doc_y = scroll_top + anchor_screen_y;

        // Locate the page containing `doc_y`, plus how far down that page it
        // sits. `over` carries any excess past the page bottom (i.e. a
        // position inside the following gap) so it can be re-added unscaled.
        //
        // `index_at` reports the page whose span contains the position, but it
        // treats a gap position as belonging to the NEXT page; the anchoring
        // semantics attribute it to the page ABOVE (a gap has no content to
        // anchor), so a position that landed inside a gap is re-attributed
        // with `frac = 1` and `over = position - bottom`.
        let mut idx = self.strip.index_at(doc_y).min(n - 1);
        let start = self.strip.offset(idx);
        let h = self.height(idx);
        let bottom = start + h;
        let (frac, over) = if doc_y < start {
            // Inside the gap above `idx`: the page above owns the point.
            let prev = idx.saturating_sub(1);
            idx = prev;
            let prev_bottom = self.strip.offset(prev) + self.height(prev);
            (1.0, (doc_y - prev_bottom).max(0.0))
        } else if doc_y >= bottom {
            // Inside the gap below `idx` (or past the end of the last page).
            (1.0, doc_y - bottom)
        } else if h > 0.0 {
            (((doc_y - start) / h).clamp(0.0, 1.0), 0.0)
        } else {
            (0.0, 0.0)
        };

        // Where that same point lands once every page is `factor` taller.
        // `offset(i)` = sum(heights[..i]) + gap * i, so the scaled prefix is
        // factor * (offset(i) - gap*i) + gap*i.
        let scaled_prefix =
            (self.strip.offset(idx) - self.gap * idx as f64) * factor + self.gap * idx as f64;
        let new_doc_y = scaled_prefix + frac * h * factor + over;

        // total() = sum(heights) + gap * (n-1); same split.
        let total_new = (self.strip.total() - self.gap * (n - 1) as f64) * factor
            + self.gap * (n - 1) as f64;
        let max_scroll = (total_new - viewport_h).max(0.0);
        Some((new_doc_y - anchor_screen_y).clamp(0.0, max_scroll))
    }}

#[cfg(test)]
mod anchor_tests {
    use crate::layout::{anchored_scroll, Strip};

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

