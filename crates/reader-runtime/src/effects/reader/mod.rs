//! Reader effect arms: navigation, selection, search, zoom, layout, measure.

pub mod auto_scroll;
#[cfg(feature = "pdf")]
pub mod blend_backdrop;
pub mod first_paint;
pub mod layout_prefs;
pub mod link_navigation;
pub mod mode_change;
pub mod navigation_sync;
#[cfg(feature = "reflow")]
pub mod outline_jump;
pub mod page_selection;
pub mod reading_progress;
pub mod reflow_layout;
pub mod reflow_measure;
pub mod reflow_outline;
pub mod search;
pub mod selection_tracking;
pub mod shortcuts;
pub mod zoom_watchers;
