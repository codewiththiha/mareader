//! App-level effects: browser window concerns (drag-drop, global
//! shortcuts), the theme applier, the motion preferences, the library's
//! two automatic moments, and the reading-position write-back.
//!
//! `reading_progress` is here rather than with the reader's own effects
//! because of what it WRITES: the library's rows and their persisted blob. It
//! only reads reader signals, and the shell is the half that owns the
//! library.

pub mod drag_drop;
pub mod library;
pub mod motion;
pub mod reading_progress;
pub mod shortcuts;
pub mod theme;
pub mod typography;
