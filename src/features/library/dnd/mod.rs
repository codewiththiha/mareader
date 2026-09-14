//! Library drag & drop: one pointer-driven session for every move a reader makes on the shelf.
//!
//! Nothing in here rides the browser's own drag-and-drop: once the engine takes a drag over,
//! `pointerup` never reaches the element the press began on, so the card's own "I am being
//! held" flag had no release to clear it.

pub mod commit;
pub mod controller;
pub mod effect;
pub mod layer;
pub mod target;

/// Longer than the hold that starts a selection (`crate::components::primitives::interactions::long_press::SELECT_PRESS_MS`), on purpose: a reader crossing a shelf on the way to somewhere else rests over cards.
pub const FOLD_DWELL_MS: i32 = 650;

/// The crumb is the only target smaller than the ghost hovering it, so it is the one place where
/// a full-size ghost hides the thing being aimed at. Shorter than [`FOLD_DWELL_MS`].
pub const SINK_DWELL_MS: i32 = 420;
