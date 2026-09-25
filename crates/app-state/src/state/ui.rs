//! The UI chrome slice of the old `AppState`, plus the appearance signal
//! alias. The reader and library state slices live beside this module; the
//! runtime-scoped contexts that bundle them live in the runtime crates.

use std::sync::atomic::{AtomicU64, Ordering};

use leptos::prelude::{Memo, RwSignal};

use reader_core::appearance::Appearance;

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
    /// `library_runtime::services::toast` and the document-open failure paths.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            id: TOAST_ID.fetch_add(1, Ordering::Relaxed),
            message: message.into(),
        }
    }
}

/// Which sidebar panel is open. UI chrome state, not viewer state:
/// reader-side rendering receives it as a plain signal when it needs to know
/// and never owns it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarMode {
    None,
    Outline,
    Thumbs,
}

/// UI chrome state: the sidebar, the toast surface, and the window flag the
/// frameless captions read.
#[derive(Clone, Copy)]
pub struct UiState {
    pub sidebar: RwSignal<SidebarMode>,
    pub toast: RwSignal<Option<Toast>>,
    /// Whether the window is maximized — the frameless caption cluster's
    /// maximize/restore glyph. Written by the app-lifetime window-state bridge
    /// (crates/app-ui/src/window_bridge.rs), never by the cluster itself: the state changes
    /// under it by more than its own button (snapping, taskbar restores,
    /// drag-to-edge), all of which resize the window.
    pub window_maximized: RwSignal<bool>,
}
