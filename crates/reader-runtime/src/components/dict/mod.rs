//! The dictionary's cards: the hover over a red word, the selection's ask.

pub mod hover_card;

use ai_core::gloss::GlossBox;

/// An explicit ask for the card: the selection's own box, not a hover's.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DictOpen {
    pub word: String,
    pub context: String,
    /// The selection's viewport-space box at the ask.
    pub anchor: GlossBox,
}
