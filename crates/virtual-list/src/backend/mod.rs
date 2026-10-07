//! The backend trait: five primitives, all windowing written once.

use crate::units::{from_sub, to_sub};
use crate::window::{Budget, Window};

/// The primitive column geometry every backend provides, in sub-pixels.
pub trait StripBackend {
    /// How many items the backend holds.
    fn len(&self) -> usize;

    /// Whether the backend holds none; the default is `len() == 0`.
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
    fn index_at_sub(&self, p: i64) -> usize;

    /// Set item `index` to `new_sub`; returns the signed delta.
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

    /// Total extent of the column, no trailing gap; 0.0 when empty.
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

    /// Set item `index` to `new_size`; returns the signed delta.
    fn set_size(&mut self, index: usize, new_size: f64) -> f64 {
        let new_sub = to_sub(new_size);
        let delta_sub = self.set_size_sub(index, new_sub);
        from_sub(delta_sub)
    }

    /// [`index_at`](Self::index_at) with a per-frame hint; the default
    /// records the unhinted answer.
    fn index_at_hinted(&self, pos: f64, hint: &mut usize) -> usize {
        let index = self.index_at(pos);
        *hint = index;
        index
    }

    /// [`window`](window) with a per-frame hint.
    fn window_hinted(
        &self,
        scroll_top: f64,
        viewport: f64,
        budget: Budget,
        hint: &mut usize,
    ) -> Option<Window> {
        window_hinted(self, scroll_top, viewport, budget, hint)
    }
}

/// Shared `overlapping`, identical for every backend.
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
    overlapping_from_first(b, first, bottom_sub)
}

/// [`overlapping`] with a hint for the LEADING item.
pub fn overlapping_hinted<B: StripBackend + ?Sized>(
    b: &B,
    top: f64,
    extent: f64,
    hint: &mut usize,
) -> Option<Window> {
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

    let mut first = b.index_at_hinted(top, hint);
    if b.offset_sub(first).saturating_add(b.size_sub(first)) <= top_sub {
        first += 1;
    }
    overlapping_from_first(b, first, bottom_sub)
}

/// The tail half of an overlap query, leading item known.
fn overlapping_from_first<B: StripBackend + ?Sized>(
    b: &B,
    first: usize,
    bottom_sub: i64,
) -> Option<Window> {
    let len = b.len();
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
pub fn visible<B: StripBackend + ?Sized>(b: &B, scroll_top: f64, viewport: f64) -> Option<Window> {
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
    if b.is_empty() {
        return None;
    }
    let vh = viewport.max(0.0);

    if vh == 0.0 {
        return (scroll_top < b.total()).then(|| Window {
            first: b.index_at(scroll_top),
            last: b.index_at(scroll_top),
        });
    }

    let look = budget.overscan.padding(vh, b.mean_size());
    let padded = overlapping(b, scroll_top - look, vh + 2.0 * look)?;
    // What is strictly on screen must survive any trim.
    let vis = visible(b, scroll_top, vh);
    Some(trim_to_budget(padded, vis, budget.max_items))
}

/// Shared `window_hinted`: [`window`] with a per-frame hint.
pub fn window_hinted<B: StripBackend + ?Sized>(
    b: &B,
    scroll_top: f64,
    viewport: f64,
    budget: Budget,
    hint: &mut usize,
) -> Option<Window> {
    if b.is_empty() {
        return None;
    }
    let vh = viewport.max(0.0);

    if vh == 0.0 {
        return (scroll_top < b.total()).then(|| {
            let index = b.index_at_hinted(scroll_top, hint);
            Window {
                first: index,
                last: index,
            }
        });
    }

    let look = budget.overscan.padding(vh, b.mean_size());
    let padded = overlapping_hinted(b, scroll_top - look, vh + 2.0 * look, hint)?;
    // What is strictly on screen must survive any trim.
    let vis = visible(b, scroll_top, vh);
    Some(trim_to_budget(padded, vis, budget.max_items))
}

/// The one budget trim every windowing path answers identically.
fn trim_to_budget(padded: Window, vis: Option<Window>, max_items: usize) -> Window {
    let max = max_items.max(1);
    let mut first = padded.first;
    let mut last = padded.last;
    let Some(vis) = vis else {
        return Window { first, last };
    };
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
    Window { first, last }
}

pub mod strip;

pub use strip::Strip;

#[cfg(test)]
mod tests {
    use super::*;

    /// The trait's primitive half only, no hinted overrides.
    struct Bare(Strip);

    impl Bare {
        fn new(sizes: impl IntoIterator<Item = f64>, gap: f64) -> Self {
            Self(Strip::new(sizes, gap))
        }
    }

    impl StripBackend for Bare {
        fn len(&self) -> usize {
            self.0.len()
        }

        fn gap_sub(&self) -> i64 {
            to_sub(self.0.gap())
        }

        fn offset_sub(&self, index: usize) -> i64 {
            to_sub(self.0.offset(index))
        }

        fn size_sub(&self, index: usize) -> i64 {
            to_sub(self.0.size(index))
        }

        fn total_sub(&self) -> i64 {
            to_sub(self.0.total())
        }

        fn index_at_sub(&self, p: i64) -> usize {
            self.0.index_at(from_sub(p))
        }

        fn set_size_sub(&mut self, index: usize, new_sub: i64) -> i64 {
            to_sub(self.0.set_size(index, from_sub(new_sub)))
        }
    }

    #[test]
    fn a_backend_without_overrides_gets_the_hinted_defaults_for_free() {
        let bare = Bare::new([100.0, 200.0, 150.0, 100.0, 200.0], 24.0);
        // The default index_at_hinted answers like index_at and records it.
        let want = bare.index_at(300.0);
        let mut hint = 0usize;
        assert_eq!(StripBackend::index_at_hinted(&bare, 300.0, &mut hint), want);
        assert_eq!(hint, want);
        // And the default window_hinted agrees with the unhinted window.
        let budget = Budget::screenfuls(0.5, 4);
        let mut hint = 0usize;
        let mut top = 0.0;
        while top < bare.total() {
            assert_eq!(
                StripBackend::window_hinted(&bare, top, 300.0, budget, &mut hint),
                window(&bare, top, 300.0, budget),
                "default hinted window disagrees at top={top}"
            );
            top += 47.0;
        }
    }
}
