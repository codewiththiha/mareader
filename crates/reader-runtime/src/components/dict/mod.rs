//! The dictionary's cards: the hover over a red word, the selection's ask.

pub mod hover_card;

use ai_core::gloss::{PageAnchor, ReflowSpot};

/// An explicit ask for the card: the selection's own anchor, not a hover's.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DictOpen {
    pub word: String,
    pub context: String,
    /// Where the selection is, in a gloss mark's space; `None` with
    /// no anchor.
    pub anchor: Option<PageAnchor>,
    /// The reflow spot the anchor resolves through, when there is one.
    pub spot: Option<ReflowSpot>,
}
