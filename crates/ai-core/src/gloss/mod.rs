//! The gloss domain: the card's geometry and spring, and the persisted mark.

pub mod geometry;
pub mod mark;

pub use geometry::{GlossBox, boxes_close, is_glossable, is_hintable, place_card, step_spring};
pub use mark::{GlossMark, PageAnchor, ReflowSpot, capture_id, mark_id};
