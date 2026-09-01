//! The AI text-selection slice: what the reader highlighted, where it sits on
//! the page, and whether the explanation popover is open.

use leptos::prelude::*;
use serde::Deserialize;

use pdf_core::gloss::PageAnchor;

/// Bounding rectangle of the selected text, in viewport CSS pixels — the
/// "warp window" the AI selection menu anchors to.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct SelectionRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// Everything the AI feature needs about the current text selection, as
/// dispatched by the engine's `pdfreader:selection-detail` event.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SelectionDetail {
    /// The exact text the user highlighted.
    pub text: String,
    /// Surrounding sentence (~120 chars from the same text layer) so the
    /// model can disambiguate the word.
    pub context: String,
    /// Tight bounding box around the selection (the "warp window").
    pub rect: SelectionRect,
}

/// Reactive state for the AI text-selection feature: what is selected and
/// whether the explanation popover is open.
#[derive(Clone, Copy)]
pub struct AiSelectionState {
    /// The current selection details, or `None` if nothing is selected.
    pub detail: RwSignal<Option<SelectionDetail>>,
    /// The selection's origin in page space, so the Info pill can follow
    /// scroll and die when it leaves the viewport.
    pub anchor: RwSignal<Option<PageAnchor>>,
    /// Whether the "Info" popover is currently open.
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
    /// (a stale `popover_open = true` would hide the Info button and make
    /// the next open a no-op).
    pub fn reset(&self) {
        self.detail.set(None);
        self.anchor.set(None);
        self.popover_open.set(false);
    }
}
