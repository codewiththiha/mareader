//! The frameless window's caption cluster: minimize / maximize-restore /
//! close, drawn by the webview where the platform config stripped the native
//! title bar (Windows and Linux — macOS keeps its traffic lights and never
//! mounts this; the app's titlebar owns that split).
//!
//! The two frameless desktops get their platform's own shape: Windows square
//! full-height buttons ([`caption_windows::WindowsControls`]), Linux
//! GNOME-style circles ([`caption_gnome::GnomeControls`]). [`WindowControls`]
//! picks per process — the platform is fixed for the webview's life
//! ([`crate::platform`]) — so the choice is a plain branch. Same three
//! commands, same glyph swap, same position; only shape and styling differ.
//!
//! The cluster lives INSIDE the toolbar row, so it shares the bar's
//! hover-reveal: like the native lights on macOS, the captions go with the bar
//! instead of floating over the page. The top hover band spans the full window
//! width, so the corner is never more than a pointer-away.
//!
//! The buttons carry no `data-tauri-drag-region`, which keeps them clickable
//! inside an otherwise draggable bar — Tauri only starts a drag from the
//! element owning the attribute. Double-clicking the bar still maximizes:
//! Tauri's injected drag script runs `internal_toggle_maximize` itself, the
//! same command behind the caption button.
//!
//! The maximize/restore glyph is a PROP, not a local guess: the app's
//! window-state bridge owns the live flag, because the state changes under
//! this cluster by more than its own button (snapping, drag-to-edge, taskbar
//! restores) and a page-scoped listener would die with the page that mounted
//! it. The cluster only fires commands.

use leptos::prelude::*;

use crate::platform::is_linux;
use crate::window::caption_gnome::GnomeControls;
use crate::window::caption_windows::WindowsControls;

#[component]
pub fn WindowControls(
    /// The window's live maximized state (owned by the app's UI state,
    /// written by the window-state bridge). Swaps the maximize/restore
    /// glyph.
    maximized: RwSignal<bool>,
) -> impl IntoView {
    if is_linux() {
        view! { <GnomeControls maximized=maximized /> }.into_any()
    } else {
        view! { <WindowsControls maximized=maximized /> }.into_any()
    }
}

/// Fire-and-forget caption commands. Each is a silent no-op outside Tauri
/// (the probes in [`crate::window::api`] own that), which is why plain browser
/// previews of the bar stay clickable.
pub(crate) fn minimize() {
    wasm_bindgen_futures::spawn_local(async move {
        crate::window::api::minimize_window().await;
    });
}

pub(crate) fn toggle_maximize() {
    wasm_bindgen_futures::spawn_local(async move {
        crate::window::api::toggle_maximize_window().await;
    });
}

pub(crate) fn close() {
    wasm_bindgen_futures::spawn_local(async move {
        crate::window::api::close_window().await;
    });
}
