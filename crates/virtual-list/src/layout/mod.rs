//! The [`Layout`] contract: one geometry engine behind any virtualized
//! surface.

mod grid;
mod list;

pub use grid::{GridColumns, GridLayout, GridSpec};
pub use list::ListLayout;

use crate::{Budget, Viewport, Window};

/// One geometry engine behind a virtualized surface.
pub trait Layout {
    /// Number of items.
    fn item_count(&self) -> usize;

    /// Whether there are no items.
    fn is_empty(&self) -> bool {
        self.item_count() == 0
    }

    /// Total extent of the content along the scroll axis. `0.0` when empty.
    fn total(&self) -> f64;

    /// Offset of the item's leading edge along the scroll axis.
    fn offset(&self, index: usize) -> f64;

    /// Extent of the item along the scroll axis. `0.0` out of range.
    fn size(&self, index: usize) -> f64;

    /// Offset of the item along the cross axis.
    fn cross_offset(&self, index: usize) -> f64;

    /// Extent of the item along the cross axis (`0.0` for plain lists).
    fn cross_size(&self, index: usize) -> f64;

    /// Index of the item whose span contains `pos` (leading-edge semantics).
    fn index_at(&self, pos: f64) -> usize;

    /// [`index_at`](Self::index_at) with the previous frame's answer as a
    /// hint — amortized `O(1)` for continuous scrolling.
    fn index_at_hinted(&self, pos: f64, hint: &mut usize) -> usize;

    /// Items overlapping the half-open span `[top, top + extent)`.
    fn overlapping(&self, top: f64, extent: f64) -> Option<Window>;

    /// Items at least partly on screen. Shorthand for `overlapping` with the
    /// raw viewport extent.
    fn visible(&self, top: f64, extent: f64) -> Option<Window> {
        self.overlapping(top, extent)
    }

    /// Items to keep mounted: visible + overscan, trimmed to the budget.
    fn window(&self, scroll: f64, viewport: Viewport, budget: Budget) -> Option<Window>;

    /// [`window`](Self::window) with a per-frame hint (amortized `O(1)`).
    fn window_hinted(
        &self,
        scroll: f64,
        viewport: Viewport,
        budget: Budget,
        hint: &mut usize,
    ) -> Option<Window>;

    /// The item occupying most of the viewport, ties to the lower index.
    fn dominant(&self, scroll: f64, extent: f64) -> usize;

    /// Resize one item; returns the signed delta for the anchor.
    fn set_size(&mut self, index: usize, new_size: f64) -> f64;

    /// Average item extent along the scroll axis — resolves
    /// [`crate::Overscan::Items`] without external knowledge.
    fn item_size_hint(&self) -> f64;
}

/// Either layout behind one handle, without `dyn` dispatch.
#[derive(Debug, Clone, PartialEq)]
pub enum LayoutKind {
    /// A single-column (or single-row) strip of variably-sized items.
    List(ListLayout),
    /// A uniform multi-column grid, windowed per row.
    Grid(GridLayout),
}

impl Layout for LayoutKind {
    fn item_count(&self) -> usize {
        match self {
            Self::List(l) => l.item_count(),
            Self::Grid(g) => g.item_count(),
        }
    }

    fn total(&self) -> f64 {
        match self {
            Self::List(l) => l.total(),
            Self::Grid(g) => g.total(),
        }
    }

    fn offset(&self, index: usize) -> f64 {
        match self {
            Self::List(l) => l.offset(index),
            Self::Grid(g) => g.offset(index),
        }
    }

    fn size(&self, index: usize) -> f64 {
        match self {
            Self::List(l) => l.size(index),
            Self::Grid(g) => g.size(index),
        }
    }

    fn cross_offset(&self, index: usize) -> f64 {
        match self {
            Self::List(l) => l.cross_offset(index),
            Self::Grid(g) => g.cross_offset(index),
        }
    }

    fn cross_size(&self, index: usize) -> f64 {
        match self {
            Self::List(l) => l.cross_size(index),
            Self::Grid(g) => g.cross_size(index),
        }
    }

    fn index_at(&self, pos: f64) -> usize {
        match self {
            Self::List(l) => l.index_at(pos),
            Self::Grid(g) => g.index_at(pos),
        }
    }

    fn index_at_hinted(&self, pos: f64, hint: &mut usize) -> usize {
        match self {
            Self::List(l) => l.index_at_hinted(pos, hint),
            Self::Grid(g) => g.index_at_hinted(pos, hint),
        }
    }

    fn overlapping(&self, top: f64, extent: f64) -> Option<Window> {
        match self {
            Self::List(l) => l.overlapping(top, extent),
            Self::Grid(g) => g.overlapping(top, extent),
        }
    }

    fn window(&self, scroll: f64, viewport: Viewport, budget: Budget) -> Option<Window> {
        match self {
            Self::List(l) => l.window(scroll, viewport, budget),
            Self::Grid(g) => g.window(scroll, viewport, budget),
        }
    }

    fn window_hinted(
        &self,
        scroll: f64,
        viewport: Viewport,
        budget: Budget,
        hint: &mut usize,
    ) -> Option<Window> {
        match self {
            Self::List(l) => l.window_hinted(scroll, viewport, budget, hint),
            Self::Grid(g) => g.window_hinted(scroll, viewport, budget, hint),
        }
    }

    fn dominant(&self, scroll: f64, extent: f64) -> usize {
        match self {
            Self::List(l) => l.dominant(scroll, extent),
            Self::Grid(g) => g.dominant(scroll, extent),
        }
    }

    fn set_size(&mut self, index: usize, new_size: f64) -> f64 {
        match self {
            Self::List(l) => l.set_size(index, new_size),
            Self::Grid(g) => g.set_size(index, new_size),
        }
    }

    fn item_size_hint(&self) -> f64 {
        match self {
            Self::List(l) => l.item_size_hint(),
            Self::Grid(g) => g.item_size_hint(),
        }
    }
}
