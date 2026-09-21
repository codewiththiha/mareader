//! The window-event table's import path in the shell.
//!
//! The table itself lives in `ui-kit` (`crates/ui-kit/src/events.rs`):
//! the reader build shares the vocabulary, and Phase 2's pane events are
//! declared beside these rather than in a second table. The names are
//! re-exported unchanged, so the shell's callers keep reading
//! `crate::events`.

pub use ui_kit::events::*;
