//! The word card's two phase machines: box and data.

/// The card's box geometry, orthogonal to [`AiPhase`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GlossPhase {
    /// Exact-fit stroke hugging the word; no surface mounted.
    #[default]
    Processing,
    /// The full card, sprung open from the word.
    Expanded,
    /// Folded back onto the word (scroll-to-close).
    Compact,
}

/// What the card's content is doing, separate from its box.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AiPhase {
    /// No AI activity. The card is closed.
    #[default]
    Idle,
    /// The request is sent, first token pending.
    Processing,
    /// Tokens are arriving; snapshots patch sections in place.
    Streaming,
    /// All data received. Final state.
    Done,
    /// Something went wrong.
    Error,
}
