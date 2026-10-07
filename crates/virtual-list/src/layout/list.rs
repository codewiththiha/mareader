//! A single column of variably-sized items over a [`Strip`] backend.

use crate::{Budget, Strip, StripBackend, Viewport, Window};

use super::Layout;

/// A column of variably-sized items with a fixed gap.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ListLayout<B: StripBackend = Strip> {
    pub(crate) backend: B,
    pub(crate) gap: f64,
}

impl<B: StripBackend> ListLayout<B> {
    /// Build from explicit item sizes.
    /// Note: for the default `Strip` backend, this rebuilds the prefix-sum.
    pub fn new(sizes: impl IntoIterator<Item = f64>, gap: f64) -> Self
    where
        B: From<Strip>,
    {
        Self {
            backend: B::from(Strip::new(sizes, gap)),
            gap,
        }
    }

    /// Build a layout of `count` identically-sized items.
    pub fn uniform(count: usize, size: f64, gap: f64) -> Self
    where
        B: From<Strip>,
    {
        Self::new(core::iter::repeat_n(size, count), gap)
    }

    /// Build from a per-item estimate, refined later by measurement.
    pub fn estimated(count: usize, estimate: impl Fn(usize) -> f64, gap: f64) -> Self
    where
        B: From<Strip>,
    {
        Self::new((0..count).map(estimate), gap)
    }

    /// The gap between adjacent items.
    #[inline]
    pub fn gap(&self) -> f64 {
        self.gap
    }
}

impl<B: StripBackend> Layout for ListLayout<B> {
    #[inline]
    fn item_count(&self) -> usize {
        self.backend.len()
    }

    #[inline]
    fn total(&self) -> f64 {
        self.backend.total()
    }

    #[inline]
    fn offset(&self, index: usize) -> f64 {
        self.backend.offset(index)
    }

    #[inline]
    fn size(&self, index: usize) -> f64 {
        self.backend.size(index)
    }

    #[inline]
    fn cross_offset(&self, _index: usize) -> f64 {
        0.0
    }

    #[inline]
    fn cross_size(&self, _index: usize) -> f64 {
        0.0
    }

    #[inline]
    fn index_at(&self, pos: f64) -> usize {
        self.backend.index_at(pos)
    }

    fn index_at_hinted(&self, pos: f64, hint: &mut usize) -> usize {
        // The backend's hinted search, amortized O(1) like the grid's.
        self.backend.index_at_hinted(pos, hint)
    }

    fn overlapping(&self, top: f64, extent: f64) -> Option<Window> {
        crate::backend::overlapping(&self.backend, top, extent)
    }

    fn window(&self, scroll: f64, viewport: Viewport, budget: Budget) -> Option<Window> {
        crate::backend::window(&self.backend, scroll, viewport.main, budget)
    }

    fn window_hinted(
        &self,
        scroll: f64,
        viewport: Viewport,
        budget: Budget,
        hint: &mut usize,
    ) -> Option<Window> {
        self.backend
            .window_hinted(scroll, viewport.main, budget, hint)
    }

    #[inline]
    fn dominant(&self, scroll: f64, extent: f64) -> usize {
        crate::backend::dominant(&self.backend, scroll, extent)
    }

    #[inline]
    fn set_size(&mut self, index: usize, new_size: f64) -> f64 {
        self.backend.set_size(index, new_size)
    }

    #[inline]
    fn item_size_hint(&self) -> f64 {
        self.backend.mean_size()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Overscan;

    #[test]
    fn estimated_seeds_each_item_from_its_own_size() {
        let l: ListLayout = ListLayout::estimated(4, |i| [100.0, 300.0, 200.0, 100.0][i], 0.0);
        assert_eq!(l.offset(0), 0.0);
        assert_eq!(l.offset(1), 100.0);
        assert_eq!(l.offset(2), 400.0);
        assert_eq!(l.offset(3), 600.0);
        assert_eq!(l.total(), 700.0);
    }

    #[test]
    fn overscan_items_uses_the_mean_item_size() {
        let l: ListLayout = ListLayout::uniform(40, 100.0, 0.0);
        let a = l
            .window(2_000.0, Viewport::main_only(200.0), Budget::items(2, 100))
            .unwrap();
        let b = l
            .window(
                2_000.0,
                Viewport::main_only(200.0),
                Budget {
                    overscan: Overscan::Px(200.0),
                    max_items: 100,
                },
            )
            .unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn hinted_matches_unhinted() {
        // The hint runs, so the list owes the grid's guarantee.
        let l: ListLayout = ListLayout::estimated(200, |i| 80.0 + (i % 7) as f64 * 13.0, 11.0);
        let mut hint = 0usize;
        let mut pos = 0.0;
        while pos < l.total() {
            assert_eq!(
                l.index_at(pos),
                l.index_at_hinted(pos, &mut hint),
                "index_at {pos}"
            );
            pos += 17.0;
        }
        let budget = Budget::items(2, 64);
        let mut hint = 0usize;
        let mut top = 0.0;
        while top < l.total() {
            assert_eq!(
                l.window(top, Viewport::main_only(720.0), budget),
                l.window_hinted(top, Viewport::main_only(720.0), budget, &mut hint),
                "window at {top}"
            );
            top += 61.0;
        }
    }
}
