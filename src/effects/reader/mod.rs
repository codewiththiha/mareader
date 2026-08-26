//! Reader effects: the reactive systems that keep the reader in sync
//! (scroll/page navigation sync, fit/zoom, search, selection tracking,
//! reading progress).

pub mod continuous_scroll;
pub mod fit_mode;
pub mod link_navigation;
pub mod nav_mode_flip;
pub mod navigation_sync;
pub mod page_selection;
pub mod reading_progress;
pub mod search;
pub mod text_selection;
pub mod zoom;
