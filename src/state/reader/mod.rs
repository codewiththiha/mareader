//! Reader-level reactive state, one file per domain: the document, the viewer
//! signals, the zoom pipeline's shape, search, the AI text selection and the
//! gloss marks. Pure UI chrome (sidebar, toast) lives in `state/ui` +
//! `state/app`; pure domain logic in `pdf-core`.
//!
//! This module is the barrel. Everything below used to be one 650-line file,
//! which meant a page that only wanted `ViewerSignals` read the document, the
//! search state and the gloss marks to find it, and any change to any of them
//! touched the same file. The six domains have nothing to say to each other
//! beyond the struct at the bottom that owns one of each.

pub mod ai;
pub mod document;
pub mod gloss;
pub mod search;
pub mod viewer;
pub mod zoom;

use leptos::prelude::Memo;

use pdf_core::appearance::TextureMode;

// Only the names the app actually reaches for by their short path are
// re-exported; the rest are reached through their own module, which is the
// point of the split.
pub use ai::{AiSelectionState, SelectionDetail};
pub use document::{DEFAULT_PAGE_ASPECT, DocumentState, NO_DOCUMENT};
pub use gloss::GlossState;
pub use search::SearchState;
pub use viewer::{Motion, ViewerSignals};
pub use zoom::{ZoomCommand, ZoomTransition};

/// Page-host texture, provided via Leptos context by the app shell (derived
/// from settings). `PageCanvas` reads it to pick the `texture-*` class; the
/// reader never touches settings.
pub type TextureSignal = Memo<TextureMode>;

/// The reader's slice of app state: everything the PDF components and the
/// reader effects read/write. Sidebar/UI chrome is deliberately NOT here —
/// it is app chrome state, passed in explicitly where the reader needs it.
#[derive(Clone, Copy, Default)]
pub struct ReaderState {
    pub document: DocumentState,
    pub viewer: ViewerSignals,
    pub search: SearchState,
    pub ai_selection: AiSelectionState,
    pub gloss: GlossState,
}
