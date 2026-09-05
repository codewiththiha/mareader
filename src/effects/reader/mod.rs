//! Reader effects: the reactive systems that keep the reader in sync
//! (scroll/page navigation sync, zoom sources, search, selection tracking,
//! reading progress, the layout preferences, the mode flip).

pub mod auto_scroll;
pub mod blend_backdrop;
pub mod layout_prefs;
pub mod reflow_layout;
pub mod reflow_outline;
pub mod zoom_watchers;
pub mod link_navigation;
pub mod mode_change;
pub mod navigation_sync;
pub mod page_selection;
pub mod reading_progress;
pub mod search;
pub mod selection_tracking;
