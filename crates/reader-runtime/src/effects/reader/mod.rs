//! Reader effect arms (from the unified tree): navigation, selection,
//! search, zoom, layout, measurement, and the shortcuts + document-drop
//! entry points the session installs.

pub mod auto_scroll;
pub mod blend_backdrop;
pub mod first_paint;
pub mod layout_prefs;
pub mod link_navigation;
pub mod mode_change;
pub mod navigation_sync;
pub mod page_selection;
pub mod reading_progress;
pub mod reflow_layout;
pub mod reflow_measure;
pub mod reflow_outline;
pub mod search;
pub mod selection_tracking;
pub mod shortcuts;
pub mod zoom_watchers;
