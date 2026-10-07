//! The AI selection slice: what is highlighted and whether the card
//! is open.

use ai_core::gloss::{PageAnchor, ReflowSpot};
use leptos::prelude::*;
use serde::Deserialize;

/// The selection's viewport rect, the pill's warp window.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize)]
pub struct SelectionRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// What the AI feature knows about the selection.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct SelectionDetail {
    pub text: String,
    /// The sentence around the word, for the model.
    pub context: String,
    pub rect: SelectionRect,
    /// Which family painted the selection's host.
    #[serde(default)]
    pub host: Option<String>,
    /// The selection's durable identity in a reflowable document.
    #[serde(default)]
    pub spot: Option<ReflowSpot>,
}

impl SelectionDetail {
    /// Whether the host's document is reflowable.
    pub fn is_reflow(&self) -> bool {
        self.host.as_deref() == Some(app_state::dom_contract::HOST_REFLOW)
    }
}

/// Reactive state for the AI selection feature.
#[derive(Clone, Copy)]
pub struct AiSelectionState {
    /// The current selection details, or `None` if nothing is selected.
    pub detail: RwSignal<Option<SelectionDetail>>,
    /// The selection's origin, so the pill can follow it.
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
    /// Clear detail, anchor and the open flag.
    pub fn reset(&self) {
        let Self {
            detail,
            anchor,
            popover_open,
        } = *self;
        detail.set(None);
        anchor.set(None);
        popover_open.set(false);
    }
}
