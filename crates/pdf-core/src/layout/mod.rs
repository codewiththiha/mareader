//! Reader layout policy shared across the app.
//!
//! Geometry lives in `virtual-list` and `virtual-list-leptos`; this module just
//! carries the reader's constants and view-mode enum.

pub use virtual_list::Budget;

/// Gap between pages in the continuous reader, in CSS px.
pub const PAGE_GAP: f64 = 24.0;

/// Height of the glass toolbar, in CSS px. The viewer scrollport spans the
/// full window height so pages travel under the translucent header; each view
/// offsets its content by this inset.
///
/// MUST stay in sync with Tailwind `h-12` / `mt-12` on the title bar and the
/// continuous page-list offset.
pub const TOOLBAR_H: f64 = 48.0;

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
