//! The render contract: what the adapter hands to the view layer.

/// The lifecycle state of one rendered item.
///
/// Items normally render because they sit inside the active mount window.
/// When the window moves (a scroll fling, a zoom's geometry commit), an
/// item that just left can be **retained** for a short grace period as a
/// [`VirtualItemState::Zombie`]: it stays mounted at its laid-out position —
/// bridging the virtualization lifecycle across the change — but it is no
/// longer part of the window, never drives dominant-item selection, and a
/// renderer should not start new expensive work for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VirtualItemState {
    /// Inside the active mount window.
    #[default]
    Active,
    /// Outside the window, retained briefly so its DOM can outlive the
    /// window change that evicted it. Expires on its own.
    Zombie,
}

/// One mounted item, DOM-ready. All coordinates include `padding_start`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VirtualItem {
    /// Item index.
    pub index: usize,
    /// Main-axis offset for absolute positioning.
    pub start: f64,
    /// Main-axis extent.
    pub size: f64,
    /// Cross-axis offset (`x` for grids, `0.0` for lists).
    pub cross_start: f64,
    /// Cross-axis extent (`width` for grids, `0.0` for lists).
    pub cross_size: f64,
    /// The row this item belongs to (`index` for lists).
    pub row: usize,
    /// Whether the item is active or a retained zombie.
    pub state: VirtualItemState,
}

/// One mounted row — the render unit for grids. For lists, one row per item.
#[derive(Debug, Clone, PartialEq)]
pub struct VirtualRow {
    /// Row index.
    pub row: usize,
    /// Main-axis offset for absolute positioning (includes padding).
    pub start: f64,
    /// The item indices in this row.
    pub items: core::ops::Range<usize>,
}
