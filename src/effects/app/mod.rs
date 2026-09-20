//! App-level effects: browser window concerns (drag-drop, global
//! shortcuts), the theme applier, the motion preferences and the library's
//! two automatic moments.

#[cfg(feature = "library")]
pub mod drag_drop;
#[cfg(feature = "library")]
pub mod library;
pub mod motion;
pub mod shortcuts;
pub mod theme;
pub mod typography;
