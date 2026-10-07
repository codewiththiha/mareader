//! Library drag & drop: one pointer-driven session for every shelf move.

pub mod commit;
pub mod controller;
pub mod effect;
pub mod layer;
pub mod target;

/// Longer than the selection hold: crossing a shelf rests over cards
/// on the way.
pub const FOLD_DWELL_MS: i32 = 650;

/// Shorter than [`FOLD_DWELL_MS`]: a full-size ghost hides the crumb
/// it aims at.
pub const SINK_DWELL_MS: i32 = 420;
