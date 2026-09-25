//! The gloss domain: the word card's geometry and spring, and the persisted
//! gloss mark whose [`mark::PageAnchor`] says where a mark sits in the
//! document without the AI feature knowing anything about the page.
//!
//! Pure — no wasm, no DOM, no leptos — unit-testable on the host via
//! `cargo test -p ai-core gloss`.

pub mod geometry;
pub mod mark;

pub use geometry::{
    GlossBox, MAX_CARD_H_FRAC, MIN_CARD_H, MIN_CARD_W, boxes_close, is_glossable, is_hintable,
    place_card, step_spring,
};
pub use mark::{GlossMark, PageAnchor, ReflowSpot, mark_id};
