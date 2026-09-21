//! App-level state: settings, the reader slice, the library and the UI
//! chrome. Deliberately four groups — a flat grab-bag of signals grows
//! unbounded; these four are the app's real domains.

use std::sync::atomic::{AtomicU64, Ordering};

use leptos::prelude::{Memo, RwSignal};

use crate::state::library::LibraryState;
use crate::state::reader::ReaderState;
use reader_core::appearance::Appearance;
// The panel mode itself lives in `reader-core` (`reader_core::ui`): the
// reader's state and the reader build need it too, and this crate re-exports
// it at `crate::state::SidebarMode` so the shell's callers keep the short
// path.
use reader_core::settings::Settings;
use reader_core::ui::SidebarMode;

/// The appearance slice of the settings, as its own tracked value.
///
/// Every DOM-writing appearance consumer subscribes to THIS rather than to
/// `settings`: reading the whole signal subscribes to the whole blob, and the
/// blob is written for things that have nothing to do with the look — a layout
/// toggle, a gloss colour, `last_path` on every open — each of which used to
/// repaint every custom property on `<html>` and re-bake the engine's rasters.
/// A memo of the slice notifies only when the look changed.
pub type AppearanceSignal = Memo<Appearance>;

/// Monotonic toast ids: the host's equality guard needs a per-toast identity
/// so a stale auto-dismiss timer never wipes a newer toast.
static TOAST_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, PartialEq)]
pub struct Toast {
    pub id: u64,
    pub message: String,
}

impl Toast {
    /// A fresh toast. Producers reach it through
    /// `crate::services::library::toast` and the document-open failure paths.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            id: TOAST_ID.fetch_add(1, Ordering::Relaxed),
            message: message.into(),
        }
    }
}

/// UI chrome state: the sidebar, the toast surface, and the window flag the
/// frameless captions read.
#[derive(Clone, Copy)]
pub struct UiState {
    pub sidebar: RwSignal<SidebarMode>,
    pub toast: RwSignal<Option<Toast>>,
    /// Whether the window is maximized — the frameless caption cluster's
    /// maximize/restore glyph. Written by the app-lifetime window-state bridge
    /// (services/window.rs), never by the cluster itself: the state changes
    /// under it by more than its own button (snapping, taskbar restores,
    /// drag-to-edge), all of which resize the window.
    pub window_maximized: RwSignal<bool>,
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
                window_maximized: RwSignal::new(false),
            },
        }
    }
}
