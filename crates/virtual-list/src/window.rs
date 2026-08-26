//! Shared windowing types: the mounted range, the viewport, and the
//! overscan/budget policy that decides how much to keep warm around it.

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

    /// Smallest window containing both `self` and `other` (union hull).
    /// Used for pinning: selection pages / dominant-page pins extend the
    /// mount window without touching the overscan math.
    #[inline]
    pub const fn union(self, other: Window) -> Window {
        Window {
            first: if self.first < other.first {
                self.first
            } else {
                other.first
            },
            last: if self.last > other.last {
                self.last
            } else {
                other.last
            },
        }
    }
}

impl IntoIterator for Window {
    type Item = usize;
    type IntoIter = core::ops::RangeInclusive<usize>;

    fn into_iter(self) -> Self::IntoIter {
        self.first..=self.last
    }
}

/// The extents of the visible scrollport.
///
/// `main` is the extent along the scroll axis (height for a vertical list,
/// width for a horizontal one); `cross` is the extent across it. Plain lists
/// ignore `cross`; responsive grids resolve their column count from it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    /// Extent along the scroll axis.
    pub main: f64,
    /// Extent across the scroll axis (0 if unknown / irrelevant).
    pub cross: f64,
}

impl Viewport {
    /// Both extents known.
    pub const fn new(main: f64, cross: f64) -> Self {
        Self { main, cross }
    }

    /// Scroll-axis extent only.
    pub const fn main_only(main: f64) -> Self {
        Self { main, cross: 0.0 }
    }
}

impl From<f64> for Viewport {
    fn from(main: f64) -> Self {
        Self::main_only(main)
    }
}

/// How far past the viewport to keep mounted.
///
/// The policy is deliberately blind to *which* items it warms: it produces a
/// symmetric pixel padding, and the window builder decides membership with
/// two hard invariants — every partly-visible item is always mounted, and
/// trimming evicts the item furthest from the viewport first (preferring to
/// keep the item below, in reading direction). Callers never say "render N
/// above and M below"; they say how much slack they can afford, and the
/// top/below split falls out of wherever the reader is scrolled.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Overscan {
    /// A multiple of the viewport extent (zoom-invariant). The right default
    /// for expensive cells (canvases, rasters): one screenful ahead means
    /// the same thing at any item size or zoom.
    Screenfuls(f64),
    /// A fixed number of items — or, for row-windowed grids, rows. The right
    /// choice for cheap, uniform cells (thumbnails): "pre-mount exactly two
    /// rows" regardless of viewport height.
    Items(usize),
    /// A fixed pixel distance.
    Px(f64),
}

impl Overscan {
    /// Resolve to a concrete pixel padding. `unit_hint` is the layout's
    /// average item (or row) extent, used by [`Overscan::Items`].
    pub fn padding(self, viewport: f64, unit_hint: f64) -> f64 {
        match self {
            Self::Screenfuls(f) => f.max(0.0) * viewport.max(0.0),
            Self::Items(n) => n as f64 * unit_hint.max(0.0),
            Self::Px(px) => px.max(0.0),
        }
    }
}

/// How much to keep mounted around the viewport.
///
/// Two knobs, orthogonal by design:
///
/// - [`overscan`](Self::overscan) — how much slack around the visible range;
/// - [`max_items`](Self::max_items) — a hard ceiling on mounted count, which
///   only ever trims items that are **not** visible (for grids it counts
///   *rows*, since a row's cells mount and unmount together).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Budget {
    /// Read-around policy. See [`Overscan`].
    pub overscan: Overscan,
    /// Hard ceiling on mounted items (rows, for grids). `0` behaves as `1`.
    pub max_items: usize,
}

impl Budget {
    /// Screenful-based budget.
    pub const fn screenfuls(screenfuls: f64, max_items: usize) -> Self {
        Self {
            overscan: Overscan::Screenfuls(screenfuls),
            max_items,
        }
    }

    /// Fixed item/row budget (thumbnail grids: `Budget::items(2, …)`).
    pub const fn items(items: usize, max_items: usize) -> Self {
        Self {
            overscan: Overscan::Items(items),
            max_items,
        }
    }
}

impl Default for Budget {
    fn default() -> Self {
        Self::screenfuls(0.5, 5)
    }
}

/// Alignment for scroll-to commands (consumed by the framework adapter).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Align {
    /// Keep the target visible if it already is; otherwise scroll the
    /// nearest edge into view.
    #[default]
    Auto,
    /// Align the target's leading edge with the viewport's.
    Start,
    /// Center the target in the viewport.
    Center,
    /// Align the target's trailing edge with the viewport's.
    End,
}
