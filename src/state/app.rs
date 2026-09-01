//! App-level state: settings, the reader slice, the library and the UI
//! chrome. Deliberately four groups — a flat grab-bag of signals grows
//! unbounded; these four are the app's real domains.

use std::sync::atomic::{AtomicU64, Ordering};

use leptos::prelude::{Memo, RwSignal};

use crate::state::library::LibraryState;
use crate::state::ui::SidebarMode;
use crate::state::reader::ReaderState;
use pdf_core::appearance::Appearance;
use pdf_core::settings::Settings;

/// The appearance slice of the settings, as its own tracked value.
///
/// Every DOM-writing appearance consumer subscribes to THIS rather than to
/// `settings` directly. Reading the whole settings signal subscribes to the
/// whole blob, and the blob is written for things that have nothing to do
/// with the look — a layout toggle, a gloss colour, `last_path` on every
/// single document open. Each of those used to repaint every custom property
/// on `<html>` and re-bake the engine's rasters. A memo of the slice only
/// notifies when the look actually changed.
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
    /// A fresh error surface (the only toast producers today).
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            id: TOAST_ID.fetch_add(1, Ordering::Relaxed),
            message: message.into(),
        }
    }
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
