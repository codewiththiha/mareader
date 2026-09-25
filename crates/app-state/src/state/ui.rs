//! The UI chrome slice of the old `AppState`, plus the appearance signal
//! alias. The reader and library state slices live beside this module; the
//! runtime-scoped contexts that bundle them live in the runtime crates.

use std::sync::atomic::{AtomicU64, Ordering};

use leptos::prelude::{Memo, RwSignal};

use reader_core::appearance::Appearance;
use reader_core::settings::AnimationSettings;

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

/// Which of the reader's motions animate. Projected from the persisted
/// [`AnimationSettings`] by the app root
/// (`effects::app::motion::publish_motion`) and read by everything that moves
/// a page, so no consumer has to know a master switch exists — the projection
/// already applied it.
///
/// Read TRACKED by views (the rail's transition class must change when the
/// reader flips a switch) and UNTRACKED by effects and scroll calls: a flag
/// that stops something animating must not be what triggers the animation.
///
/// Nothing here skips a change: off renders the end frame in the frame the
/// change arrives, which is why freezing the reader loses no fit, no follow
/// and no scroll target.
///
/// Shared chrome, not reader state: the chrome slice hands a `Motion` signal
/// to the per-runtime `ChromeState`, and the reader's own viewer keeps the
/// live value beside its other signals.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Motion {
    /// The rail animates its open/close: the docked rail tweens its width
    /// (`SIDEBAR_SLIDE_MS`), the floating rail fades (`SIDEBAR_FADE_MS`).
    pub sidebar_slide: bool,
    /// The page rides a window drag: the canvas flexes on every frame of it.
    /// Riding the RAIL is not in here on purpose — a measured container is
    /// answered in the same frame, animation or not, and deferring it cropped
    /// the page for a visible instant.
    pub canvas_resize: bool,
    /// A zoom eases to its target over the profile's duration.
    pub zoom: bool,
    /// A jump to a page glides the column (or the thumbnail rail) over it.
    pub scroll_glide: bool,
}

impl Motion {
    /// The one place the master switch is honoured. Off, no detail can bring
    /// an animation back on; the Animations tab hides itself for the same
    /// reason, so the detail switches are never shown lying.
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
    /// Everything moves. The shell publishes the reader's prefs before
    /// anything can act on them, and a reader that has not been published to
    /// yet (a document opening, a test) must not look broken.
    fn default() -> Self {
        Self {
            sidebar_slide: true,
            canvas_resize: true,
            zoom: true,
            scroll_glide: true,
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
    /// (crates/app-ui/src/window_bridge.rs), never by the cluster itself: the state changes
    /// under it by more than its own button (snapping, taskbar restores,
    /// drag-to-edge), all of which resize the window.
    pub window_maximized: RwSignal<bool>,
}
