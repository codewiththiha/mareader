//! The AI text-selection slice: what the reader highlighted, where it sits on
//! the page, and whether the explanation popover is open.

use ai_core::gloss::{PageAnchor, ReflowSpot};
use leptos::prelude::*;
use serde::Deserialize;

/// Bounding rectangle of the selected text, in viewport CSS pixels — the
/// "warp window" the AI selection pill anchors to.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct SelectionRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Everything the AI feature needs about the current text selection, as
/// dispatched by the engine's `pdfreader:selection-detail` event.
///
/// The two optional fields are the format half of the protocol, and both
/// default so a PDF's event — which carries neither — deserializes unchanged.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SelectionDetail {
    /// The exact text the user highlighted.
    pub text: String,
    /// Surrounding sentence (~120 chars from the same layer of the document) so
    /// the model can disambiguate the word.
    pub context: String,
    /// Tight bounding box around the selection (the "warp window").
    pub rect: SelectionRect,
    /// Which format family painted the host the selection is in
    /// (`"pdf"` / `"reflow"`), or `None` when it is in neither.
    #[serde(default)]
    pub host: Option<String>,
    /// The selection's durable identity in a reflowable document: the block and
    /// the character range inside it. `None` for a PDF, whose page-space rect
    /// already is its identity, and for a reflowable selection whose block the
    /// tracker could not identify — which then falls back to the rect.
    #[serde(default)]
    pub spot: Option<ReflowSpot>,
}

impl SelectionDetail {
    /// Whether the selection is in a reflowable document (plain text,
    /// Markdown). Decided by the host the tracker found rather than by the open
    /// document's format, so a selection that outlives a document switch cannot
    /// be anchored through the wrong pipeline.
    pub fn is_reflow(&self) -> bool {
        self.host.as_deref() == Some(crate::dom_contract::HOST_REFLOW)
    }
}

/// Reactive state for the AI text-selection feature: what is selected and
/// whether the explanation popover is open.
#[derive(Clone, Copy)]
pub struct AiSelectionState {
    /// The current selection details, or `None` if nothing is selected.
    pub detail: RwSignal<Option<SelectionDetail>>,
    /// The selection's origin in page space, so the Explain pill can follow
    /// scroll and die when it leaves the viewport.
    pub anchor: RwSignal<Option<PageAnchor>>,
    /// Whether the "Explain" popover is currently open.
    pub popover_open: RwSignal<bool>,
}

impl Default for AiSelectionState {
    fn default() -> Self {
        Self {
            detail: RwSignal::new(None),
            anchor: RwSignal::new(None),
            popover_open: RwSignal::new(false),
        }
    }
}

impl AiSelectionState {
    /// Clear selection detail, page anchor and the open flag. Called on
    /// document close so a card left open on PDF A cannot poison PDF B
    /// (a stale `popover_open = true` would hide the Explain button and make
    /// the next open a no-op).
    pub fn reset(&self) {
        self.detail.set(None);
        self.anchor.set(None);
        self.popover_open.set(false);
    }
}
