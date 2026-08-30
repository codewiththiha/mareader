//! The per-page palette and the interpolation that walks it.
//!
//! Continuous mode's question is "what colour is the reader looking at RIGHT
//! NOW?", and the honest answer is a position along the book, not a page
//! pair. The shell reports the viewport's visible-paint-weighted mean page
//! index — resting on page N it is exactly `N.0`, straddling pages N and N+1
//! at 40/60 it is `N + 0.6` — and [`PagePalette::colour_at`] resolves that
//! against the known page colours like a ladder: piecewise-linear between
//! neighbouring pages, held flat past either end.
//!
//! The ladder is what the old page-pair blend could not do. A pair
//! `(dominant, dominant + 1)` is blind to the page BEFORE the dominant one,
//! so right after a handover — when the previous page still fills half the
//! window — the backdrop snapped to the new page's colour while the eye
//! still saw the old one. A weighted position carries every visible page's
//! share, so the backdrop meets the pages where they actually are, with no
//! seam at the handover.

use std::collections::BTreeMap;

use crate::color::{lerp, Rgb};

/// Known paper colours, keyed by 1-based page number.
#[derive(Debug, Default, Clone)]
pub struct PagePalette {
    pages: BTreeMap<u32, Rgb>,
}

impl PagePalette {
    pub fn new() -> Self {
        Self::default()
    }

    /// Remember page `page`'s colour. A re-detection overwrites: the newest
    /// sample wins, so a page whose colour was guessed while its raster was
    /// still streaming corrects itself on the next frame.
    pub fn set(&mut self, page: u32, colour: Rgb) {
        self.pages.insert(page, colour);
    }

    pub fn get(&self, page: u32) -> Option<Rgb> {
        self.pages.get(&page).copied()
    }

    pub fn contains(&self, page: u32) -> bool {
        self.pages.contains_key(&page)
    }

    pub fn len(&self) -> usize {
        self.pages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }

    pub fn clear(&mut self) {
        self.pages.clear();
    }

    /// The colour at a fractional page `position` (1-based): exactly page
    /// N's colour at `N.0`, the linear blend of pages N and N+1 at `N + t`,
    /// clamped to the first/last known page outside the palette's span.
    ///
    /// Pages whose colour is still unknown are skipped over, not treated as
    /// blanks: the ladder simply runs between the nearest known pages on
    /// either side. `None` only when no page's colour is known at all.
    pub fn colour_at(&self, position: f64) -> Option<Rgb> {
        if self.pages.is_empty() || !position.is_finite() {
            return None;
        }
        let first = *self.pages.keys().next()? as f64;
        let last = *self.pages.keys().next_back()? as f64;
        let pos = position.clamp(first, last);

        // The greatest known page at or below `pos`, and the smallest known
        // page above it — the two knots the position falls between.
        let floor = pos.floor() as u32;
        let (lo_key, lo_colour) = self.pages.range(..=floor).next_back()?;
        let hi = self.pages.range(floor + 1..).next();
        let Some((hi_key, hi_colour)) = hi else {
            return Some(*lo_colour); // at (or past) the last known page
        };
        let span = f64::from(*hi_key - *lo_key);
        if span <= f64::EPSILON {
            return Some(*lo_colour);
        }
        let t = (pos - f64::from(*lo_key)) / span;
        Some(lerp(*lo_colour, *hi_colour, t))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CREAM: Rgb = Rgb::new(0xfa, 0xf4, 0xe8);
    const INK: Rgb = Rgb::new(0x40, 0x40, 0x40);
    const WHITE: Rgb = Rgb::new(0xff, 0xff, 0xff);

    #[test]
    fn an_empty_palette_has_no_opinion() {
        assert_eq!(PagePalette::new().colour_at(1.0), None);
        assert!(PagePalette::new().is_empty());
    }

    #[test]
    fn resting_on_a_page_is_that_pages_colour() {
        let mut p = PagePalette::new();
        p.set(1, CREAM);
        p.set(2, INK);
        assert_eq!(p.colour_at(1.0), Some(CREAM));
        assert_eq!(p.colour_at(2.0), Some(INK));
    }

    #[test]
    fn the_midpoint_of_two_pages_blends_their_papers() {
        let mut p = PagePalette::new();
        p.set(1, CREAM);
        p.set(2, WHITE);
        // The assertion compares against the same lerp, so the test pins the
        // BEHAVIOUR (midpoint blend) without hard-coding rounding.
        let mid = p.colour_at(1.5).unwrap();
        assert_eq!(mid, lerp(CREAM, WHITE, 0.5));
    }

    #[test]
    fn a_weighted_position_carries_both_pages_shares() {
        // 40% page 1 + 60% page 2 → position 1.6 → 60% of page 2's colour.
        // This is the case the old pair blend got wrong after handovers.
        let mut p = PagePalette::new();
        p.set(1, CREAM);
        p.set(2, WHITE);
        assert_eq!(p.colour_at(1.6), Some(lerp(CREAM, WHITE, 0.6)));
        assert_eq!(p.colour_at(1.25), Some(lerp(CREAM, WHITE, 0.25)));
    }

    #[test]
    fn positions_clamp_to_the_known_span() {
        let mut p = PagePalette::new();
        p.set(3, CREAM);
        p.set(5, INK);
        assert_eq!(p.colour_at(0.0), Some(CREAM)); // below the first page
        assert_eq!(p.colour_at(9.0), Some(INK)); // past the last page
        assert_eq!(p.colour_at(3.0), Some(CREAM));
    }

    #[test]
    fn an_unknown_page_blends_across_the_gap() {
        // Page 4's colour never resolved; a reader at 3.5 is half way
        // between page 3 and page 5 in every sense that matters.
        let mut p = PagePalette::new();
        p.set(3, CREAM);
        p.set(5, INK);
        assert_eq!(p.colour_at(3.5), Some(lerp(CREAM, INK, 0.25)));
        assert_eq!(p.colour_at(4.0), Some(lerp(CREAM, INK, 0.5)));
        assert_eq!(p.colour_at(4.9), Some(lerp(CREAM, INK, 0.95)));
    }

    #[test]
    fn a_single_known_page_holds_its_colour_everywhere() {
        let mut p = PagePalette::new();
        p.set(7, INK);
        for pos in [0.0, 1.0, 6.9, 7.0, 7.5, 100.0] {
            assert_eq!(p.colour_at(pos), Some(INK), "pos {pos}");
        }
    }

    #[test]
    fn non_finite_positions_hold_their_tongue() {
        let mut p = PagePalette::new();
        p.set(1, CREAM);
        assert_eq!(p.colour_at(f64::NAN), None);
        assert_eq!(p.colour_at(f64::INFINITY), None);
    }

    #[test]
    fn clear_empties_the_palette() {
        let mut p = PagePalette::new();
        p.set(1, CREAM);
        p.clear();
        assert!(p.is_empty());
        assert_eq!(p.colour_at(1.0), None);
    }
}
