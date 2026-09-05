//! Reader-level reactive state, one file per domain: the document, the viewer
//! signals, the zoom pipeline's shape, search, the AI text selection and the
//! gloss marks. Pure UI chrome (sidebar, toast) lives in `state::app`;
//! pure domain logic in `reader-core` and the format crates.
//!
//! This module is the barrel. Everything below used to be one 650-line file,
//! which meant a page that only wanted `ViewerSignals` read the document, the
//! search state and the gloss marks to find it, and any change to any of them
//! touched the same file. The domains have nothing to say to each other beyond
//! the struct at the bottom that owns one of each — with the single exception
//! of the format questions the chrome keeps asking, which is exactly why they
//! are answered here, once, as [`ReaderState::reflowable`].

pub mod ai;
pub mod document;
pub mod gloss;
pub mod search;
pub mod viewer;
pub mod zoom;

use leptos::prelude::{Get, GetUntracked};

use reader_core::appearance::TextureMode;
use reader_core::format::Format;
use reader_core::view::ViewMode;
use reflow_core::typography::TextSettings;

// Only the names the app actually reaches for by their short path are
// re-exported; the rest are reached through their own module, which is the
// point of the split.
pub use ai::{AiSelectionState, SelectionDetail};
pub use document::{DEFAULT_PAGE_ASPECT, DocumentState, NO_DOCUMENT, ReflowContent};
pub use gloss::GlossState;
pub use search::SearchState;
pub use viewer::{Motion, ViewerSignals};
pub use zoom::{ZoomCommand, ZoomTransition};

/// Page-host texture, provided via Leptos context by the app shell (derived
/// from settings). The page canvases and the reflowable page hosts read it to
/// pick their `texture-*` class; neither ever touches settings.
pub type TextureSignal = leptos::prelude::Memo<TextureMode>;

/// The reflowable formats' typography, provided via context by the app
/// bootstrap (derived from settings) — the same pattern [`TextureSignal`] uses,
/// for the same reason: the pages, the measure column and the stream all need
/// the resolved knobs, and none of them may reach into settings to get them.
pub type TypographySignal = leptos::prelude::Memo<TextSettings>;

/// The reader's slice of app state: everything the format components and the
/// reader effects read/write. Sidebar/UI chrome is deliberately NOT here — it
/// is app chrome state, passed in explicitly where the reader needs it.
#[derive(Clone, Copy, Default)]
pub struct ReaderState {
    pub document: DocumentState,
    pub viewer: ViewerSignals,
    pub search: SearchState,
    pub ai_selection: AiSelectionState,
    pub gloss: GlossState,
}

impl ReaderState {
    /// True while a reflowable document (plain text, Markdown) is open.
    ///
    /// TRACKED: a document of the other kind swapping in re-renders the caller,
    /// which is what lets a page host, a `<Show>` and a disabled settings row
    /// all answer the same question without any of them learning what a file
    /// extension is. This is the only yes/no the reader answers about format; a
    /// new format means one more arm on `Format::is_reflowable`, not one more
    /// `if` in the viewer.
    pub fn reflowable(&self) -> bool {
        self.document.format.get().is_reflowable()
    }

    /// The same question for an effect or a callback that must not subscribe.
    pub fn reflowable_untracked(&self) -> bool {
        self.document.format.get_untracked().is_reflowable()
    }

    /// The open format, tracked, for the few callers that need to tell the
    /// formats apart rather than merely ask whether type reflows: the block
    /// renderer, which paints Markdown as Markdown and text as text, and the two
    /// reflow effects that decide whether they take part at all.
    pub fn format(&self) -> Format {
        self.document.format.get()
    }

    /// True while a reflowable document is being read in the continuous
    /// stream — the one view mode whose reading is not paging. The chrome
    /// asks this before showing anything page-shaped (the indicator, the
    /// bottom bar's controls, the settings rows that would lie).
    pub fn reflow_streaming(&self) -> bool {
        self.reflowable() && self.viewer.mode.get() == ViewMode::ScrollVertical
    }

    /// The stream's reading position as a rounded percentage of the whole
    /// document. Reads `scroll_top` and `container_size` TRACKED, so a derived
    /// signal around it updates with every scroll tick; the extent itself is
    /// read once, off the stream's own total (the scroll offset is the thing
    /// that moves, and it moves through `scroll_top`).
    pub fn stream_percent(&self) -> u32 {
        let top = self.viewer.scroll_top.get();
        let (_, viewport_h) = self.viewer.container_size.get();
        let total = self.document.content.reflow.stream_total.get();
        let extent = (total - viewport_h).max(1.0);
        ((top / extent) * 100.0).round().clamp(0.0, 100.0) as u32
    }

    /// The stream's reading position as 0..=1, or `None` while no stream is
    /// mounted. Purely a snapshot: persistence calls it inside its effect,
    /// where the tracked reads that matter (the page, the mode) already run.
    pub fn stream_fraction(&self) -> Option<f64> {
        self.document.content.reflow.stream_fraction()
    }
}
