//! Reader effects: the reactive systems that keep the reader in sync
//! (scroll/page navigation sync, zoom sources, search, selection tracking,
//! the layout preferences, the mode flip). Reading progress — which writes
//! the library — stays in the shell (`src/effects/app`).

pub mod auto_scroll;
pub mod blend_backdrop;
pub mod first_paint;
pub mod layout_prefs;
pub mod link_navigation;
pub mod mode_change;
pub mod navigation_sync;
pub mod page_selection;
pub mod reflow_layout;
pub mod reflow_measure;
pub mod reflow_outline;
pub mod search;
pub mod selection_tracking;
pub mod zoom_watchers;
