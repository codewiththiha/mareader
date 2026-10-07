//! The reader's view model for any format: modes, gaps, look-ahead and
//! the shared zoom maths.

/// Gap between pages in the continuous reader, in CSS px.
pub const PAGE_GAP: f64 = 24.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewMode {
    Single,
    Spread,
    #[default]
    ScrollVertical,
    ScrollHorizontal,
}

impl ViewMode {
    /// Auto-scroll only makes sense on the two scrolling modes.
    pub fn can_scroll(self) -> bool {
        matches!(self, ViewMode::ScrollVertical | ViewMode::ScrollHorizontal)
    }

    pub fn is_paginated(self) -> bool {
        matches!(self, ViewMode::Single | ViewMode::Spread)
    }
}

/// Reading progress along one scroll axis.
pub fn scroll_fraction(offset: f64, total: f64, viewport: f64) -> f64 {
    let travel = total - viewport;
    if travel > 0.0 {
        (offset / travel).clamp(0.0, 1.0)
    } else {
        0.0
    }
}

/// The inverse of [`scroll_fraction`]: offset for a reading fraction.
pub fn fraction_offset(fraction: f64, total: f64, viewport: f64) -> f64 {
    fraction.clamp(0.0, 1.0) * (total - viewport).max(0.0)
}

/// Where the point under the viewport centre lands after a rescale.
pub fn anchored_position(
    height: f64,
    above_with_gap: f64,
    height_sum: f64,
    gap: f64,
    centre_y_doc: f64,
    factor: f64,
    index: usize,
) -> f64 {
    // Where the items above land at the new scale.
    let above = height_sum * factor + index as f64 * gap;
    let offset_inside = centre_y_doc - above_with_gap;
    above
        + if offset_inside <= height {
            offset_inside * factor
        } else {
            height * factor + (offset_inside - height)
        }
}

/// First 1-based page of the two-up spread containing `page`.
pub fn spread_start(page: u32) -> u32 {
    ((page.max(1) - 1) / 2) * 2 + 1
}

/// Zero-based index of the spread containing `page`.
pub fn spread_index(page: u32) -> u32 {
    (page.max(1) - 1) / 2
}

/// First 1-based page of the LAST spread of an `n`-page document.
pub fn last_spread_start(page_count: u32) -> u32 {
    if page_count == 0 {
        1
    } else {
        spread_start(page_count)
    }
}

/// The page a "previous spread" step lands on.
pub fn spread_step_prev(page: u32) -> u32 {
    spread_start(page).saturating_sub(2).max(1)
}

/// The page a "next spread" step lands on.
pub fn spread_step_next(page_count: u32, page: u32) -> u32 {
    (spread_start(page) + 2).min(last_spread_start(page_count))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A point inside a page moves with the page.
    #[test]
    fn an_anchor_on_a_page_scales_with_it() {
        assert_eq!(anchored_position(100.0, 0.0, 0.0, 20.0, 40.0, 2.0, 0), 80.0);
    }

    /// The gap is fixed chrome: an anchor in it keeps the unscaled
    /// remainder.
    #[test]
    fn an_anchor_in_a_gap_keeps_the_gap_unscaled() {
        // Page 0 ends at 100, the gap spans 100..120, 110 is 10 into it.
        assert_eq!(
            anchored_position(100.0, 0.0, 0.0, 20.0, 110.0, 2.0, 0),
            210.0
        );
    }

    /// Every gap above the reader counts, not just the current one.
    #[test]
    fn gaps_above_the_anchor_hold_the_page_still() {
        // Page 5 starts at 600; +30 into it scales to 1160.
        assert_eq!(
            anchored_position(100.0, 600.0, 500.0, 20.0, 630.0, 2.0, 5),
            1160.0
        );
        // Zooming back out by the same factor returns to the exact start.
        let forward = anchored_position(100.0, 600.0, 500.0, 20.0, 630.0, 2.0, 5);
        assert_eq!(
            anchored_position(200.0, 1100.0, 1000.0, 20.0, forward, 0.5, 5),
            630.0
        );
    }

    /// A centre past the end of a short document lands at the scaled end.
    #[test]
    fn a_centre_past_the_end_keeps_the_overflow_unscaled() {
        // 900 is past the single page: it scales to 200, overflow stays 800.
        assert_eq!(
            anchored_position(100.0, 0.0, 0.0, 20.0, 900.0, 2.0, 0),
            1000.0
        );
    }

    #[test]
    fn spreads_pair_pages_and_clamp_degenerate_input() {
        assert_eq!(spread_start(0), 1);
        assert_eq!(spread_start(1), 1);
        assert_eq!(spread_start(2), 1);
        assert_eq!(spread_start(3), 3);
        assert_eq!(spread_start(4), 3);
        assert_eq!(spread_start(u32::MAX), ((u32::MAX - 1) / 2) * 2 + 1);
    }

    #[test]
    fn spread_index_is_the_zero_based_for_key() {
        assert_eq!(spread_index(1), 0);
        assert_eq!(spread_index(2), 0);
        assert_eq!(spread_index(3), 1);
        assert_eq!(spread_index(4), 1);
        // Round-trip: a spread's first page maps back to its own index.
        for index in [0u32, 1, 7, 1000] {
            assert_eq!(spread_index(spread_start(index * 2 + 1)), index);
        }
    }

    #[test]
    fn last_spread_start_holds_an_odd_tail_and_a_documentless_zero() {
        assert_eq!(last_spread_start(0), 1);
        assert_eq!(last_spread_start(1), 1);
        assert_eq!(last_spread_start(2), 1);
        assert_eq!(last_spread_start(3), 3);
        assert_eq!(last_spread_start(4), 3);
        assert_eq!(last_spread_start(5), 5);
    }

    #[test]
    fn steps_saturate_at_the_first_and_last_spread() {
        // At the first spread, prev stays put.
        assert_eq!(spread_step_prev(1), 1);
        assert_eq!(spread_step_prev(2), 1);
        // One spread back per step.
        assert_eq!(spread_step_prev(3), 1);
        assert_eq!(spread_step_prev(5), 3);
        // At the last spread, next stays put — even and odd tails alike.
        assert_eq!(spread_step_next(5, 5), 5);
        assert_eq!(spread_step_next(4, 3), 3);
        // Otherwise one spread forward.
        assert_eq!(spread_step_next(5, 1), 3);
        assert_eq!(spread_step_next(5, 3), 5);
        assert_eq!(spread_step_next(0, 1), 1);
    }

    /// The pair is exact inverses, so a resume point survives as a
    /// fraction.
    #[test]
    fn reading_progress_is_travel_relative_and_round_trips() {
        assert_eq!(scroll_fraction(250.0, 1000.0, 500.0), 0.5);
        assert_eq!(fraction_offset(0.5, 1000.0, 500.0), 250.0);
        // Both ends land back on themselves.
        let (total, viewport) = (1000.0, 500.0);
        for offset in [0.0, 250.0, 500.0] {
            let there = scroll_fraction(offset, total, viewport);
            assert_eq!(fraction_offset(there, total, viewport), offset);
        }
    }

    /// A strip that fits its viewport has no travel: it reads 0.
    #[test]
    fn a_strip_with_no_travel_reads_zero_rather_than_dividing() {
        assert_eq!(scroll_fraction(0.0, 500.0, 500.0), 0.0);
        assert_eq!(scroll_fraction(0.0, 100.0, 500.0), 0.0);
        assert_eq!(fraction_offset(0.5, 100.0, 500.0), 0.0);
    }
}
