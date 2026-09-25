//! The handles shared chrome code reads, per runtime instance.
//!
//! Chrome CODE (app-ui) is shared; chrome STATE is per-runtime: every runtime
//! creates its own `ChromeState` inside its instance and provides it (§3, §16).
//! Leptos signals never cross the wasm module boundary — runtime facts flow
//! through the shell's serializable commands (§14) — so each instance keeps
//! its own signals and the shell hears about them via events.

use leptos::prelude::{RwSignal, Signal};

use crate::state::ui::{Motion, UiState};
use reader_core::settings::Settings;

/// What chrome reads about the ACTIVE reader surface: reflow gating for the
/// appearance menu, search visibility for the titlebar, sidebar motion for
/// the shell controller.
#[derive(Clone, Copy)]
pub struct ReaderSurface {
    pub reflowable: Signal<bool>,
    pub search_visible: RwSignal<bool>,
    pub sidebar_slide: RwSignal<Motion>,
}

/// The settings + window/toast slice a runtime hands its chrome.
#[derive(Clone, Copy)]
pub struct ChromeState {
    pub settings: RwSignal<Settings>,
    pub ui: UiState,
    pub reader: ReaderSurface,
}
