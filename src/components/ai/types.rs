//! The AI feature's phase types: what the card's *box* is doing
//! ([`GlossPhase`]) and what the explanation *data* is doing ([`AiPhase`]).
//!
//! The wire types (WordInfo, AiError, the chunk envelope, ...) are
//! format-agnostic and now live in `ai_core::types`; the data types are
//! re-exported here so the existing `crate::components::ai::types` import
//! paths keep resolving, and the chunk types ride in through
//! `crate::services::ai`.

/// Geometry of the word card. Orthogonal to [`AiPhase`] (the AI data status):
/// `AiPhase` says what the *data* is doing, `GlossPhase` says what the
/// *card's box* is doing. The two are decoupled so the card body can stream
/// text while the box independently expands / folds back onto the stroke.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GlossPhase {
    /// Exact-fit stroke hugging the selected word. No surface is mounted in
    /// this phase: the in-page highlighter's thinking pulse is the whole
    /// waiting UI.
    #[default]
    Processing,
    /// The full card, sprung open from the word.
    Expanded,
    /// Folded back onto the word (scroll-to-close) — a chip the reader can
    /// hover/tap to re-open in place.
    Compact,
}

/// The data status of the AI explanation: what the *content* of the card is
/// doing, independent of the card's geometry phase ([`GlossPhase`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AiPhase {
    /// No AI activity. The card is closed.
    #[default]
    Idle,
    /// The AI request has been sent; waiting for the first token. Only the
    /// highlighter stroke's processing animation is on screen.
    Processing,
    /// Tokens are arriving. The card is open and text is streaming in; each
    /// snapshot PATCHES the sections in place (a fade-in runs once, on
    /// mount, never per chunk).
    Streaming,
    /// All data received. Final state.
    Done,
    /// Something went wrong.
    Error,
}

pub use ai_core::types::{AiError, AiErrorKind, WordInfo};
