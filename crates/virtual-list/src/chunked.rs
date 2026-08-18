//! Chunked prefix-sum variant: `O(1)` offset lookup + `O(sqrt n)` per-item
//! size update.
//!
//! This is a middle ground between [`Strip`](super::Strip) and
//! [`FenwickStrip`](super::FenwickStrip):
//!
//! | Operation      | [`Strip`] | [`ChunkedStrip`] | [`FenwickStrip`] |
//! | -------------- | --------- | ----------------- | ----------------- |
//! | offset lookup  | `O(1)`    | `O(1)`            | `O(log n)`        |
//! | size update    | `O(n)`    | `O(sqrt n)`       | `O(log n)`        |
//! | memory         | `8(n+1)`  | `8(n+1)`          | `8(n+1)`          |
//!
//! The structure stores the prefix-sum array (for `O(1)` lookup) AND a small
//! per-chunk delta register. Updating item `i` walks only the items in its
//! chunk (`O(K)` items, `K` = chunk size) and then walks the chunk-offset
//! array (`O(N/K)` chunks) — choosing `K = sqrt(N)` minimises the worst case.
//!
//! For most UIs with `N ~= 1_000..1_000_000`, `K = 64..=1024` is the sweet
//! spot.

use alloc::vec::Vec;

use crate::{from_sub, to_sub, Budget, Window};

/// A column of variably-sized items separated by a fixed gap, stored as a
/// chunked prefix-sum array.
#[derive(Debug, Clone, Default)]
pub struct ChunkedStrip {
    /// Per-item prefix sum, captured at build time and rewritten in-chunk on
    /// updates. `starts[i]` is the start of item `i` (sub-pixels); `starts[n]`
    /// is the total extent. Always has `n + 1` entries.
    starts: Vec<i64>,
    /// `chunk_delta[c]` is the **cumulative** size-change accumulated by all
    /// items in chunks strictly before chunk `c`. With cumulative semantics,
    /// `offset(i) = starts[i] + chunk_delta[chunk_of(i)]` — a single lookup,
    /// `O(1)`. Updates add `delta` to every `chunk_delta[c]` for `c >
    /// chunk_of(index)` — `O(num_chunks)` per update.
    chunk_delta: Vec<i64>,
    /// Items per chunk (the `K` parameter).
    chunk_size: usize,
    /// Number of items.
    n: usize,
    /// Gap between items, in sub-pixels.
    gap: i64,
}

impl PartialEq for ChunkedStrip {
    fn eq(&self, other: &Self) -> bool {
        self.starts == other.starts
            && self.chunk_delta == other.chunk_delta
            && self.chunk_size == other.chunk_size
            && self.n == other.n
            && self.gap == other.gap
    }
}

impl ChunkedStrip {
    /// Build a chunked strip from explicit item sizes. The chunk size is
    /// picked automatically as `max(16, floor(sqrt(n)))`, which is the
    /// asymptotic optimum for square-root decomposition.
    pub fn new<I>(sizes: I, gap: f64) -> Self
    where
        I: IntoIterator<Item = f64>,
    {
        let sizes_vec: Vec<f64> = sizes.into_iter().collect();
        let n_hint = sizes_vec.len();
        let chunk_size = n_hint.isqrt().max(16);
        Self::new_with_chunk(sizes_vec, gap, chunk_size)
    }

    /// Build a chunked strip with an explicit chunk size. Useful for
    /// benchmarking or when the caller knows the typical update pattern.
    /// `chunk_size` is clamped to `[1, max(n, 1)]`.
    pub fn new_with_chunk<I>(sizes: I, gap: f64, chunk_size: usize) -> Self
    where
        I: IntoIterator<Item = f64>,
    {
        let gap_sub = to_sub(gap);
        let iter = sizes.into_iter();
        let (n_hint, _) = iter.size_hint();
        let mut starts: Vec<i64> = Vec::with_capacity(n_hint + 1);
        let mut acc: i64 = 0;
        for size in iter {
            starts.push(acc);
            acc = acc.saturating_add(to_sub(size)).saturating_add(gap_sub);
        }
        let n = starts.len();
        if !starts.is_empty() {
            starts.push(acc.saturating_sub(gap_sub));
        }
        let chunk_size = chunk_size.clamp(1, n.max(1));
        // Allocate enough chunks to also cover the trailing `starts[n]` slot
        // (the total extent entry), which lives in chunk `n / chunk_size`.
        let num_chunks = (n + 1).div_ceil(chunk_size).max(1);
        let chunk_delta = vec![0i64; num_chunks];
        Self {
            starts,
            chunk_delta,
            chunk_size,
            n,
            gap: gap_sub,
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

    /// The chunk size in use.
    #[inline]
    pub fn chunk_size(&self) -> usize {
        self.chunk_size
    }

    /// The gap between adjacent items, in CSS pixels.
    #[inline]
    pub fn gap(&self) -> f64 {
        from_sub(self.gap)
    }

    /// Total extent of the column. `0.0` when empty.
    #[inline]
    pub fn total(&self) -> f64 {
        if self.n == 0 {
            return 0.0;
        }
        // `starts[n]` (original total) plus the cumulative delta for the
        // chunk that owns it. `chunk_of(n)` is the last chunk in our
        // allocation.
        let last_chunk = self.chunk_of(self.n);
        from_sub(self.starts[self.n].saturating_add(self.chunk_delta[last_chunk]))
    }

    /// Offset of the start of item `index`. `0.0` for an empty strip or for
    /// `index == 0`; the total for `index >= n`.
    ///
    /// `O(1)`: a single `chunk_delta[chunk_of(index)]` lookup plus the cached
    /// `starts[index]`.
    #[inline]
    pub fn offset(&self, index: usize) -> f64 {
        if index == 0 {
            return 0.0;
        }
        if index >= self.n {
            return self.total();
        }
        let chunk = self.chunk_of(index);
        from_sub(self.starts[index].saturating_add(self.chunk_delta[chunk]))
    }

    /// Size of item `index`, or `0.0` if out of range.
    pub fn size(&self, index: usize) -> f64 {
        if index >= self.n {
            return 0.0;
        }
        let start = to_sub(self.offset(index));
        let end = if index + 1 == self.n {
            to_sub(self.total())
        } else {
            to_sub(self.offset(index + 1)).saturating_sub(self.gap)
        };
        let s = end.saturating_sub(start).max(0);
        from_sub(s)
    }

    /// Index of the item whose span contains `pos`. Same boundary semantics
    /// as [`Strip::index_at`](super::Strip::index_at).
    pub fn index_at(&self, pos: f64) -> usize {
        if self.n == 0 || pos <= 0.0 {
            return 0;
        }
        let p = to_sub(pos);
        // Account for chunk deltas by computing the adjusted starts[i] in
        // `O(sqrt n)` worst case (binary search over items, each lookup paying
        // the chunk prefix). For long runs of identical updates this stays
        // cache-local because `chunk_delta` is tiny.
        let mut lo = 0usize;
        let mut hi = self.n;
        while lo + 1 < hi {
            let mid = (lo + hi) / 2;
            let start = to_sub(self.offset(mid));
            if start <= p {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let idx = lo;
        // Apply the same boundary rule.
        let end = to_sub(self.offset(idx)).saturating_add(to_sub(self.size(idx)));
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
        let bottom = top + extent.max(0.0);
        let mut first = self.index_at(top);
        let start_first = to_sub(self.offset(first));
        let end_first = start_first.saturating_add(to_sub(self.size(first)));
        if end_first <= to_sub(top) {
            first += 1;
        }
        if first >= self.n || self.offset(first) > bottom {
            return None;
        }
        // Last item whose start is <= bottom.
        let mut lo = first;
        let mut hi = self.n;
        let bottom_sub = to_sub(bottom);
        while lo + 1 < hi {
            let mid = (lo + hi) / 2;
            if to_sub(self.offset(mid)) <= bottom_sub {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        let last = lo;
        (last >= first).then_some(Window { first, last })
    }

    /// Inclusive range of items that are at least partly on screen.
    #[inline]
    pub fn visible(&self, scroll_top: f64, viewport: f64) -> Option<Window> {
        self.overlapping(scroll_top, viewport)
    }

    /// Inclusive range of items to keep mounted. See
    /// [`Strip::window`](super::Strip::window).
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

    /// Change the size of a single item in `O(sqrt n)` time. Returns the
    /// signed delta in CSS pixels.
    ///
    /// The chunked layout stores `starts[i]` (the immutable prefix-sum
    /// captured at build time) plus a small `chunk_delta[c]` register that
    /// records the cumulative size change applied to any item inside chunks
    /// `[0, c)` (strictly before chunk `c`). `offset(i)` is then
    /// `starts[i] + chunk_delta[chunk_of(i)]` — `O(1)`.
    ///
    /// `set_size(index, new_size)` does:
    /// 1. Shift every `starts[i+1..end_of_chunk]` (within-chunk) by `delta`
    ///    so items in the same chunk AT OR AFTER `index` see the new offset.
    /// 2. Add `delta` to `chunk_delta[c]` for every chunk `c > chunk_of(index)`
    ///    so items in later chunks see the new offset too.
    ///
    /// Step 1 is `O(chunk_size)`; step 2 is `O(num_chunks)`. With
    /// `chunk_size = sqrt(n)` the worst case is `O(sqrt n)`.
    ///
    /// # Flushing
    /// Lazy deltas accumulate in `chunk_delta`. To fold them back into the
    /// `starts` array (so subsequent `offset` calls don't pay the
    /// `chunk_delta` lookup at all, becoming `O(1)` with zero overhead), call
    /// [`flush`](Self::flush) — typically after a burst of `set_size` calls
    /// has settled.
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
        let chunk = self.chunk_of(index);
        let chunk_end = ((chunk + 1) * self.chunk_size).min(self.starts.len());
        // 1) Within the same chunk: shift starts[i+1..chunk_end] by delta.
        for j in (index + 1)..chunk_end {
            self.starts[j] = self.starts[j].saturating_add(delta);
        }
        // 2) For chunks strictly AFTER chunk_of(index): add delta to their
        //    chunk_delta entries. `offset(i)` reads chunk_delta[chunk_of(i)],
        //    which is the cumulative delta from all items in chunks BEFORE
        //    chunk_of(i) — that is exactly the chunks `[0, chunk_of(i))`.
        //    For items in chunk `c > chunk_of(index)`, we want their offset
        //    to shift by `delta`, so we add delta to `chunk_delta[c]`.
        for c in (chunk + 1)..self.chunk_delta.len() {
            self.chunk_delta[c] = self.chunk_delta[c].saturating_add(delta);
        }
        // `total()` reads `starts[n] + chunk_delta[chunk_of(n-1)]` (or
        // chunk_of(n) if that's larger, see `total`). Either way, the loop
        // above has added delta to chunk_delta[chunk_of(n-1) + 1 .. ], so we
        // also need to ensure chunk_delta for the chunk CONTAINING the
        // trailing `starts[n]` slot reflects the delta. We allocated
        // `num_chunks = ceil((n+1) / chunk_size)` so the total slot
        // `starts[n]` lives in chunk `chunk_of(n)`. The loop above already
        // incremented chunk_delta for chunks `> chunk_of(index)`, which
        // includes `chunk_of(n)`. ✓
        from_sub(delta)
    }

    /// Fold all lazy chunk deltas back into the `starts` array and reset the
    /// deltas to zero. After this call, `offset` is `O(1)` with zero overhead
    /// (`chunk_delta[c] == 0` for every `c`) until the next `set_size`.
    ///
    /// Useful to call after a burst of `set_size` calls has settled (e.g.
    /// "image gallery finished loading everything"), so subsequent reads are
    /// as fast as [`Strip::offset`](super::Strip::offset).
    pub fn flush(&mut self) {
        if self.n == 0 {
            return;
        }
        // `chunk_delta[c]` is cumulative (sum of deltas applied to items in
        // chunks `[0, c)`), so we can simply add `chunk_delta[c]` to every
        // `starts[i]` in chunk `c` in any order.
        for c in 0..self.chunk_delta.len() {
            let d = self.chunk_delta[c];
            if d == 0 {
                continue;
            }
            let lo = c * self.chunk_size;
            let hi = ((c + 1) * self.chunk_size).min(self.starts.len());
            for i in lo..hi {
                self.starts[i] = self.starts[i].saturating_add(d);
            }
            self.chunk_delta[c] = 0;
        }
    }

    /// Chunk index that owns item `index`.
    #[inline]
    fn chunk_of(&self, index: usize) -> usize {
        index / self.chunk_size
    }
}

// `usize::isqrt` is stable since Rust 1.84 and used directly above.
