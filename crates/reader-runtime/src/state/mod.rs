//! Reader-level reactive state, one file per domain.

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

// Only the names the app reaches for by their short path are
// re-exported.
pub use ai::{AiSelectionState, SelectionDetail};
pub use document::{DocumentState, NO_DOCUMENT, ReflowContent};
pub use gloss::GlossState;
pub use search::SearchState;
pub use viewer::ViewerSignals;
pub use zoom::{ZoomCommand, ZoomTransition};

/// Page-host texture, provided by the pane realm.
pub type TextureSignal = leptos::prelude::Memo<TextureMode>;

/// The typography signal the pages and the stream read.
pub type TypographySignal = leptos::prelude::Memo<TextSettings>;

/// The reader's slice of app state; sidebar chrome is not here.
#[derive(Clone, Copy)]
pub struct ReaderState {
    /// The pane this state belongs to: its gate and its document session.
    pub pane: crate::pane::handle::PaneHandle,
    pub document: DocumentState,
    pub viewer: ViewerSignals,
    pub search: SearchState,
    pub ai_selection: AiSelectionState,
    pub gloss: GlossState,
    /// The pane's root element and host-given box.
    pub dom: crate::pane::dom::PaneDom,
    /// The pane's reflow measurement inbox.
    pub measure: crate::effects::reader::reflow_measure::MeasureInbox,
}

impl ReaderState {
    /// A pane's fresh reader state.
    pub fn new(pane: crate::pane::handle::PaneHandle) -> Self {
        Self {
            pane,
            document: DocumentState::default(),
            viewer: ViewerSignals::default(),
            search: SearchState::default(),
            ai_selection: AiSelectionState::default(),
            gloss: GlossState::default(),
            dom: crate::pane::dom::PaneDom::default(),
            measure: crate::effects::reader::reflow_measure::MeasureInbox::default(),
        }
    }

    /// True while a reflowable document is open; TRACKED.
    pub fn reflowable(&self) -> bool {
        self.document.format.get().is_reflowable()
    }

    /// The same question for an effect or a callback that must not subscribe.
    pub fn reflowable_now(&self) -> bool {
        self.document.format.get_untracked().is_reflowable()
    }

    /// The open format, tracked, for callers that need it apart.
    pub fn format(&self) -> Format {
        self.document.format.get()
    }

    /// True while a reflowable document is read in the continuous stream.
    pub fn reflow_streaming(&self) -> bool {
        self.reflowable() && self.viewer.mode.get() == ViewMode::ScrollVertical
    }

    /// The stream's reading position as a rounded percentage.
    pub fn stream_percent(&self) -> u32 {
        let top = self.viewer.scroll_top.get();
        let (_, viewport_h) = self.viewer.container_size.get();
        let total = self.document.content.reflow.stream_total.get();
        (reader_core::view::scroll_fraction(top, total, viewport_h) * 100.0).round() as u32
    }

    /// The stream's reading position as 0..=1, or `None`.
    pub fn stream_fraction(&self) -> Option<f64> {
        self.document.content.reflow.stream_fraction()
    }
}
