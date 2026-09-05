//! The reader's view model, for any format.
//!
//! Which view mode is on, which axis a strip scrolls, the gap between pages,
//! how far ahead to mount, and the two maths every strip shares with the
//! zoom coordinator (spread arithmetic, and holding the point under the
//! reader's eyes still across a rescale). A reflowable document is laid out
//! through exactly the same model as a PDF, so none of this may name a format
//! — that is the whole point of it living here rather than beside the page
//! canvas.
//!
//! The windowing arithmetic itself lives in `virtual-list` and
//! `virtual-list-leptos`; this module carries the reader's policy and re-exports
//! [`Budget`] so a caller sizes a strip without naming two crates. The PDF
//! page frame's own constant (the toolbar band the search reveal must clear)
//! stays at `pdf_core`'s root.

pub use virtual_list::Budget;

/// Gap between pages in the continuous reader, in CSS px.
pub const PAGE_GAP: f64 = 24.0;

/// Comfortable read-ahead: half a screenful each way, up to 3 mounted pages
/// total (visible + ~1 above + ~1 below). Each mounted page at 2× DPR plus
/// its raw is ~64MB worst case, so the ceiling is what keeps idle RAM sane.
pub const RENDER_BUDGET: Budget = Budget::screenfuls(0.5, 3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    /// One page at a time. Paginated.
    Single,
    /// Two pages side by side, no gap (a "spread"). Paginated.
    Spread,
    #[default]
    /// All pages in one vertical strip; wheel/keys scroll vertically.
    ScrollVertical,
    /// All pages in one horizontal strip; wheel/keys scroll horizontally.
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

/// Where the document point that was under the viewport centre lands once
/// the item it sits in has been scaled by `factor` and the gaps between
/// items have been left alone.
///
/// `index` is the anchored item, already resolved in `O(log n)` by the
/// strip's `index_at`; `height` is that item's pre-scale height,
/// `above_with_gap` the extent of the items above it (gaps included) and
/// `height_sum` their heights alone — the part that scales. Shared by the
/// PDF page strip and the text column: both rescale layout, not transforms,
/// and both hold the point under the reader's eyes while they do.
pub fn anchored_position(
    height: f64,
    above_with_gap: f64,
    height_sum: f64,
    gap: f64,
    centre_y_doc: f64,
    factor: f64,
    index: usize,
) -> f64 {
    // Where the items above land at the new scale, plus this point's offset
    // inside them. An anchor that fell in the gap keeps the unscaled
    // remainder: the gap is fixed chrome and never scales.
    let above = height_sum * factor + index as f64 * gap;
    let offset_inside = centre_y_doc - above_with_gap;
    above + if offset_inside <= height {
        offset_inside * factor
    } else {
        height * factor + (offset_inside - height)
    }
}

/// First 1-based page of the two-up spread containing `page`.
/// Pages 1 and 2 form the first spread, so both report 1; a 0 (no page yet)
/// clamps to the first spread too.
pub fn spread_start(page: u32) -> u32 {
    ((page.max(1) - 1) / 2) * 2 + 1
}

/// Zero-based index of the spread containing `page` — the `<For>` key the
/// spread layout renders from. The inverse of [`spread_start`].
pub fn spread_index(page: u32) -> u32 {
    (page.max(1) - 1) / 2
}

/// First 1-based page of the LAST spread of an `n`-page document. `n == 0`
/// (no document) reports 1 so clamping still lands on page 1.
pub fn last_spread_start(page_count: u32) -> u32 {
    if page_count == 0 {
        1
    } else {
        spread_start(page_count)
    }
}

/// The page a "previous spread" step lands on: the start of the spread
/// before `page`'s, saturating at the first spread.
pub fn spread_step_prev(page: u32) -> u32 {
    spread_start(page).saturating_sub(2).max(1)
}

/// The page a "next spread" step lands on: the start of the spread after
/// `page`'s, clamped so the last spread stays put.
pub fn spread_step_next(page_count: u32, page: u32) -> u32 {
    (spread_start(page) + 2).min(last_spread_start(page_count))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A point inside a page moves with the page: page 0 spans 0..100, so 40
    /// inside it lands at 80 after a 2× zoom.
    #[test]
    fn an_anchor_on_a_page_scales_with_it() {
        assert_eq!(anchored_position(100.0, 0.0, 0.0, 20.0, 40.0, 2.0, 0), 80.0);
    }

    /// The load-bearing case: the gap between pages is fixed chrome, so an
    /// anchor that falls in it is carried along by the pages above it at their
    /// scale and keeps the unscaled remainder of the gap. A uniform rescale of
    /// the whole extent would put it at 110 × 2 = 220 instead.
    #[test]
    fn an_anchor_in_a_gap_keeps_the_gap_unscaled() {
        // Page 0 ends at 100, the gap spans 100..120; 110 is 10 into the gap,
        // so after doubling, page 0 ends at 200 and the gap is still 20.
        assert_eq!(anchored_position(100.0, 0.0, 0.0, 20.0, 110.0, 2.0, 0), 210.0);
    }

    /// Every gap above the reader counts, not just the one it is standing in:
    /// deep in a long document the unscaled sum is what keeps the page still.
    #[test]
    fn gaps_above_the_anchor_hold_the_page_still() {
        // Page 5 starts at 5 * (100 + 20) = 600; +30 into it is 630, which
        // scales to 5 * 200 + 5 * 20 + 60 = 1160.
        assert_eq!(anchored_position(100.0, 600.0, 500.0, 20.0, 630.0, 2.0, 5), 1160.0);
        // Zooming back out by the same factor returns to the exact start.
        let forward = anchored_position(100.0, 600.0, 500.0, 20.0, 630.0, 2.0, 5);
        assert_eq!(anchored_position(200.0, 1100.0, 1000.0, 20.0, forward, 0.5, 5), 630.0);
    }

    /// A centre past the end of a short document still lands at the scaled end,
    /// keeping the overflow beyond the single page exactly as long as it was.
    #[test]
    fn a_centre_past_the_end_keeps_the_overflow_unscaled() {
        // 900 is far past the single page: the page scales to 200 and the 800
        // of overflow beyond it stays exactly as long as it was.
        assert_eq!(anchored_position(100.0, 0.0, 0.0, 20.0, 900.0, 2.0, 0), 1000.0);
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
}
