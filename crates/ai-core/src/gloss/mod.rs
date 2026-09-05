//! The gloss domain: the word card's geometry and spring, the persisted
//! gloss mark, and the [`mark::MarkAnchor`] trait that lets a document format
//! say where a mark sits without the AI feature knowing the format.
//!
//! Pure — no wasm, no DOM, no leptos — unit-testable on the host via
//! `cargo test -p ai-core gloss`.

pub mod geometry;
pub mod mark;

pub use geometry::{
    boxes_close, is_glossable, is_hintable, place_card, step_spring, GlossBox, MAX_CARD_H_FRAC,
    MIN_CARD_H, MIN_CARD_W,
};
pub use mark::{mark_id, GlossMark, MarkAnchor, PageAnchor, ReflowSpot};
