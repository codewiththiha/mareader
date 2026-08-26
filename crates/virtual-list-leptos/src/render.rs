//! The render contract: what the adapter hands to the view layer.

/// How mounted items are positioned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Positioning {
    /// One spacer of [`total_size`](crate::Virtualizer::total_size); items are
    /// absolutely positioned at `item.start`. Best for canvas/raster cells.
    #[default]
    Absolute,
    /// A `(before, after)` spacer pair from [`padding`](crate::Virtualizer::padding);
    /// items stay in normal flow.
    Padding,
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
