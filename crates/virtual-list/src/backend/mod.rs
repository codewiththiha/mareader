//! Shared backend trait: `StripBackend` defines the primitives every geometry
//! engine provides (`offset_sub`, `size_sub`, `total_sub`, `index_at_sub`,
//! `set_size_sub`). All windowing logic (`overlapping`, `visible`, `window`,
//! `dominant`, `window_with_sticky`) is written once against this trait,
//! so adding a new backend only requires implementing the primitives.
//!
//! The math stays in `i64` sub-pixels (`to_sub` / `from_sub`) so boundary
//! behavior is bit-for-bit identical across backends.

use crate::units::{from_sub, to_sub};
use crate::window::{Budget, Window};

/// The primitive column geometry every backend provides, in sub-pixels.
pub trait StripBackend {
    /// Number of items.
    fn len(&self) -> usize;

    /// Whether there are no items.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Gap between adjacent items, in sub-pixels.
    fn gap_sub(&self) -> i64;

    /// Offset of item `index` start, in sub-pixels. Returns total for
    /// `index >= len` (trailing-spacer friendly).
    fn offset_sub(&self, index: usize) -> i64;

    /// Size of item `index`, in sub-pixels. `0` out of range.
    fn size_sub(&self, index: usize) -> i64;

    /// Total extent (every item + gaps between them, no trailing gap), sub-px.
    fn total_sub(&self) -> i64;

    /// Index of the item whose span contains sub-pixel position `p`.
    /// Same leading-edge boundary rules as `Strip::index_at`.
    fn index_at_sub(&self, p: i64) -> usize;

    /// Set item `index` to `new_sub` (sub-pixels). Returns signed delta in sub-px.
    fn set_size_sub(&mut self, index: usize, new_sub: i64) -> i64;

    // f64 convenience wrappers (default implementations, can be overridden)
    /// Gap between adjacent items, in CSS pixels.
    fn gap(&self) -> f64 {
        from_sub(self.gap_sub())
    }

    /// Offset of the start of item `index`, in CSS pixels.
    fn offset(&self, index: usize) -> f64 {
        from_sub(self.offset_sub(index))
    }

    /// Size of item `index`, in CSS pixels. `0.0` out of range.
    fn size(&self, index: usize) -> f64 {
        from_sub(self.size_sub(index))
    }

    /// Total extent of the column: every item plus the gaps between them,
    /// with no trailing gap. `0.0` when empty.
    fn total(&self) -> f64 {
        from_sub(self.total_sub())
    }

    /// Average item extent — resolves [`crate::Overscan::Items`] budgets.
    fn mean_size(&self) -> f64 {
        let len = self.len();
        if len == 0 {
            0.0
        } else {
            self.total() / len as f64
        }
    }

    /// Index of the item whose span contains `pos` (f64 version).
    fn index_at(&self, pos: f64) -> usize {
        if pos <= 0.0 {
            return 0;
        }
        self.index_at_sub(to_sub(pos))
    }

    /// Set item `index` to `new_size` (f64). Returns signed delta in CSS pixels.
    fn set_size(&mut self, index: usize, new_size: f64) -> f64 {
        let new_sub = to_sub(new_size);
        let delta_sub = self.set_size_sub(index, new_sub);
        from_sub(delta_sub)
    }
}

/// Shared `overlapping` — written once, identical for every backend.
/// Keeps the boundary-critical math in `i64` sub-pixels.
pub fn overlapping<B: StripBackend + ?Sized>(b: &B, top: f64, extent: f64) -> Option<Window> {
    let len = b.len();
    if len == 0 {
        return None;
    }
    let extent = extent.max(0.0);
    if extent == 0.0 {
        return None;
    }
    let top_sub = to_sub(top);
    let bottom_sub = to_sub(top + extent);

    let mut first = b.index_at_sub(top_sub);
    if b.offset_sub(first).saturating_add(b.size_sub(first)) <= top_sub {
        first += 1;
    }
    if first >= len || b.offset_sub(first) >= bottom_sub {
        return None;
    }
    // Binary search for the last item whose start is strictly below bottom.
    let mut lo = first;
    let mut hi = len;
    while lo + 1 < hi {
        let mid = (lo + hi) / 2;
        if b.offset_sub(mid) < bottom_sub {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let last = lo;
    (last >= first).then_some(Window { first, last })
}

/// Shared `visible` — shorthand for `overlapping` with the raw viewport.
#[inline]
pub fn visible<B: StripBackend + ?Sized>(
    b: &B,
    scroll_top: f64,
    viewport: f64,
) -> Option<Window> {
    overlapping(b, scroll_top, viewport)
}

/// Shared `dominant` — the item occupying the most viewport area.
pub fn dominant<B: StripBackend + ?Sized>(b: &B, scroll_top: f64, viewport: f64) -> usize {
    if b.is_empty() {
        return 0;
    }
    if viewport <= 0.0 {
        return b.index_at(scroll_top);
    }
    let Some(win) = visible(b, scroll_top, viewport) else {
        return b.index_at(scroll_top);
    };
    let bottom = scroll_top + viewport;
    let mut best = win.first;
    let mut best_cover = -1.0;
    for i in win.first..=win.last {
        let top = b.offset(i);
        let cover = (top + b.size(i)).min(bottom) - top.max(scroll_top);
        if cover > best_cover {
            best_cover = cover;
            best = i;
        }
    }
    best
}

/// Shared `window` — visible + overscan, trimmed to budget.
pub fn window<B: StripBackend + ?Sized>(
    b: &B,
    scroll_top: f64,
    viewport: f64,
    budget: Budget,
) -> Option<Window> {
    window_with_sticky(b, scroll_top, viewport, budget, &[])
}

/// Shared `window_with_sticky` — sticky items pin to the viewport top.
/// This is the exact same algorithm used by `Strip::window_with_sticky`.
pub fn window_with_sticky<B: StripBackend + ?Sized>(
    b: &B,
    scroll_top: f64,
    viewport: f64,
    budget: Budget,
    sticky_indices: &[usize],
) -> Option<Window> {
    if b.is_empty() {
        return None;
    }
    let vh = viewport.max(0.0);

    // Find pinned sticky: largest index in sticky_indices with offset <= scroll_top.
    let pinned: Option<usize> = sticky_indices
        .iter()
        .copied()
        .filter(|&i| i < b.len())
        .filter(|&i| b.offset_sub(i) <= to_sub(scroll_top))
        .max();

    if vh == 0.0 {
        return (scroll_top < b.total()).then(|| {
            let point = Window {
                first: b.index_at(scroll_top),
                last: b.index_at(scroll_top),
            };
            match pinned {
                Some(index) => point.union(Window {
                    first: index,
                    last: index,
                }),
                None => point,
            }
        });
    }

    let pinned_size = pinned.map(|i| b.size(i)).unwrap_or(0.0);
    let effective_top = scroll_top + pinned_size;
    let effective_vh = (vh - pinned_size).max(0.0);
    let look = budget.overscan.padding(effective_vh, b.mean_size());

    let padded = overlapping(b, effective_top - look, effective_vh + 2.0 * look)?;
    let vis = visible(b, effective_top, effective_vh).unwrap_or(padded);

    let max = budget.max_items.max(1);
    let mut first = padded.first;
    let mut last = padded.last;

    // The pinned sticky itself must always be included.
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
            break;
        }
    }
    Some(Window { first, last })
}

pub mod strip;
#[cfg(feature = "advanced-trees")]
pub mod fenwick;
#[cfg(feature = "advanced-trees")]
pub mod chunked;
pub mod uniform;

pub use strip::Strip;
#[cfg(feature = "advanced-trees")]
pub use fenwick::FenwickStrip;
#[cfg(feature = "advanced-trees")]
pub use chunked::ChunkedStrip;
pub use uniform::UniformStrip;
