//! The handles shared chrome code reads, per runtime instance.

use leptos::prelude::{RwSignal, Signal};

use crate::state::ui::{Motion, UiState};
use reader_core::settings::Settings;

/// What chrome reads about the active surface: reflow gating, search, motion.
pub struct ReaderSurface {
    pub reflowable: Signal<bool>,
    pub search_visible: Signal<bool>,
    pub sidebar_slide: RwSignal<Motion>,
}

/// The settings + window/toast slice a runtime hands its chrome.
#[derive(Clone, Copy)]
pub struct ChromeState {
    pub settings: RwSignal<Settings>,
    pub ui: UiState,
    pub reader: ReaderSurface,
}
