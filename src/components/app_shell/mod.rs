//! Reusable structural application shell: the title bar shell, the
//! collision-aware toolbar, the document titles, and the geometry metrics
//! that Rust layout code shares with the CSS classes.

pub mod adaptive_toolbar;
pub mod app_title_bar;
pub mod document_title;
pub mod floating_document_title;
pub mod overflow_row;
pub mod title_bar;
pub mod toolbar_overflow;
pub mod toolbar_popover;
pub mod traffic_lights;
pub(crate) mod constants;

pub use overflow_row::OverflowRow;
