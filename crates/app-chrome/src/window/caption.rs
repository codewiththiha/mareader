//! The frameless window's caption cluster: minimize, maximize, close.

use leptos::prelude::*;

use crate::platform::is_linux;
use crate::window::caption_gnome::GnomeControls;
use crate::window::caption_windows::WindowsControls;

#[component]
pub fn WindowControls(
    /// The window's live maximized state, owned by the app.
    maximized: RwSignal<bool>,
) -> impl IntoView {
    if is_linux() {
        view! { <GnomeControls maximized=maximized /> }.into_any()
    } else {
        view! { <WindowsControls maximized=maximized /> }.into_any()
    }
}

/// Fire-and-forget caption commands, no-ops outside Tauri.
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
