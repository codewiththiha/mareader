//! The [`Strip`] windowing core.

use alloc::vec::Vec;

use crate::units::{from_sub, to_sub};
use crate::window::{Budget, Window};

// The math lives ONCE, in the impl and the shared free functions.
use super::StripBackend;

/// A column of variably-sized items separated by a fixed gap.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Strip {
    /// `starts[i]` is item `i`'s offset in sub-pixels; `starts[len]` the total.
    starts: Vec<i64>,
    /// Gap between adjacent items, in sub-pixels.
    gap: i64,
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
        let gap_sub = to_sub(gap);
        let mut acc: i64 = 0;
        for size in iter {
            starts.push(acc);
            acc = acc.saturating_add(to_sub(size)).saturating_add(gap_sub);
        }
        if !starts.is_empty() {
            // Total extent excludes the gap after the final item.
            starts.push(acc.saturating_sub(gap_sub));
        }
        Self {
            starts,
            gap: gap_sub,
        }
    }

    /// Build a strip of `count` same-sized items.
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

    /// The gap between adjacent items, in CSS pixels.
    #[inline]
    pub fn gap(&self) -> f64 {
        from_sub(self.gap)
    }

    /// Offset of the start of item `index`.
    #[inline]
    pub fn offset(&self, index: usize) -> f64 {
        StripBackend::offset(self, index)
    }

    /// Size of item `index`, or `0.0` if out of range.
    #[inline]
    pub fn size(&self, index: usize) -> f64 {
        StripBackend::size(self, index)
    }

    /// Total extent of the column, no trailing gap; 0.0 when empty.
    #[inline]
    pub fn total(&self) -> f64 {
        StripBackend::total(self)
    }

    /// Average item extent — resolves [`crate::Overscan::Items`] budgets.
    pub fn mean_size(&self) -> f64 {
        StripBackend::mean_size(self)
    }

    /// Index of the item whose span contains `pos`, by leading edge.
    pub fn index_at(&self, pos: f64) -> usize {
        StripBackend::index_at(self, pos)
    }

    /// [`Strip::index_at`] with a hint, the previous frame's result.
    pub fn index_at_hinted(&self, pos: f64, hint: &mut usize) -> usize {
        StripBackend::index_at_hinted(self, pos, hint)
    }

    /// Inclusive range of items overlapping `[top, top + extent)`.
    pub fn overlapping(&self, top: f64, extent: f64) -> Option<Window> {
        super::overlapping(self, top, extent)
    }

    /// Inclusive range of items at least partly on screen.
    #[inline]
    pub fn visible(&self, scroll_top: f64, viewport: f64) -> Option<Window> {
        super::visible(self, scroll_top, viewport)
    }

    /// Inclusive range of items to keep mounted, trimmed to the budget.
    pub fn window(&self, scroll_top: f64, viewport: f64, budget: Budget) -> Option<Window> {
        crate::backend::window(self, scroll_top, viewport, budget)
    }

    /// [`Strip::window`] with a hinted overlap search.
    pub fn window_hinted(
        &self,
        scroll_top: f64,
        viewport: f64,
        budget: Budget,
        hint: &mut usize,
    ) -> Option<Window> {
        StripBackend::window_hinted(self, scroll_top, viewport, budget, hint)
    }

    /// Index of the item occupying most of the viewport.
    pub fn dominant(&self, scroll_top: f64, viewport: f64) -> usize {
        super::dominant(self, scroll_top, viewport)
    }

    /// Change one item's size in `O(n)`, returning the delta.
    pub fn set_size(&mut self, index: usize, new_size: f64) -> f64 {
        StripBackend::set_size(self, index, new_size)
    }
}

impl super::StripBackend for Strip {
    #[inline]
    fn len(&self) -> usize {
        self.len()
    }

    fn gap_sub(&self) -> i64 {
        self.gap
    }

    fn offset_sub(&self, index: usize) -> i64 {
        match self.starts.get(index) {
            Some(&v) => v,
            None => self.total_sub(),
        }
    }

    fn size_sub(&self, index: usize) -> i64 {
        let len = self.len();
        if index >= len {
            return 0;
        }
        let end = if index + 1 == len {
            self.starts[len]
        } else {
            self.starts[index + 1].saturating_sub(self.gap)
        };
        end.saturating_sub(self.starts[index]).max(0)
    }

    fn total_sub(&self) -> i64 {
        self.starts.last().copied().unwrap_or(0)
    }

    fn index_at_sub(&self, p: i64) -> usize {
        let len = self.len();
        if len == 0 || p <= 0 {
            return 0;
        }
        let idx = self.starts[..len]
            .partition_point(|&s| s <= p)
            .saturating_sub(1);
        if self.starts[idx].saturating_add(self.size_sub(idx)) <= p && idx + 1 < len {
            idx + 1
        } else {
            idx
        }
    }

    /// The hinted leading-edge search: neighbour, then a galloping bracket.
    fn index_at_hinted(&self, pos: f64, hint: &mut usize) -> usize {
        let len = self.len();
        if len == 0 || pos <= 0.0 {
            *hint = 0;
            return 0;
        }
        let p = to_sub(pos);

        // Clamp hint to a valid item index.
        if *hint >= len {
            *hint = len - 1;
        }
        let h = *hint;

        // 1) O(1): still inside the same item?
        let h_start = self.starts[h];
        let h_end = h_start.saturating_add(self.size_sub(h));
        if p >= h_start && p < h_end {
            return h;
        }
        // 2) O(1): did we step into the next / previous item?
        if h + 1 < len {
            let n_start = self.starts[h + 1];
            let n_end = n_start.saturating_add(self.size_sub(h + 1));
            if p >= n_start && p < n_end {
                *hint = h + 1;
                return h + 1;
            }
        }
        if h > 0 {
            let p_start = self.starts[h - 1];
            let p_end = p_start.saturating_add(self.size_sub(h - 1));
            if p >= p_start && p < p_end {
                *hint = h - 1;
                return h - 1;
            }
        }

        // 3) Galloping search: bracket the answer, then binary search inside.
        let target = if p < h_start {
            // Jumped UPWARDS: the largest i <= h with starts[i] <= p.
            let mut lo = 0usize;
            let mut step = 1usize;
            // Probe 1, 2, 4, ... below h to bracket the answer.
            let mut probe = h;
            loop {
                let next = probe.saturating_sub(step);
                if next == probe {
                    break;
                }
                if self.starts[next] <= p {
                    probe = next;
                    // We found a lower bound; binary search [probe, h].
                    lo = probe;
                    break;
                }
                probe = next;
                if probe == 0 {
                    break;
                }
                step <<= 1;
            }
            // Binary search in [lo, h] for the largest index whose start <= p.
            self.starts[lo..=h]
                .partition_point(|&s| s <= p)
                .saturating_sub(1)
                + lo
        } else {
            // Jumped DOWNWARDS: the largest i >= h with starts[i] <= p.
            let mut hi = h;
            let mut step = 1usize;
            let mut probe = h;
            loop {
                let next = probe.saturating_add(step).min(len - 1);
                if next == probe {
                    break;
                }
                if self.starts[next] > p {
                    hi = next;
                    break;
                }
                probe = next;
                if probe == len - 1 {
                    // Reached the end; the answer is len-1 (or its
                    // neighbour, resolved below).
                    hi = len - 1;
                    break;
                }
                step <<= 1;
            }
            // Binary search in [h, hi].
            h + self.starts[h..=hi]
                .partition_point(|&s| s <= p)
                .saturating_sub(1)
        };

        // Same boundary rule as `index_at`: past the candidate, next leads.
        let idx =
            if self.starts[target].saturating_add(self.size_sub(target)) <= p && target + 1 < len {
                target + 1
            } else {
                target
            };
        *hint = idx;
        idx
    }

    fn set_size_sub(&mut self, index: usize, new_sub: i64) -> i64 {
        let len = self.len();
        if index >= len {
            return 0;
        }
        let old_sub = self.size_sub(index);
        if new_sub == old_sub {
            return 0;
        }
        let delta = new_sub.saturating_sub(old_sub);
        // An O(n) suffix walk per measured page; a Fenwick tree beyond
        // tens of thousands.
        for i in (index + 1)..=len {
            self.starts[i] = self.starts[i].saturating_add(delta);
        }
        delta
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::units::{SUBPIXEL_FACTOR, from_sub, to_sub};

    /// Tolerance for comparing an i64-derived f64 with a computed one.
    const APPROX_TOL: f64 = 1e-3;

    // Sizes 100 / 200 / 100, gap 24: starts 0 / 124 / 348.
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
        // 100..124 is the gap after item 0: the item BELOW leads.
        assert_eq!(s.index_at(110.0), 1);
        assert_eq!(s.index_at(124.0), 1);
        assert_eq!(s.index_at(347.0), 2);
        assert_eq!(s.index_at(348.0), 2);
        // Past the end clamps to the last item.
        assert_eq!(s.index_at(10_000.0), 2);
    }

    /// `index_at` and `overlapping` agree about who leads.
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
        assert_eq!(
            s.overlapping(0.0, 100.0).unwrap(),
            Window { first: 0, last: 0 }
        );
        // An item ending exactly at the top edge has scrolled out.
        assert_eq!(
            s.overlapping(100.0, 100.0).unwrap(),
            Window { first: 1, last: 1 }
        );
        assert_eq!(
            s.overlapping(0.0, 150.0).unwrap(),
            Window { first: 0, last: 1 }
        );
        assert_eq!(
            s.overlapping(0.0, 10_000.0).unwrap(),
            Window { first: 0, last: 2 }
        );
        // A viewport parked wholly inside the 100..124 gap sees nothing.
        assert_eq!(s.overlapping(105.0, 10.0), None);
        // Past the end.
        assert_eq!(s.overlapping(1_000.0, 100.0), None);
    }

    #[test]
    fn window_keeps_every_visible_item() {
        let s = Strip::uniform(40, 300.0, 24.0);
        let budget = Budget::screenfuls(1.0, 3);
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
        let win = s
            .window(1_000.0, 200.0, Budget::screenfuls(5.0, 4))
            .unwrap();
        assert_eq!(win.len(), 4);

        // `max_items: 0` behaves as `1`: a zero budget would blank the list.
        let s0 = Strip::uniform(10, 1_000.0, 24.0);
        let win0 = s0.window(0.0, 100.0, Budget::screenfuls(0.0, 0)).unwrap();
        assert_eq!(win0.len(), 1);
    }

    #[test]
    fn window_exceeds_budget_only_for_visible_items() {
        // Ten short items are all on screen at once, with a budget of 1.
        let s = Strip::uniform(10, 50.0, 0.0);
        let win = s.window(0.0, 500.0, Budget::screenfuls(0.0, 1)).unwrap();
        let vis = s.visible(0.0, 500.0).unwrap();
        assert_eq!(win, vis, "visibility must win over the ceiling");
    }

    #[test]
    fn window_trims_furthest_first_and_keeps_the_item_below() {
        // Items are 100 tall, gap 0; viewport parked exactly on item 5.
        let s = Strip::uniform(20, 100.0, 0.0);
        let win = s.window(500.0, 100.0, Budget::screenfuls(2.0, 3)).unwrap();
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
        // Exactly 50/50 between items 0 and 1: ties go to the lower index.
        assert_eq!(s.dominant(50.0, 100.0), 0);

        // A zero-extent viewport falls back to the top edge.
        let s2 = Strip::new([100.0, 200.0, 100.0], 24.0);
        assert_eq!(s2.dominant(130.0, 0.0), s2.index_at(130.0));
        assert_eq!(s2.dominant(130.0, 0.0), 1);
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
    fn window_iteration_is_inclusive() {
        let w = Window { first: 2, last: 5 };
        assert_eq!(w.len(), 4);
        assert!(w.contains(2) && w.contains(5) && !w.contains(6));
        assert_eq!(w.iter().collect::<Vec<_>>(), alloc::vec![2, 3, 4, 5]);
        assert_eq!(w.into_iter().count(), 4);
    }

    #[test]
    fn offsets_are_consistent_with_sizes_for_ragged_input() {
        // Power-of-2 denominators round-trip EXACTLY; others lose ~1.5e-5.
        let sizes = [
            13.0, 400.0, 7.5, 999.25, 1.0, 0.1, 0.333, 0.999, 123.456, 0.25, 0.0625,
        ];
        let s = Strip::new(sizes, 11.0);
        let mut expect = 0.0;
        for (i, &sz) in sizes.iter().enumerate() {
            assert!(
                (s.offset(i) - expect).abs() < APPROX_TOL,
                "offset {i}: {} vs {}",
                s.offset(i),
                expect
            );
            assert!(
                (s.size(i) - sz).abs() < APPROX_TOL,
                "size {i}: {} vs {}",
                s.size(i),
                sz
            );
            expect += sz + 11.0;
        }
        assert!((s.total() - (expect - 11.0)).abs() < APPROX_TOL);
    }

    /// The i64 sub-pixel precision trade-off, demonstrated.
    #[test]
    fn subpixel_precision_for_non_binary_fractions() {
        // Exact: denominators are powers of 2.
        for &x in &[0.0, 1.0, 0.5, 0.25, 0.125, 7.5, 999.25, 13.0, 1_234_567.0] {
            assert!(
                (from_sub(to_sub(x)) - x).abs() < 1e-12,
                "exact round-trip {x}"
            );
        }
        // Approximate: non-power-of-2 denominators lose up to one sub-pixel.
        let bound = 1.0 / (SUBPIXEL_FACTOR as f64);
        for &x in &[0.1, 0.333, 0.999, 123.456, 0.001, 0.789, 42.195] {
            let err = (from_sub(to_sub(x)) - x).abs();
            assert!(err < bound, "round-trip {x}: error {err} exceeds {bound}");
        }
        // NaN / inf / negative clamps to zero, never panics.
        assert_eq!(to_sub(f64::NAN), 0);
        assert_eq!(to_sub(f64::INFINITY), i64::MAX);
        assert_eq!(to_sub(-1.0), 0);
    }

    #[test]
    fn hinted_index_matches_unhinted_for_all_positions() {
        let s = Strip::uniform(50, 100.0, 24.0);
        let mut hint = 0usize;
        let mut pos = 0.0;
        while pos < s.total() {
            let a = s.index_at(pos);
            let b = s.index_at_hinted(pos, &mut hint);
            assert_eq!(
                a, b,
                "hinted disagrees with unhinted at pos={pos}: {a} vs {b}"
            );
            pos += 1.0;
        }
    }

    #[test]
    fn hinted_index_handles_large_jumps() {
        let s = Strip::uniform(1000, 100.0, 24.0);
        let mut hint = 0usize;
        // Jump to the middle.
        let mid = s.total() / 2.0;
        assert_eq!(s.index_at_hinted(mid, &mut hint), s.index_at(mid));
        // Jump to near the end.
        let end = s.total() - 50.0;
        assert_eq!(s.index_at_hinted(end, &mut hint), s.index_at(end));
        // Jump back to the start.
        assert_eq!(s.index_at_hinted(0.0, &mut hint), s.index_at(0.0));
    }

    #[test]
    fn hinted_overlapping_matches_unhinted() {
        let s = Strip::new([100.0, 200.0, 150.0, 100.0, 200.0], 24.0);
        let mut hint = 0usize;
        let mut top = 0.0;
        while top < s.total() {
            let a = s.overlapping(top, 200.0);
            let b = crate::backend::overlapping_hinted(&s, top, 200.0, &mut hint);
            assert_eq!(a, b, "hinted overlapping disagrees at top={top}");
            top += 23.0;
        }
    }

    #[test]
    fn set_size_updates_offsets_and_total() {
        let mut s = Strip::new([100.0, 200.0, 100.0], 24.0);
        let delta = s.set_size(1, 300.0);
        assert_eq!(delta, 100.0);
        assert_eq!(s.size(0), 100.0);
        assert_eq!(s.size(1), 300.0);
        assert_eq!(s.size(2), 100.0);
        assert_eq!(s.offset(0), 0.0);
        assert_eq!(s.offset(1), 124.0);
        assert_eq!(s.offset(2), 124.0 + 300.0 + 24.0);
        assert_eq!(s.total(), 100.0 + 24.0 + 300.0 + 24.0 + 100.0);

        // Out-of-range index is a no-op; so is an unchanged size.
        let mut s2 = Strip::new([100.0, 200.0], 24.0);
        assert_eq!(s2.set_size(5, 200.0), 0.0);
        assert_eq!(s2.set_size(0, 100.0), 0.0);
        assert_eq!(s2.size(0), 100.0);
        assert_eq!(s2.size(1), 200.0);
        assert_eq!(s2.total(), 100.0 + 24.0 + 200.0);
    }

    #[test]
    fn a_size_change_above_the_anchor_moves_the_item_by_the_delta() {
        // The correction itself is `crate::anchor::correct`; a strip owes a
        // delta and honest offsets.
        let mut s = Strip::uniform(20, 100.0, 0.0);
        let before = s.offset(10);
        assert_eq!(s.set_size(5, 150.0), 50.0);
        assert_eq!(s.offset(10), before + 50.0);
        // A change below the anchor leaves everything above it where it was.
        let before = s.offset(10);
        assert_eq!(s.set_size(15, 200.0), 100.0);
        assert_eq!(s.offset(10), before);
    }
}
