//! Library drag & drop: one pointer-driven session for every move a reader
//! makes on the shelf.
//!
//! Nothing here rides the browser's own drag-and-drop: once the engine takes
//! a drag over, `pointerup` never reaches the element the press began on, so
//! a card's "I am being held" flag would have no release to clear it.

pub mod commit;
pub mod controller;
pub mod effect;
pub mod layer;
pub mod target;

/// Longer than the selection hold
/// (`ui_kit::interactions::long_press::SELECT_PRESS_MS`)
/// on purpose: a reader crossing a shelf rests over cards on the way past.
pub const FOLD_DWELL_MS: i32 = 650;

/// Shorter than [`FOLD_DWELL_MS`]: the crumb is the only target smaller than
/// the ghost hovering it, the one place a full-size ghost hides what is being
/// aimed at.
pub const SINK_DWELL_MS: i32 = 420;
