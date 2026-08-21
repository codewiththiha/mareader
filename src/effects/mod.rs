//! App-level effects. Viewer effects (fit/zoom, scroll, page tracking,
//! shortcuts, search) live in the `pdf-viewer` crate.

pub mod appearance_scrub;
pub mod drag_drop;
pub mod link_nav;
pub mod reading_progress;
pub mod selection_pages;
pub mod theme_applier;
