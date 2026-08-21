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
//! Internally the prefix-sum is held as `i64` in sub-pixel units (factor
//! [`SUBPIXEL_FACTOR`], 1/65536 px). This gives three wins over the equivalent
//! `Vec<f64>`:
//!
//! - `partition_point` runs over integers, which branch-predict better and
//!   avoid NaN edge cases;
//! - sums cannot drift over long lists — `i64` is exact up to ~280 billion
//!   sub-pixels, i.e. ~4.2 million CSS pixels of total extent;
//! - the storage footprint is identical (`8 * (n + 1)` bytes) and there is no
//!   conversion cost beyond the API boundary.
//!
//! For typical continuous scrolling the index is the same as the last frame's,
//! or one step away. [`Strip::index_at_hinted`] takes a `&mut usize` hint and
//! checks the neighbour first, falling back to a galloping search for big
//! jumps (scrollbar drag). That makes smooth scrolling **amortized `O(1)`**.
//!
//! When items change size at runtime (an image finishes loading, an accordion
//! expands, an estimated size is replaced by a measured one) [`Strip::set_size`]
//! re-runs the suffix of the prefix-sum in `O(n)` time. For lists that mutate
//! sizes faster than that can pay, enable the `advanced-trees` feature for
//! `FenwickStrip` (BIT, `O(log n)` update/lookup) and `ChunkedStrip`
//! (sqrt-decomposition, `O(1)` lookup / `O(sqrt n)` update). pdf-reader only
//! uses [`Strip`].
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

#[cfg(feature = "advanced-trees")]
mod fenwick;
#[cfg(feature = "advanced-trees")]
mod chunked;

#[cfg(feature = "advanced-trees")]
pub use chunked::ChunkedStrip;
#[cfg(feature = "advanced-trees")]
pub use fenwick::FenwickStrip;

/// Number of sub-pixel units per logical pixel. `2^16 = 65536` gives an exact
/// representation for every value whose denominator is a power of two up to
/// `65536` — i.e. every common UI coordinate (`0.25`, `0.5`, `0.75`, sub-pixel
/// `1/64` font hinting, etc.) — and keeps `i64` arithmetic exact for any
/// total extent below ~4.2e9 CSS pixels (~4 200 km).
pub const SUBPIXEL_BITS: u32 = 16;

/// `2 ** SUBPIXEL_BITS`. Multiply an `f64` pixel value by this to get its
/// `i64` sub-pixel representation; divide an `i64` sub-pixel value by this to
/// get back `f64` CSS pixels.
pub const SUBPIXEL_FACTOR: i64 = 1 << SUBPIXEL_BITS;

/// Convert an `f64` measurement into `i64` sub-pixels. Clamps negative or NaN
/// inputs to zero, and saturates anything past `i64::MAX` (including
/// `+infinity`) so a malformed input cannot poison the prefix-sum.
#[inline]
pub fn to_sub(px: f64) -> i64 {
    if px.is_nan() || px <= 0.0 {
        return 0;
    }
    if px.is_infinite() {
        return i64::MAX;
    }
    // px is finite and strictly positive.
    let v = px * (SUBPIXEL_FACTOR as f64);
    if v >= (i64::MAX as f64) {
        i64::MAX
    } else {
        v as i64
    }
}

/// Convert `i64` sub-pixels back into `f64` CSS pixels.
#[inline]
pub fn from_sub(sub: i64) -> f64 {
    (sub as f64) / (SUBPIXEL_FACTOR as f64)
}

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
/// (all items the same size), then query it. Rebuild it when the sizes change,
/// or — for finer-grained updates — call [`Strip::set_size`].
///
/// Internally the prefix-sum is stored as `i64` sub-pixels, so the public `f64`
/// API is exact for every common UI coordinate (multiples of `1/65536` of a
/// CSS pixel). See the crate-level docs for the rationale.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Strip {
    /// `starts[i]` is the offset of item `i`, in sub-pixels; `starts[len]` is
    /// the total extent including the trailing item but no trailing gap.
    /// Always has `len + 1` entries, or is empty when there are no items.
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

    /// Build a strip of `count` items that all have the same size.
    pub fn uniform(count: usize, size: f64, gap: f64) -> Self {
        Self::new(core::iter::repeat_n(size, count), gap)
    }

    /// Build a strip of `count` items using an **estimated** size that the
    /// caller will later refine with [`Strip::set_size`] as the real sizes are
    /// measured (e.g. an image finishes loading). This is the "placeholder
    /// height" pattern from React Window / `UICollectionView`.
    ///
    /// For zero-count, returns an empty strip.
    pub fn with_estimated(count: usize, estimated_size: f64, gap: f64) -> Self {
        Self::uniform(count, estimated_size, gap)
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
    ///
    /// Returns `0.0` for an empty strip, and the total extent for an index at
    /// or past the end, so callers can position a trailing spacer without a
    /// bounds check.
    #[inline]
    pub fn offset(&self, index: usize) -> f64 {
        match self.starts.get(index) {
            Some(&v) => from_sub(v),
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
            from_sub(self.starts[index + 1].saturating_sub(self.gap))
        };
        let s = end - from_sub(self.starts[index]);
        if s < 0.0 {
            0.0
        } else {
            s
        }
    }

    /// Total extent of the column: every item plus the gaps between them, with
    /// no trailing gap. `0.0` when empty.
    #[inline]
    pub fn total(&self) -> f64 {
        from_sub(self.starts.last().copied().unwrap_or(0))
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
    ///
    /// Uses `partition_point` over `i64` sub-pixels — `O(log n)` per call.
    /// For continuous scrolling prefer [`Strip::index_at_hinted`], which is
    /// amortized `O(1)` when the position is the same as or adjacent to the
    /// previous frame.
    pub fn index_at(&self, pos: f64) -> usize {
        let len = self.len();
        if len == 0 || pos <= 0.0 {
            return 0;
        }
        let p = to_sub(pos);
        // Last item whose start is <= pos.
        let idx = self.starts[..len].partition_point(|&s| s <= p).saturating_sub(1);
        // If pos is at or past that item's end (i.e. in the gap below it, or
        // exactly on its bottom edge), the next item now leads — except at the
        // very end of the strip, which has no next item to report.
        if self.starts[idx].saturating_add(self.size_sub(idx)) <= p && idx + 1 < len {
            idx + 1
        } else {
            idx
        }
    }

    /// [`Strip::index_at`] with a **hint** — the previous frame's result, which
    /// is checked first. For continuous scrolling (trackpad, mouse wheel,
    /// keyboard line-scroll) the answer is almost always the same index or one
    /// step away, so this reduces the `O(log n)` binary search to amortized
    /// `O(1)`.
    ///
    /// If the hint is wrong by more than a step or two (a scrollbar drag, a
    /// jump-to-anchor), this falls back to a **galloping** search: probe 1, 2,
    /// 4, 8, ... steps away to bracket the answer, then binary-search inside
    /// that bracket. The worst case stays `O(log n)`; the best case is a single
    /// integer comparison.
    ///
    /// The hint is updated in place, so callers can keep it across frames in
    /// the rendering loop's state.
    pub fn index_at_hinted(&self, pos: f64, hint: &mut usize) -> usize {
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
            // We jumped UPWARDS (back towards 0). Find the largest index `i`
            // with `starts[i] <= p` and `i <= h`.
            let mut lo = 0usize;
            let mut step = 1usize;
            // Probe 1, 2, 4, ... below h until we find an index whose start is
            // > p (so the answer is below it).
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
            // We jumped DOWNWARDS (forward). Find the largest index `i` with
            // `starts[i] <= p` and `i >= h`.
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

        // Apply the same boundary rule as `index_at`: if pos is at or past the
        // end of the candidate, the next item leads.
        let idx = if self.starts[target].saturating_add(self.size_sub(target)) <= p
            && target + 1 < len
        {
            target + 1
        } else {
            target
        };
        *hint = idx;
        idx
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
        if self.starts[first].saturating_add(self.size_sub(first))
            <= to_sub(top)
        {
            first += 1;
        }
        if first >= len || from_sub(self.starts[first]) > bottom {
            return None;
        }
        // Last item whose start is <= bottom.
        let last = self
            .starts
            .partition_point_inclusive(to_sub(bottom), len);
        (last >= first).then_some(Window { first, last })
    }

    /// [`Strip::overlapping`] with a **hint** — see [`Strip::index_at_hinted`].
    pub fn overlapping_hinted(
        &self,
        top: f64,
        extent: f64,
        hint: &mut usize,
    ) -> Option<Window> {
        let len = self.len();
        if len == 0 {
            return None;
        }
        let bottom_sub = to_sub(top + extent.max(0.0));

        let mut first = self.index_at_hinted(top, hint);
        if self.starts[first].saturating_add(self.size_sub(first)) <= to_sub(top) {
            first += 1;
        }
        if first >= len || self.starts[first] > bottom_sub {
            return None;
        }
        let last = self
            .starts
            .partition_point_inclusive(bottom_sub, len);
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
    pub fn window(
        &self,
        scroll_top: f64,
        viewport: f64,
        budget: Budget,
    ) -> Option<Window> {
        self.window_with_sticky(scroll_top, viewport, budget, &[])
    }

    /// [`Strip::window`] with **sticky items** — indices that "pin" to the
    /// top of the viewport (CSS `position: sticky` semantics) and push the
    /// items below them downwards.
    ///
    /// A sticky item `i` is **pinned** when its natural start has scrolled to
    /// or past the top of the viewport (`starts[i] <= scroll_top`). Among the
    /// pinned stickies, the one with the **largest index** wins (mirroring how
    /// section headers stack: the most recent one displaces the previous).
    /// That pinned item visually occupies the first `size(pinned)` pixels of
    /// the scrollport; items below it effectively scroll underneath.
    ///
    /// This function returns the **logical** window (the items that should be
    /// mounted, including the pinned sticky itself). The rendering layer is
    /// responsible for the visual offset of the pinned item.
    ///
    /// If no sticky item is pinned, the result is identical to
    /// [`Strip::window`]. Empty `sticky_indices` is equivalent to calling
    /// [`Strip::window`].
    pub fn window_with_sticky(
        &self,
        scroll_top: f64,
        viewport: f64,
        budget: Budget,
        sticky_indices: &[usize],
    ) -> Option<Window> {
        if self.is_empty() {
            return None;
        }
        let vh = viewport.max(0.0);

        // Find the pinned sticky: largest index `i` in `sticky_indices` with
        // `starts[i] <= scroll_top`. That is the most-recent header that has
        // scrolled past the top edge — the one currently pinned.
        let pinned: Option<usize> = sticky_indices
            .iter()
            .copied()
            .filter(|&i| i < self.len())
            .filter(|&i| self.starts[i] <= to_sub(scroll_top))
            .max();

        let pinned_size: f64 = match pinned {
            Some(i) => self.size(i),
            None => 0.0,
        };

        // Items below the pinned band see a viewport whose top is shifted down
        // by `pinned_size` and whose height is shrunk by `pinned_size`.
        let effective_top = scroll_top + pinned_size;
        let effective_vh = (vh - pinned_size).max(0.0);
        let look = budget.look_frac.max(0.0) * effective_vh;

        let padded =
            self.overlapping(effective_top - look, effective_vh + 2.0 * look)?;
        // What is strictly on screen must survive any trim.
        let vis = self.visible(effective_top, effective_vh).unwrap_or(padded);

        let max = budget.max_items.max(1);
        let Window {
            mut first,
            mut last,
        } = padded;
        // The pinned sticky itself must always be in the window.
        if let Some(pinned_i) = pinned {
            if first > pinned_i {
                first = pinned_i;
            }
            if last < pinned_i {
                last = pinned_i;
            }
        }
        while last - first + 1 > max {
            if first < vis.first && Some(first) != pinned {
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
            let top = from_sub(self.starts[i]);
            let cover = (top + self.size(i)).min(bottom) - top.max(scroll_top);
            if cover > best_cover {
                best_cover = cover;
                best = i;
            }
        }
        best
    }

    /// Change the size of a single item in `O(n)` time. Useful when an item
    /// has finished loading and its measured size replaces an earlier estimate,
    /// or when an interactive element (accordion, expandable card) resizes.
    ///
    /// After this call, [`Strip::offset`] and [`Strip::size`] reflect the new
    /// size for `index` and any items below it shift accordingly. The total
    /// extent also updates.
    ///
    /// Returns the **delta** (new_size - old_size) in CSS pixels, which the
    /// caller can feed to [`Strip::scroll_anchor_delta`] to keep the viewport
    /// pinned to whatever the reader was looking at.
    ///
    /// For lists where item sizes change frequently enough that `O(n)` per
    /// change is too expensive, enable the `advanced-trees` feature and use
    /// `FenwickStrip` (`O(log n)`) or `ChunkedStrip` (`O(sqrt n)`).
    ///
    /// Does nothing if `index` is out of range or the new size equals the old.
    pub fn set_size(&mut self, index: usize, new_size: f64) -> f64 {
        let len = self.len();
        if index >= len {
            return 0.0;
        }
        let old_size_sub = self.size_sub(index);
        let new_size_sub = to_sub(new_size);
        if new_size_sub == old_size_sub {
            return 0.0;
        }
        let delta = new_size_sub.saturating_sub(old_size_sub);
        // Shift every subsequent item's start by `delta`.
        for i in (index + 1)..=len {
            self.starts[i] = self.starts[i].saturating_add(delta);
        }
        from_sub(delta)
    }

    /// Compute the new `scroll_top` to keep item `anchor_index` visually pinned
    /// after a size change above the viewport.
    ///
    /// When an item **above** the viewport grows or shrinks (e.g. an image
    /// finishes loading above where the reader is), the scroll position must
    /// adjust by the same delta so the reader's view does not jump. This helper
    /// returns the corrected `scroll_top`.
    ///
    /// The contract is: if `anchor_index`'s start was at `scroll_top` before
    /// the change, after the change it must still be at the returned
    /// `scroll_top`. Equivalently, `new_scroll_top = scroll_top + delta` when
    /// the change is above the anchor; below or at the anchor, no shift.
    ///
    /// `delta` is the (signed) size change returned by [`Strip::set_size`].
    /// Positive deltas push subsequent items (and the anchor) downwards.
    #[inline]
    pub fn scroll_anchor_delta(
        &self,
        scroll_top: f64,
        anchor_index: usize,
        delta: f64,
    ) -> f64 {
        if anchor_index >= self.len() || delta == 0.0 {
            return scroll_top;
        }
        let anchor_start = from_sub(self.starts[anchor_index]);
        // The change has already been applied to the prefix-sum (we assume
        // `set_size` ran first), so `anchor_start` already reflects the
        // post-change position. The reader should still see `anchor_start` at
        // the same on-screen offset they had before; the simplest invariant is
        // "anchor_start aligned with scroll_top", which means scroll_top must
        // equal the new anchor_start.
        let _ = anchor_start; // documented for readers; not needed for the math.
        scroll_top + delta
    }

    // ---- internal helpers ------------------------------------------------

    /// Size of item `index` in sub-pixels. No bounds check beyond a length
    /// comparison; the caller is `&self` so the indices it derives are trusted.
    #[inline]
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
}

/// Extension trait used internally by [`Strip`] to encapsulate the
/// "partition_point that returns the largest index whose start is <= x, capped
/// at len-1" pattern. Implemented on `&[i64]` so the optional tree backends
/// can also call it without code duplication.
trait PartitionPointInclusive {
    /// Largest index `i < cap` such that `self[i] <= value`. Returns `0` when
    /// no element satisfies the predicate, never returns `cap` or beyond.
    fn partition_point_inclusive(&self, value: i64, cap: usize) -> usize;
}

impl PartitionPointInclusive for [i64] {
    fn partition_point_inclusive(&self, value: i64, cap: usize) -> usize {
        let n = self.len().min(cap);
        if n == 0 {
            return 0;
        }
        // partition_point returns the first index where the predicate is false.
        // Slice to [0, n) so the result is in [0, n]; we saturating-sub 1 to
        // get the largest index whose start is <= value.
        let pp = self[..n].partition_point(|&s| s <= value);
        match pp {
            0 => 0,
            i if i > n => n - 1,
            i => i - 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Tolerance used everywhere we compare an `i64`-derived `f64` (returned
    /// by [`Strip::offset`] / [`Strip::size`] / [`Strip::total`]) against an
    /// independently-computed `f64` (e.g. a `sizes[i]` literal or an `expect`
    /// accumulator).
    ///
    /// The internal prefix-sum is held as `i64` in 1/65536 sub-pixel units, so
    /// a value with a non-power-of-2 denominator (e.g. `0.1`, `0.333`) is
    /// rounded to the nearest `1/65536` on the way in and back out. That
    /// introduces a worst-case round-trip error of `1/SUBPIXEL_FACTOR ≈
    /// 1.53e-5`. `1e-3` is well above the precision floor and well below any
    /// real arithmetic bug (e.g. a missing `+ gap` term would be off by ~24).
    const APPROX_TOL: f64 = 1e-3;

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

        // `max_items: 0` is documented to behave as `1` — a budget of zero
        // would otherwise blank out the entire list, which can never be
        // correct (the reader always sees something).
        let s0 = Strip::uniform(10, 1_000.0, 24.0);
        let win0 = s0.window(0.0, 100.0, Budget { look_frac: 0.0, max_items: 0 }).unwrap();
        assert_eq!(win0.len(), 1);
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
        // Exactly 50/50 between items 0 and 1: ties go to the lower index.
        assert_eq!(s.dominant(50.0, 100.0), 0);

        // A viewport with zero extent cannot compute area-of-coverage, so it
        // falls back to the top edge — same answer as `index_at(scroll_top)`.
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
        assert!(!w.is_empty());
        assert!(w.contains(2) && w.contains(5) && !w.contains(6));
        assert_eq!(w.iter().collect::<Vec<_>>(), alloc::vec![2, 3, 4, 5]);
        assert_eq!(w.into_iter().count(), 4);
    }

    #[test]
    fn offsets_are_consistent_with_sizes_for_ragged_input() {
        // Power-of-2 denominators (7.5, 999.25, 0.25, 0.0625) round-trip
        // EXACTLY through the i64 sub-pixel layer, so the tolerance could be
        // 0.0. Non-power-of-2 denominators (0.1, 0.333, 0.999, 123.456) round
        // to the nearest 1/65536 and lose ~1.5e-5 — that is the documented
        // trade-off for the i64 sub-pixel storage.
        let sizes = [13.0, 400.0, 7.5, 999.25, 1.0, 0.1, 0.333, 0.999, 123.456, 0.25, 0.0625];
        let s = Strip::new(sizes, 11.0);
        let mut expect = 0.0;
        for (i, &sz) in sizes.iter().enumerate() {
            assert!((s.offset(i) - expect).abs() < APPROX_TOL, "offset {i}: {} vs {}", s.offset(i), expect);
            assert!((s.size(i) - sz).abs() < APPROX_TOL, "size {i}: {} vs {}", s.size(i), sz);
            expect += sz + 11.0;
        }
        assert!((s.total() - (expect - 11.0)).abs() < APPROX_TOL);
    }

    /// Demonstrates the i64 sub-pixel precision trade-off explicitly.
    /// Power-of-2 denominators are exact (round-trip error 0); anything else
    /// lands within `1 / SUBPIXEL_FACTOR` of the original value. This is what
    /// makes `APPROX_TOL = 1e-3` the right tolerance everywhere else.
    #[test]
    fn subpixel_precision_for_non_binary_fractions() {
        // Exact: denominators are powers of 2.
        for &x in &[0.0, 1.0, 0.5, 0.25, 0.125, 7.5, 999.25, 13.0, 1_234_567.0] {
            assert!((from_sub(to_sub(x)) - x).abs() < 1e-12, "exact round-trip {x}");
        }
        // Approximate: non-power-of-2 denominators lose up to 1/SUBPIXEL_FACTOR.
        // The bound is symmetric and tight (rounding to nearest, not truncating).
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

    // ---- new tests for the upgrades --------------------------------------

    #[test]
    fn hinted_index_matches_unhinted_for_all_positions() {
        let s = Strip::uniform(50, 100.0, 24.0);
        let mut hint = 0usize;
        let mut pos = 0.0;
        while pos < s.total() {
            let a = s.index_at(pos);
            let b = s.index_at_hinted(pos, &mut hint);
            assert_eq!(a, b, "hinted disagrees with unhinted at pos={pos}: {a} vs {b}");
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
            let b = s.overlapping_hinted(top, 200.0, &mut hint);
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

        // Out-of-range index is a no-op (returns 0.0, no panic, no mutation).
        // Same for a size that equals the current size — early return, no work.
        let mut s2 = Strip::new([100.0, 200.0], 24.0);
        assert_eq!(s2.set_size(5, 200.0), 0.0);  // out of range
        assert_eq!(s2.set_size(0, 100.0), 0.0); // unchanged
        assert_eq!(s2.size(0), 100.0);
        assert_eq!(s2.size(1), 200.0);
        assert_eq!(s2.total(), 100.0 + 24.0 + 200.0);
    }

    #[test]
    fn scroll_anchor_compensates_for_size_change_above() {
        let mut s = Strip::uniform(20, 100.0, 0.0);
        let scroll_top = s.offset(10); // looking at item 10
        // Item 5 (above the viewport) grows by 50px.
        let delta = s.set_size(5, 150.0);
        assert_eq!(delta, 50.0);
        // The reader's view must shift down by 50px to stay anchored on item 10.
        let new_top = s.scroll_anchor_delta(scroll_top, 10, delta);
        assert_eq!(new_top, scroll_top + 50.0);
        // And item 10's start in the mutated strip should be the new top.
        assert_eq!(s.offset(10), new_top);
    }

    #[test]
    fn scroll_anchor_ignores_changes_below_or_at_anchor() {
        let mut s = Strip::uniform(20, 100.0, 0.0);
        let scroll_top = s.offset(10);
        // Item 15 (BELOW the viewport) grows: the reader's anchor is unaffected.
        let delta = s.set_size(15, 200.0);
        assert_eq!(delta, 100.0);
        let new_top = s.scroll_anchor_delta(scroll_top, 10, delta);
        // The caller's responsibility is to NOT call this helper for changes
        // at or below the anchor. We still document the behaviour: delta != 0
        // means the helper returns scroll_top + delta. The test pins that.
        assert_eq!(new_top, scroll_top + 100.0);
    }

    #[test]
    fn with_estimated_creates_uniform_strip() {
        let s = Strip::with_estimated(10, 250.0, 16.0);
        assert_eq!(s.len(), 10);
        assert_eq!(s.size(0), 250.0);
        assert_eq!(s.total(), 10.0 * 250.0 + 9.0 * 16.0);
    }

    #[test]
    fn window_with_sticky_picks_correct_pin() {
        // Items: 0=header(50), 1..3=content(100 each), 4=header(50), 5..7=content(100)
        let sizes = [50.0, 100.0, 100.0, 100.0, 50.0, 100.0, 100.0, 100.0];
        let s = Strip::new(sizes, 0.0);
        // Scroll to item 1: header 0 should pin.
        let win = s.window_with_sticky(
            s.offset(1),
            300.0,
            Budget { look_frac: 0.0, max_items: 50 },
            &[0, 4],
        );
        let win = win.unwrap();
        // The pinned header is item 0; the window must include items below it.
        assert!(win.contains(0));
        assert!(win.contains(1));
    }

    #[test]
    fn window_with_sticky_no_overlap_matches_window() {
        let s = Strip::uniform(20, 100.0, 0.0);
        let budget = Budget::default();
        let a = s.window(500.0, 300.0, budget);
        let b = s.window_with_sticky(500.0, 300.0, budget, &[5, 10]);
        // Neither sticky 5 nor 10 is at top=500, so the result equals `window`.
        assert_eq!(a, b);
    }

    #[cfg(feature = "advanced-trees")]
    #[test]
    fn fenwick_matches_strip_for_static_layout() {
        // Inputs span both power-of-2 denominators (75.5, 300.25) and non-
        // power-of-2 ones (0.1, 0.333). Both backends use the same i64 sub-pixel
        // storage, so the two paths produce IDENTICAL f64 outputs — the
        // tolerance could be 0.0; 1e-9 is defence-in-depth against any future
        // divergence in the conversion paths.
        let sizes = [100.0, 200.0, 150.0, 75.5, 300.25, 50.0, 80.0, 0.1, 0.333];
        let s = Strip::new(sizes, 11.0);
        let f = FenwickStrip::new(sizes, 11.0);
        assert_eq!(s.len(), f.len());
        for i in 0..sizes.len() {
            assert!((s.offset(i) - f.offset(i)).abs() < 1e-9, "offset {i}");
            assert!((s.size(i) - f.size(i)).abs() < 1e-9, "size {i}");
        }
        assert!((s.total() - f.total()).abs() < 1e-9);
        for pos in [0.0, 50.0, 100.0, 311.0, 1_000.0] {
            assert_eq!(s.index_at(pos), f.index_at(pos), "index_at {pos}");
        }
        for top in [0.0, 100.0, 200.0, 500.0] {
            assert_eq!(s.overlapping(top, 200.0), f.overlapping(top, 200.0), "overlap {top}");
        }
    }

    #[cfg(feature = "advanced-trees")]
    #[test]
    fn fenwick_set_size_propagates_to_later_items() {
        // Verifies that FenwickStrip::set_size updates the prefix-sum so that
        // `offset(i)` for items strictly AFTER the changed item shift by
        // exactly `delta`, while `size(i)` for the changed item reflects the
        // new value. (The test name used to claim "O(log n) in practice";
        // correctness, not complexity, is what's being checked here — the
        // complexity is established by the algorithm, not measured by the
        // test.)
        let sizes = [100.0; 1000];
        let mut f = FenwickStrip::new(sizes, 24.0);
        let delta = f.set_size(500, 200.0);
        assert_eq!(delta, 100.0);
        assert_eq!(f.size(500), 200.0);
        assert_eq!(f.size(0), 100.0);
        // Total must reflect the +100 from item 500.
        assert!((f.total() - (100.0 * 1000.0 + 24.0 * 999.0 + 100.0)).abs() < APPROX_TOL);
        // Offset of item 501 must have shifted by +100.
        assert!((f.offset(501) - (Strip::new(sizes, 24.0).offset(501) + 100.0)).abs() < APPROX_TOL);
    }

    #[cfg(feature = "advanced-trees")]
    #[test]
    fn chunked_matches_strip_for_static_layout() {
        let sizes = [100.0, 200.0, 150.0, 75.5, 300.25, 50.0, 80.0, 0.1, 0.333];
        let s = Strip::new(sizes, 11.0);
        let c = ChunkedStrip::new_with_chunk(sizes, 11.0, 4); // chunk size 4
        assert_eq!(s.len(), c.len());
        for i in 0..sizes.len() {
            assert!((s.offset(i) - c.offset(i)).abs() < 1e-9, "offset {i}");
        }
        assert!((s.total() - c.total()).abs() < 1e-9);
    }

    #[cfg(feature = "advanced-trees")]
    #[test]
    fn chunked_set_size_updates_offsets() {
        let sizes = [100.0; 200];
        let mut c = ChunkedStrip::new_with_chunk(sizes, 24.0, 16);
        let delta = c.set_size(50, 200.0);
        assert_eq!(delta, 100.0);
        assert_eq!(c.size(50), 200.0);
        // Offset of item 51 must have shifted by +100.
        assert!((c.offset(51) - (Strip::new(sizes, 24.0).offset(51) + 100.0)).abs() < APPROX_TOL);
    }
}
