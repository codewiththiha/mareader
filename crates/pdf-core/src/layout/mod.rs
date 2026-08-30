//! Reader layout policy shared across the app.
//!
//! Geometry lives in `virtual-list` and `virtual-list-leptos`; this module just
//! carries the reader's constants and view-mode enum.

pub use virtual_list::Budget;

/// Gap between pages in the continuous reader, in CSS px.
pub const PAGE_GAP: f64 = 24.0;

/// Height of the glass toolbar, in CSS px. The viewer scrollport spans the
/// full window height so pages travel under the translucent header; each view
/// offsets its content by this inset.
///
/// MUST stay in sync with Tailwind `h-12` / `mt-12` on the title bar and the
/// continuous page-list offset.
pub const TOOLBAR_H: f64 = 48.0;

/// Comfortable read-ahead: half a screenful each way, up to 3 mounted pages
/// total (visible + ~1 above + ~1 below). Each mounted page at 2× DPR plus
/// its raw is ~64MB worst case, so the ceiling is what keeps idle RAM sane.
pub const RENDER_BUDGET: Budget = Budget::screenfuls(0.5, 3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    Vertical,
    Horizontal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    /// One page at a time. Paginated.
    Single,
    /// Two pages side by side, no gap (a "spread"). Paginated.
    Spread,
    #[default]
    /// All pages in one vertical strip; wheel/keys scroll vertically.
    ScrollVertical,
    /// All pages in one horizontal strip; wheel/keys scroll horizontally.
    ScrollHorizontal,
}

impl ViewMode {
    pub fn all() -> [ViewMode; 4] {
        [
            ViewMode::Single,
            ViewMode::Spread,
            ViewMode::ScrollVertical,
            ViewMode::ScrollHorizontal,
        ]
    }

    /// Auto-scroll only makes sense on the two scrolling modes.
    pub fn can_scroll(self) -> bool {
        matches!(self, ViewMode::ScrollVertical | ViewMode::ScrollHorizontal)
    }

    pub fn is_paginated(self) -> bool {
        matches!(self, ViewMode::Single | ViewMode::Spread)
    }

    /// The scroll axis for the scrolling modes. The paginated modes have
    /// neither a strip nor a main axis, so they return `None`.
    pub fn axis(self) -> Option<Axis> {
        match self {
            ViewMode::ScrollVertical => Some(Axis::Vertical),
            ViewMode::ScrollHorizontal => Some(Axis::Horizontal),
            _ => None,
        }
    }
}
