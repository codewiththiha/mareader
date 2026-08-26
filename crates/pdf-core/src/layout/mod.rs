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
pub enum ViewMode {
    Single,
    Continuous,
}
