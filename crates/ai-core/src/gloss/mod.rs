//! The gloss domain: the card's geometry and spring, and the persisted mark.

pub mod geometry;
pub mod mark;

pub use geometry::{GlossBox, MAX_GLOSS_CHARS, boxes_close, is_glossable, place_card, step_spring};
pub use mark::{GlossMark, PageAnchor, ReflowSpot, capture_id, mark_id};
