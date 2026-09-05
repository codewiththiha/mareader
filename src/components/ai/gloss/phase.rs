//! The two phase machines of the word card: what its BOX is doing
//! ([`GlossPhase`]) and what its DATA is doing ([`AiPhase`]).
//!
//! They lived in the AI feature's old `types` module, a file whose own doc had
//! to explain that the wire types were no longer in it and were only
//! re-exported "so the existing import paths keep resolving". Both enums are read exclusively by
//! this module — the card's geometry, its content, the chunk hook and the
//! surface that composes them — and neither is a wire type, so they belong here
//! with their only consumers, and the wire types are imported from
//! `ai_core::types` directly. That is what deleted the shim: nothing was left
//! in it that was not either someone else's type or these two.
//!
//! The two are deliberately orthogonal. A card can be streaming text while its
//! box is still sprung open, or folded back onto the stroke while the answer it
//! shows is complete; conflating them is what would make the morph and the
//! stream fight over one state.

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
