//! App-level effects: browser window concerns (drag-drop, global
//! shortcuts), the theme applier, the motion preferences, reading progress
//! (the one reader effect that writes the library) and the library's two
//! automatic moments.

pub mod drag_drop;
pub mod library;
pub mod motion;
pub mod reading_progress;
pub mod shortcuts;
pub mod theme;
pub mod typography;
