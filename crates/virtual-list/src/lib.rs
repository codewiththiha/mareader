//! Windowing math for virtualized scrolling lists of variably-sized items.
//!
//! A [`Strip`] is a column of items laid out one after another with a fixed gap
//! between them. It answers the four questions a virtualized list asks every
//! frame:
//!
//! - [`Strip::offset`] — where does item `i` start?
//! - [`Strip::total`] — how tall is the whole column?
//! - [`Strip::window`] — which items should be mounted right now?
//! - [`Strip::dominant`] — which item is the reader actually looking at?
//!
//! Everything is `f64` in a unit of your choosing (CSS px, points, logical
//! pixels). There is no DOM and no framework here: feed it sizes, get back
//! indices and offsets.
//!
//! # Performance
//!
//! The obvious implementation walks the size array to find an item's offset,
//! which is `O(n)` per query and `O(n²)` for a list that positions every
//! mounted item each frame. [`Strip`] stores a prefix-sum table instead, making
//! [`Strip::offset`] `O(1)` and every positional query an `O(log n)` binary
//! search. Building the table is `O(n)`, done once when the sizes change.
//!
//! # Example
//!
//! ```
//! use virtual_list::{Budget, Strip};
//!
//! let strip = Strip::new([100.0, 200.0, 100.0], 24.0);
//! assert_eq!(strip.offset(0), 0.0);
//! assert_eq!(strip.offset(1), 124.0);
//! assert_eq!(strip.offset(2), 348.0);
//! assert_eq!(strip.total(), 448.0);
//!
//! // What is on screen in a 150-tall viewport parked at the top?
//! let win = strip.visible(0.0, 150.0).unwrap();
//! assert_eq!((win.first, win.last), (0, 1));
//! ```

#![cfg_attr(not(feature = "std"), no_std)]
#![forbid(unsafe_code)]
#![warn(missing_docs)]

extern crate alloc;

use alloc::vec::Vec;

/// An inclusive range of item indices, `first ..= last`.
///
/// Always non-empty: a `Window` is only ever produced when at least one item
/// qualifies, so `first <= last` holds by construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    /// First item in the range (0-based, inclusive).
    pub first: usize,
    /// Last item in the range (0-based, inclusive).
    pub last: usize,
}

impl Window {
    /// Number of items in the range.
    #[inline]
    pub const fn len(&self) -> usize {
        self.last - self.first + 1
    }

    /// Always `false` — a `Window` is non-empty by construction. Present
    /// because clippy asks for it whenever `len` exists.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        false
    }

    /// Whether `index` falls inside the range.
    #[inline]
    pub const fn contains(&self, index: usize) -> bool {
        self.first <= index && index <= self.last
    }

    /// Iterate the indices in the range.
    #[inline]
    pub fn iter(&self) -> core::ops::RangeInclusive<usize> {
        self.first..=self.last
    }
}

impl IntoIterator for Window {
    type Item = usize;
    type IntoIter = core::ops::RangeInclusive<usize>;

    fn into_iter(self) -> Self::IntoIter {
        self.first..=self.last
    }
}

/// How much to keep mounted around the viewport.
///
/// # Why read-ahead is measured in screenfuls
///
/// Expressing read-ahead as a fixed number of *items* silently means two
/// different things at different item sizes. When items are short, three of
/// them is a modest read-ahead; when one item is several screens tall, three of
/// them is a huge amount of off-screen work the reader cannot reach for many
/// seconds — and if each mounted item owns an expensive resource (a raster, a
/// video, a canvas) that is where the memory goes.
///
/// One screenful ahead is one screenful ahead at any item size, so
/// [`look_frac`](Self::look_frac) behaves the same whether the list is zoomed
/// in or out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Budget {
    /// Read-ahead and read-behind, as a multiple of the viewport size.
    ///
    /// `0.5` keeps half a screenful mounted on each side of what is visible;
    /// `1.0` would be a full screenful. `0.0` mounts only what is strictly
    /// on screen. Negative values are clamped to `0.0`.
    pub look_frac: f64,
    /// Hard ceiling on how many items may be mounted at once.
    ///
    /// Only ever trims items that are **not** visible, so correctness never
    /// depends on this being large enough. `0` is treated as `1`.
    pub max_items: usize,
}

impl Default for Budget {
    fn default() -> Self {
        Self {
            look_frac: 0.5,
            max_items: 5,
        }
    }
}

/// A column of variably-sized items separated by a fixed gap.
///
/// Construct one with [`Strip::new`] (explicit sizes) or [`Strip::uniform`]
/// (all items the same size), then query it. Rebuild it when the sizes change.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Strip {
    /// `starts[i]` is the offset of item `i`; `starts[len]` is the total
    /// extent including the trailing item but no trailing gap. Always has
    /// `len + 1` entries, or is empty when there are no items.
    starts: Vec<f64>,
    gap: f64,
}

impl Strip {
    /// Build a strip from explicit item sizes. Inputs are trusted as-is.
    pub fn new<I>(sizes: I, gap: f64) -> Self
    where
        I: IntoIterator<Item = f64>,
    {
        let iter = sizes.into_iter();
        let (lower, _) = iter.size_hint();
        let mut starts = Vec::with_capacity(lower + 1);
        let mut acc = 0.0;
        for size in iter {
            starts.push(acc);
            acc += size + gap;
        }
        if !starts.is_empty() {
            // Total extent excludes the gap after the final item.
            starts.push(acc - gap);
        }
        Self { starts, gap }
    }

    /// Build a strip of `count` items that all have the same size.
    pub fn uniform(count: usize, size: f64, gap: f64) -> Self {
        Self::new(core::iter::repeat_n(size, count), gap)
    }

    /// Number of items.
    #[inline]
    pub fn len(&self) -> usize {
        self.starts.len().saturating_sub(1)
    }

    /// Whether the strip has no items.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.starts.is_empty()
    }

    /// The gap between adjacent items.
    #[inline]
    pub fn gap(&self) -> f64 {
        self.gap
    }

    /// Offset of the start of item `index`.
    ///
    /// Returns `0.0` for an empty strip, and the total extent for an index at
    /// or past the end, so callers can position a trailing spacer without a
    /// bounds check.
    #[inline]
    pub fn offset(&self, index: usize) -> f64 {
        match self.starts.get(index) {
            Some(&v) => v,
            None => self.total(),
        }
    }

    /// Size of item `index`, or `0.0` if out of range.
    #[inline]
    pub fn size(&self, index: usize) -> f64 {
        let len = self.len();
        if index >= len {
            return 0.0;
        }
        let end = if index + 1 == len {
            self.total()
        } else {
            self.starts[index + 1] - self.gap
        };
        (end - self.starts[index]).max(0.0)
    }

    /// Total extent of the column: every item plus the gaps between them, with
    /// no trailing gap. `0.0` when empty.
    #[inline]
    pub fn total(&self) -> f64 {
        self.starts.last().copied().unwrap_or(0.0)
    }

    /// Index of the item whose span contains `pos`.
    ///
    /// `pos` is treated as the *leading edge* of a viewport, so an item ending
    /// exactly at `pos` has scrolled out and the next item is reported. That
    /// makes the boundary agree with [`overlapping`](Self::overlapping), which
    /// uses the same strict top edge — otherwise "the item at the top of the
    /// scrollport" and "the first visible item" would disagree by one at every
    /// exact boundary.
    ///
    /// Positions inside a gap resolve to the item *below* the gap for the same
    /// reason. Positions past the end resolve to the last item, so this always
    /// names a real item (given a non-empty strip). Returns `0` when empty.
    pub fn index_at(&self, pos: f64) -> usize {
        let len = self.len();
        if len == 0 || pos <= 0.0 {
            return 0;
        }
        // Last item whose start is <= pos.
        let idx = self.starts[..len].partition_point(|&s| s <= pos).saturating_sub(1);
        // If pos is at or past that item's end (i.e. in the gap below it, or
        // exactly on its bottom edge), the next item now leads — except at the
        // very end of the strip, which has no next item to report.
        if self.starts[idx] + self.size(idx) <= pos && idx + 1 < len {
            idx + 1
        } else {
            idx
        }
    }

    /// Inclusive range of items overlapping the span `[top, top + extent)`.
    ///
    /// An item that ends exactly at `top` has scrolled out and is excluded; an
    /// item that starts exactly at the bottom edge is included. Returns `None`
    /// for an empty strip, or when the span lies entirely within a gap.
    pub fn overlapping(&self, top: f64, extent: f64) -> Option<Window> {
        let len = self.len();
        if len == 0 {
            return None;
        }
        let bottom = top + extent.max(0.0);

        // First candidate: the item containing `top`, which may end before it.
        let mut first = self.index_at(top);
        if self.starts[first] + self.size(first) <= top {
            first += 1;
        }
        if first >= len || self.starts[first] > bottom {
            return None;
        }
        // Last item whose start is <= bottom.
        let last = self.starts[..len].partition_point(|&s| s <= bottom).saturating_sub(1);
        (last >= first).then_some(Window { first, last })
    }

    /// Inclusive range of items that are at least partly on screen.
    ///
    /// Shorthand for [`overlapping`](Self::overlapping) with the raw viewport.
    #[inline]
    pub fn visible(&self, scroll_top: f64, viewport: f64) -> Option<Window> {
        self.overlapping(scroll_top, viewport)
    }

    /// Inclusive range of items to keep mounted.
    ///
    /// The window is everything overlapping
    /// `[scroll_top - look, scroll_top + viewport + look]` where
    /// `look = budget.look_frac * viewport`, trimmed to `budget.max_items`.
    ///
    /// Two invariants hold for any `budget`:
    ///
    /// - every partly-visible item is always included, so no budget can blank
    ///   out what the reader is looking at;
    /// - trimming drops the item furthest from the viewport first and prefers
    ///   to keep the item below, so the next item the reader reaches is the
    ///   last one evicted.
    pub fn window(&self, scroll_top: f64, viewport: f64, budget: Budget) -> Option<Window> {
        if self.is_empty() {
            return None;
        }
        let vh = viewport.max(0.0);
        let look = budget.look_frac.max(0.0) * vh;

        let padded = self.overlapping(scroll_top - look, vh + 2.0 * look)?;
        // What is strictly on screen must survive any trim. When the viewport
        // sits in a gap there is nothing to protect, so fall back to the
        // padded window.
        let vis = self.visible(scroll_top, vh).unwrap_or(padded);

        let max = budget.max_items.max(1);
        let Window {
            mut first,
            mut last,
        } = padded;
        while last - first + 1 > max {
            if first < vis.first {
                first += 1;
            } else if last > vis.last {
                last -= 1;
            } else {
                // Everything left is visible; the reader wins over the budget.
                break;
            }
        }
        Some(Window { first, last })
    }

    /// Index of the item occupying the most of the viewport.
    ///
    /// # Why not just "the item at the top edge"?
    ///
    /// "Which item's span contains the top pixel" is a different question, and
    /// the wrong one for a position indicator. Shrinking every item (zooming
    /// out) slides more of the *previous* item down into the top of the
    /// viewport, so the top-edge answer keeps changing even though the reader
    /// never moved and the content under their eyes is identical.
    ///
    /// Area-of-viewport degrades gracefully at both extremes: when one item
    /// fills the screen it trivially wins, and when several are visible the one
    /// you see most of wins. After a jump that aligns item `i` with the top of
    /// the viewport, `i` covers at least as much as anything below it, so a
    /// jump still reports the item it jumped to.
    ///
    /// Ties go to the lower index. Falls back to [`index_at`](Self::index_at)
    /// when the viewport has no extent.
    pub fn dominant(&self, scroll_top: f64, viewport: f64) -> usize {
        if self.is_empty() {
            return 0;
        }
        if viewport <= 0.0 {
            return self.index_at(scroll_top);
        }
        let Some(win) = self.visible(scroll_top, viewport) else {
            return self.index_at(scroll_top);
        };
        let bottom = scroll_top + viewport;
        let mut best = win.first;
        let mut best_cover = -1.0;
        for i in win.iter() {
            let top = self.starts[i];
            let cover = (top + self.size(i)).min(bottom) - top.max(scroll_top);
            if cover > best_cover {
                best_cover = cover;
                best = i;
            }
        }
        best
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Three items, sizes 100 / 200 / 100, gap 24 => starts 0 / 124 / 348.
    fn fixture() -> Strip {
        Strip::new([100.0, 200.0, 100.0], 24.0)
    }

    #[test]
    fn offsets_sizes_and_total() {
        let s = fixture();
        assert_eq!(s.len(), 3);
        assert_eq!(s.offset(0), 0.0);
        assert_eq!(s.offset(1), 124.0);
        assert_eq!(s.offset(2), 348.0);
        assert_eq!(s.size(0), 100.0);
        assert_eq!(s.size(1), 200.0);
        assert_eq!(s.size(2), 100.0);
        // No trailing gap.
        assert_eq!(s.total(), 448.0);
        // Past the end reads as the total, for trailing spacers.
        assert_eq!(s.offset(3), 448.0);
        assert_eq!(s.offset(99), 448.0);
        assert_eq!(s.size(3), 0.0);
    }

    #[test]
    fn empty_strip_is_inert() {
        let s = Strip::new([], 24.0);
        assert!(s.is_empty());
        assert_eq!(s.len(), 0);
        assert_eq!(s.total(), 0.0);
        assert_eq!(s.offset(0), 0.0);
        assert_eq!(s.index_at(500.0), 0);
        assert_eq!(s.dominant(0.0, 100.0), 0);
        assert_eq!(s.overlapping(0.0, 100.0), None);
        assert_eq!(s.window(0.0, 100.0, Budget::default()), None);
    }

    #[test]
    fn uniform_matches_explicit() {
        let a = Strip::uniform(3, 100.0, 24.0);
        let b = Strip::new([100.0, 100.0, 100.0], 24.0);
        assert_eq!(a, b);
        assert_eq!(a.total(), 348.0);
    }

    #[test]
    fn index_at_resolves_gaps_and_ends() {
        let s = fixture();
        assert_eq!(s.index_at(-10.0), 0);
        assert_eq!(s.index_at(0.0), 0);
        assert_eq!(s.index_at(99.0), 0);
        // An item ending exactly at `pos` has scrolled out: the next one leads.
        assert_eq!(s.index_at(100.0), 1);
        // 100..124 is the gap after item 0 — the item BELOW it now leads, so
        // this agrees with `overlapping`, which uses the same strict top edge.
        assert_eq!(s.index_at(110.0), 1);
        assert_eq!(s.index_at(124.0), 1);
        assert_eq!(s.index_at(347.0), 2);
        assert_eq!(s.index_at(348.0), 2);
        // Past the end clamps to the last item.
        assert_eq!(s.index_at(10_000.0), 2);
    }

    /// `index_at` and `overlapping` must agree about who leads the viewport,
    /// including at exact boundaries and inside gaps.
    #[test]
    fn index_at_agrees_with_overlapping() {
        let s = Strip::new([100.0, 200.0, 100.0], 24.0);
        let mut pos = 0.0;
        while pos < s.total() {
            if let Some(w) = s.overlapping(pos, 10.0) {
                assert_eq!(s.index_at(pos), w.first, "disagreement at pos={pos}");
            }
            pos += 0.5;
        }
    }

    #[test]
    fn overlapping_edges() {
        let s = fixture();
        assert_eq!(s.overlapping(0.0, 100.0).unwrap(), Window { first: 0, last: 0 });
        // An item ending exactly at the top edge has scrolled out.
        assert_eq!(s.overlapping(100.0, 100.0).unwrap(), Window { first: 1, last: 1 });
        assert_eq!(s.overlapping(0.0, 150.0).unwrap(), Window { first: 0, last: 1 });
        assert_eq!(s.overlapping(0.0, 10_000.0).unwrap(), Window { first: 0, last: 2 });
        // A viewport parked wholly inside the 100..124 gap sees nothing.
        assert_eq!(s.overlapping(105.0, 10.0), None);
        // Past the end.
        assert_eq!(s.overlapping(1_000.0, 100.0), None);
    }

    #[test]
    fn window_keeps_every_visible_item() {
        let s = Strip::uniform(40, 300.0, 24.0);
        let budget = Budget { look_frac: 1.0, max_items: 3 };
        // Sweep the whole scrollable range; the invariant must never break.
        let mut top = 0.0;
        while top < s.total() {
            let vh = 900.0;
            if let Some(vis) = s.visible(top, vh) {
                let win = s.window(top, vh, budget).expect("non-empty");
                assert!(
                    win.first <= vis.first && win.last >= vis.last,
                    "window {win:?} dropped a visible item {vis:?} at top={top}"
                );
            }
            top += 37.0;
        }
    }

    #[test]
    fn window_honours_max_items_when_it_can() {
        let s = Strip::uniform(40, 100.0, 24.0);
        // Viewport shows ~2 items; read-ahead would pull in many more.
        let win = s.window(1_000.0, 200.0, Budget { look_frac: 5.0, max_items: 4 }).unwrap();
        assert_eq!(win.len(), 4);
    }

    #[test]
    fn window_exceeds_budget_only_for_visible_items() {
        // Ten short items are all on screen at once, with a budget of 1.
        let s = Strip::uniform(10, 50.0, 0.0);
        let win = s.window(0.0, 500.0, Budget { look_frac: 0.0, max_items: 1 }).unwrap();
        let vis = s.visible(0.0, 500.0).unwrap();
        assert_eq!(win, vis, "visibility must win over the ceiling");
    }

    #[test]
    fn window_max_items_zero_behaves_as_one() {
        let s = Strip::uniform(10, 1_000.0, 24.0);
        let win = s.window(0.0, 100.0, Budget { look_frac: 0.0, max_items: 0 }).unwrap();
        assert_eq!(win.len(), 1);
    }

    #[test]
    fn window_trims_furthest_first_and_keeps_the_item_below() {
        // Items are 100 tall, gap 0. Viewport 100 tall parked exactly on item 5.
        let s = Strip::uniform(20, 100.0, 0.0);
        let win = s.window(500.0, 100.0, Budget { look_frac: 2.0, max_items: 3 }).unwrap();
        // Visible is item 5; with 3 slots we keep 5 and prefer below => 5,6,7.
        assert!(win.contains(5));
        assert_eq!(win.len(), 3);
        assert_eq!(win.first, 5, "should evict above before below");
    }

    #[test]
    fn window_past_end_of_document_is_none() {
        let s = Strip::uniform(3, 100.0, 24.0);
        assert_eq!(s.window(100_000.0, 900.0, Budget::default()), None);
    }

    #[test]
    fn dominant_picks_the_item_you_see_most_of() {
        let s = Strip::uniform(10, 100.0, 0.0);
        // Viewport 0..100 => item 0 fully covered.
        assert_eq!(s.dominant(0.0, 100.0), 0);
        // Viewport 90..190 => 10px of item 0, 90px of item 1.
        assert_eq!(s.dominant(90.0, 100.0), 1);
        // Viewport 40..140 => 60px of item 0, 40px of item 1.
        assert_eq!(s.dominant(40.0, 100.0), 0);
    }

    #[test]
    fn dominant_ties_go_to_the_lower_index() {
        let s = Strip::uniform(10, 100.0, 0.0);
        // Exactly 50/50 between items 0 and 1.
        assert_eq!(s.dominant(50.0, 100.0), 0);
    }

    #[test]
    fn dominant_after_a_jump_reports_the_jumped_to_item() {
        let s = Strip::uniform(30, 300.0, 24.0);
        for i in 0..30 {
            let top = s.offset(i);
            assert_eq!(s.dominant(top, 900.0), i, "jump to {i} should report {i}");
        }
    }

    #[test]
    fn dominant_without_a_viewport_falls_back_to_the_top_edge() {
        let s = fixture();
        assert_eq!(s.dominant(130.0, 0.0), 1);
    }

    #[test]
    fn window_iteration_is_inclusive() {
        let w = Window { first: 2, last: 5 };
        assert_eq!(w.len(), 4);
        assert!(!w.is_empty());
        assert!(w.contains(2) && w.contains(5) && !w.contains(6));
        assert_eq!(w.iter().collect::<Vec<_>>(), alloc::vec![2, 3, 4, 5]);
        assert_eq!(w.into_iter().count(), 4);
    }

    #[test]
    fn offsets_are_consistent_with_sizes_for_ragged_input() {
        let sizes = [13.0, 400.0, 7.5, 999.25, 1.0];
        let s = Strip::new(sizes, 11.0);
        let mut expect = 0.0;
        for (i, &sz) in sizes.iter().enumerate() {
            assert!((s.offset(i) - expect).abs() < 1e-9, "offset {i}");
            assert!((s.size(i) - sz).abs() < 1e-9, "size {i}");
            expect += sz + 11.0;
        }
        assert!((s.total() - (expect - 11.0)).abs() < 1e-9);
    }
}
