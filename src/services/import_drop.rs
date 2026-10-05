//! The OS import drop: files dragged onto the window from Finder / Explorer
//! land in the LIBRARY — and only while the library is the runtime on
//! screen. Over the reader a drop is nothing: the reader's splits are
//! dragged from its own Library panel, and an OS drag is never one of them.
//!
//! One listener set for the Shell's whole life, installed once at boot:
//! Tauri's `tauri://drag-enter` / `drag-leave` / `drag-drop` (a native file
//! drag never reaches the DOM inside the webview, and a plain browser names
//! no paths — so there is nothing to listen to off Tauri). The Shell does
//! not import anything itself: it filters the paths to the formats the app
//! opens (`reader_core::format`, the one registry) and hands them to the
//! library frame (`ShellFrame::ImportFiles`), which imports them onto the
//! shelf it shows, exactly as its Add menu would.
//!
//! The returned signal is the drop hint the Shell paints over the window
//! while an admissible drag hovers the library.

use leptos::prelude::*;

use crate::state::ShellState;

pub fn install_import_drop(state: ShellState) -> RwSignal<bool> {
    let hover = RwSignal::new(false);
    #[cfg(target_arch = "wasm32")]
    {
        if !tauri_bridge::has_tauri() {
            return hover;
        }
        use app_state::tauri_listen::tauri_listen;
        let enter = state.clone();
        tauri_listen("tauri://drag-enter", move |ev: web_sys::Event| {
            // An internal drag (a text selection) is reported with no paths;
            // a drag of nothing the app opens shows nothing either.
            let admitted = enter.manager.library_on_screen() && !documents(&ev).is_empty();
            hover.set(admitted);
        });
        tauri_listen("tauri://drag-leave", move |_ev: web_sys::Event| {
            hover.set(false);
        });
        tauri_listen("tauri://drag-drop", move |ev: web_sys::Event| {
            hover.set(false);
            let paths = documents(&ev);
            if !paths.is_empty() {
                state.manager.import_dropped(paths);
            }
        });
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = state;
    hover
}

/// The dropped paths the app can open, from a Tauri v2 drag event's
/// `payload.paths`. Every access is guarded: a malformed event is an empty
/// list, never a panic.
#[cfg(target_arch = "wasm32")]
fn documents(ev: &web_sys::Event) -> Vec<String> {
    let value: &wasm_bindgen::JsValue = ev.as_ref();
    let paths = js_sys::Reflect::get(value, &"payload".into())
        .and_then(|payload| js_sys::Reflect::get(&payload, &"paths".into()))
        .ok()
        .filter(|paths| paths.is_array());
    let Some(paths) = paths else {
        return Vec::new();
    };
    js_sys::Array::from(&paths)
        .iter()
        .filter_map(|p| p.as_string())
        .filter(|p| reader_core::format::is_supported_path(p))
        .collect()
}
