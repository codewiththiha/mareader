//! Fenwick-tree-backed [`Strip`]: `O(log n)` single-item size updates and
//! `O(log n)` offset lookups via binary lifting.
//!
//! When item sizes change faster than [`Strip::set_size`](super::Strip::set_size)
//! can amortise the `O(n)` rebuild — live chat, an accordion being repeatedly
//! expanded/collapsed, an image gallery whose heights are reported by the
//! browser as each raster finishes decoding — the `O(n)` rebuild shows up as a
//! dropped frame. Switch to [`FenwickStrip`] in those cases.
//!
//! # Trade-off
//!
//! | Operation      | [`Strip`] | [`FenwickStrip`] |
//! | -------------- | --------- | ----------------- |
//! | offset lookup  | `O(1)`    | `O(log n)`        |
//! | size update    | `O(n)`    | `O(log n)`        |
//! | memory         | `8(n+1)`  | `8(n+1)`          |
//!
//! The Fenwick tree stores per-item `size + gap` in its internal nodes; the
//! `gap` term is folded into the prefix-sum so callers see the same `offset`
//! they would from [`Strip`](super::Strip).

use alloc::vec::Vec;

use crate::{from_sub, to_sub, Budget, Window};

/// A column of variably-sized items separated by a fixed gap, backed by a
/// Fenwick (Binary Indexed) Tree.
///
/// Updates are `O(log n)`; offset lookups are `O(log n)` via binary lifting
/// instead of the obvious `O(1)` of [`Strip`](super::Strip)'s prefix-sum —
/// the trade is worth it for highly-dynamic lists.
#[derive(Debug, Clone, Default)]
pub struct FenwickStrip {
    /// Fenwick tree of "size + gap" per item. Index 0 is unused (Fenwick trees
    /// are 1-indexed), so the buffer has `n + 1` slots.
    tree: Vec<i64>,
    /// Number of items.
    n: usize,
    /// Gap between items, in sub-pixels.
    gap: i64,
    /// Cached total extent, in sub-pixels. Maintained incrementally.
    total: i64,
    /// Highest power of two `<= n`, used by binary lifting.
    top_bit: i64,
}

impl FenwickStrip {
    /// Build a Fenwick-backed strip from explicit item sizes.
    pub fn new<I>(sizes: I, gap: f64) -> Self
    where
        I: IntoIterator<Item = f64>,
    {
        let gap_sub = to_sub(gap);
        let sizes_sub: Vec<i64> = sizes.into_iter().map(to_sub).collect();
        let n = sizes_sub.len();
        // Linear-time Fenwick build: set `tree[i] = a[i-1] + gap`, then
        // propagate to `i + lowbit(i)`.
        let mut tree = vec![0i64; n + 1];
        for (i, &sz) in sizes_sub.iter().enumerate() {
            let idx = i + 1;
            tree[idx] = tree[idx].saturating_add(sz).saturating_add(gap_sub);
            let lowbit = (idx as i64) & -(idx as i64);
            let parent = (idx as i64) + lowbit;
            if (parent as usize) <= n {
                tree[parent as usize] = tree[parent as usize].saturating_add(tree[idx]);
            }
        }
        // Total = prefix_sum(n) - gap (drop the trailing gap).
        let total = prefix_sum(&tree, n).saturating_sub(gap_sub);
        let top_bit = highest_power_of_two_le(n as i64);
        Self {
            tree,
            n,
            gap: gap_sub,
            total,
            top_bit,
        }
    }

    /// Build a strip of `count` uniform items.
    pub fn uniform(count: usize, size: f64, gap: f64) -> Self {
        Self::new(core::iter::repeat_n(size, count), gap)
    }

    /// Number of items.
    #[inline]
    pub fn len(&self) -> usize {
        self.n
    }

    /// Whether the strip is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// The gap between adjacent items, in CSS pixels.
    #[inline]
    pub fn gap(&self) -> f64 {
        from_sub(self.gap)
    }

    /// Total extent of the column: every item plus the gaps between them, with
    /// no trailing gap. `0.0` when empty.
    #[inline]
    pub fn total(&self) -> f64 {
        from_sub(self.total)
    }

    /// Offset of the start of item `index`. `O(log n)` via Fenwick prefix-sum.
    ///
    /// Returns `0.0` for `index == 0`, the total extent for `index >= n`.
    pub fn offset(&self, index: usize) -> f64 {
        if index == 0 {
            return 0.0;
        }
        if index >= self.n {
            return self.total();
        }
        // prefix_sum(index) is sum(size[0..index] + gap * index). The start of
        // `index` is exactly that: every item before `index` plus the gap
        // after each. (No trailing gap on the start position itself.)
        from_sub(prefix_sum(&self.tree, index))
    }

    /// Size of item `index`, in CSS pixels. `0.0` if out of range.
    pub fn size(&self, index: usize) -> f64 {
        if index >= self.n {
            return 0.0;
        }
        let start = prefix_sum(&self.tree, index);
        // End of item `index` is `start + size(index)`; the prefix-sum at
        // `index + 1` is `start + size(index) + gap`.
        let end_plus_gap = if index + 1 > self.n {
            self.total.saturating_add(self.gap)
        } else {
            prefix_sum(&self.tree, index + 1)
        };
        let size_plus_gap = end_plus_gap.saturating_sub(start);
        let size = size_plus_gap.saturating_sub(self.gap).max(0);
        from_sub(size)
    }

    /// Index of the item whose span contains `pos`.
    ///
    /// Same boundary semantics as [`Strip::index_at`](super::Strip::index_at):
    /// `pos` is the leading edge, an item ending exactly at `pos` has
    /// scrolled out, and positions inside a gap resolve to the item below.
    ///
    /// `O(log n)` via **binary lifting** on the Fenwick tree — no separate
    /// prefix-sum array is consulted.
    pub fn index_at(&self, pos: f64) -> usize {
        if self.n == 0 || pos <= 0.0 {
            return 0;
        }
        let p = to_sub(pos);
        // Lift to the largest `i` with prefix_sum(i) <= p. That `i` is the
        // candidate item index (prefix_sum(i) is the start of item `i`).
        let idx = self.lift(p).min(self.n.saturating_sub(1));
        // Apply the same boundary rule as `Strip::index_at`.
        let start = prefix_sum(&self.tree, idx);
        let end = start.saturating_add(to_sub(self.size(idx)));
        if p >= end && idx + 1 < self.n {
            idx + 1
        } else {
            idx
        }
    }

    /// Inclusive range of items overlapping `[top, top + extent)`.
    pub fn overlapping(&self, top: f64, extent: f64) -> Option<Window> {
        if self.n == 0 {
            return None;
        }
        let top_sub = to_sub(top);
        let bottom_sub = to_sub(top + extent.max(0.0));

        let mut first = self.index_at(top);
        let start_first = prefix_sum(&self.tree, first);
        let end_first = start_first.saturating_add(to_sub(self.size(first)));
        if end_first <= top_sub {
            first += 1;
        }
        if first >= self.n || self.offset(first) > from_sub(bottom_sub) {
            return None;
        }
        let last = self.lift(bottom_sub).min(self.n - 1);
        (last >= first).then_some(Window { first, last })
    }

    /// Inclusive range of items that are at least partly on screen.
    #[inline]
    pub fn visible(&self, scroll_top: f64, viewport: f64) -> Option<Window> {
        self.overlapping(scroll_top, viewport)
    }

    /// Inclusive range of items to keep mounted. See
    /// [`Strip::window`](super::Strip::window) for the budget semantics.
    pub fn window(
        &self,
        scroll_top: f64,
        viewport: f64,
        budget: Budget,
    ) -> Option<Window> {
        if self.is_empty() {
            return None;
        }
        let vh = viewport.max(0.0);
        let look = budget.look_frac.max(0.0) * vh;

        let padded = self.overlapping(scroll_top - look, vh + 2.0 * look)?;
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
                break;
            }
        }
        Some(Window { first, last })
    }

    /// Change the size of a single item in `O(log n)`. Returns the signed
    /// delta in CSS pixels.
    ///
    /// After this call, [`offset`](Self::offset), [`size`](Self::size), and
    /// [`total`](Self::total) all reflect the new value.
    pub fn set_size(&mut self, index: usize, new_size: f64) -> f64 {
        if index >= self.n {
            return 0.0;
        }
        let old = to_sub(self.size(index));
        let new = to_sub(new_size);
        if old == new {
            return 0.0;
        }
        let delta = new.saturating_sub(old);
        // Fenwick point update: add `delta` to every covering node.
        let mut i = (index + 1) as i64;
        while (i as usize) <= self.n {
            self.tree[i as usize] = self.tree[i as usize].saturating_add(delta);
            i += i & (-i);
        }
        self.total = self.total.saturating_add(delta);
        from_sub(delta)
    }

    /// Binary-lift to the largest index `i` with `prefix_sum(i) <= value`.
    /// Returns a value in `0..=n`.
    fn lift(&self, value: i64) -> usize {
        let mut idx: i64 = 0;
        let mut sum: i64 = 0;
        let mut bit = self.top_bit;
        while bit > 0 {
            let next = idx + bit;
            if (next as usize) <= self.n
                && sum.saturating_add(self.tree[next as usize]) <= value
            {
                sum = sum.saturating_add(self.tree[next as usize]);
                idx = next;
            }
            bit >>= 1;
        }
        idx as usize
    }
}

impl PartialEq for FenwickStrip {
    fn eq(&self, other: &Self) -> bool {
        // Two FenwickStrips are equal iff they have the same n, gap, total, and
        // tree (the tree is deterministic given the inputs).
        self.n == other.n
            && self.gap == other.gap
            && self.total == other.total
            && self.tree == other.tree
    }
}

/// Compute the prefix sum `sum_{i=1..=k}(tree[i])` in `O(log n)`.
fn prefix_sum(tree: &[i64], k: usize) -> i64 {
    let mut sum: i64 = 0;
    let mut i = k as i64;
    while i > 0 {
        sum = sum.saturating_add(tree[i as usize]);
        i -= i & (-i);
    }
    sum
}

/// Largest power of two `<= x`. For `x = 0`, returns 0. For `x >= 1`, returns
/// `2 ** floor(log2 x)`.
fn highest_power_of_two_le(x: i64) -> i64 {
    if x <= 0 {
        return 0;
    }
    let mut p = 1i64;
    while p * 2 <= x {
        p *= 2;
    }
    p
}
