//! Shared UI chrome signals; reader/library data lives in their runtimes.

use std::sync::atomic::{AtomicU64, Ordering};

use leptos::prelude::{Memo, RwSignal};

use reader_core::appearance::Appearance;
use reader_core::settings::AnimationSettings;

/// The appearance slice as its own tracked value, so unrelated writes
/// repaint nothing.
pub type AppearanceSignal = Memo<Appearance>;

/// Monotonic toast ids, so a stale dismiss timer never wipes a newer toast.
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

/// Which sidebar panel is open: UI chrome state, not viewer state.
pub enum SidebarMode {
    None,
    Outline,
    Thumbs,
    /// The Library panel: the library as a tree, and the workspace's open tabs.
    Library,
}

/// Which of the reader's motions animate, projected from the settings.
pub struct Motion {
    /// The rail animates its open/close: docked tweens width, floating fades.
    pub sidebar_slide: bool,
    /// The page rides a window drag: the canvas flexes on every frame.
    pub canvas_resize: bool,
    /// A zoom eases to its target over the profile's duration.
    pub zoom: bool,
    /// A jump to a page glides the column (or the thumbnail rail) over it.
    pub scroll_glide: bool,
}

impl Motion {
    /// The one place the master switch is honoured: off means nothing animates.
    pub const fn from_prefs(p: &AnimationSettings) -> Self {
        Self {
            sidebar_slide: p.enabled && p.sidebar_slide,
            canvas_resize: p.enabled && p.canvas_resize,
            zoom: p.enabled && p.zoom,
            scroll_glide: p.enabled && p.scroll_jumps,
        }
    }
}

impl Default for Motion {
    /// Everything moves; a reader not yet published to must still look whole.
    fn default() -> Self {
        Self {
            sidebar_slide: true,
            canvas_resize: true,
            zoom: true,
            scroll_glide: true,
        }
    }
}

/// UI chrome state: the sidebar, the toast surface, the window flag.
pub struct UiState {
    pub sidebar: RwSignal<SidebarMode>,
    pub toast: RwSignal<Option<Toast>>,
    /// Whether the window is maximized, for the caption's
    /// maximize/restore glyph.
    pub window_maximized: RwSignal<bool>,
}
