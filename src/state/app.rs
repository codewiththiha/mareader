//! App-level state: settings, the reader slice, the library and the UI
//! chrome. Deliberately four groups — a flat grab-bag of signals grows
//! unbounded; these four are the app's real domains.

use leptos::prelude::RwSignal;

use crate::state::library::LibraryState;
use crate::state::ui::SidebarMode;
use crate::state::reader::ReaderState;
use pdf_core::settings::Settings;

#[derive(Debug, Clone, PartialEq)]
pub struct Toast {
    pub message: String,
}

/// UI chrome state: the sidebar and the toast surface.
#[derive(Clone, Copy)]
pub struct UiState {
    /// Which sidebar panel (if any) is open.
    pub sidebar: RwSignal<SidebarMode>,
    /// Current toast (if any), rendered by the app-root `ToastHost`.
    pub toast: RwSignal<Option<Toast>>,
}

#[derive(Clone, Copy)]
pub struct AppState {
    pub settings: RwSignal<Settings>,
    pub reader: ReaderState,
    pub library: LibraryState,
    pub ui: UiState,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            settings: RwSignal::new(Settings::default()),
            reader: ReaderState::default(),
            library: LibraryState::default(),
            ui: UiState {
                sidebar: RwSignal::new(SidebarMode::None),
                toast: RwSignal::new(None),
            },
        }
    }
}
