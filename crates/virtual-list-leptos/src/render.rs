//! The render contract: what the adapter hands to the view layer.

/// The lifecycle state of one rendered item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VirtualItemState {
    /// Inside the active mount window, carrying real content.
    #[default]
    Active,
    /// Mounted but outside the band: a placeholder at the laid-out size.
    Blank,
    /// Outside the window, retained briefly; expires on its own.
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
