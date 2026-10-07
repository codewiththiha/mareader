//! The OS import drop: files land in the LIBRARY, never the reader.

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
            // An internal drag is reported with no paths.
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

/// The dropped paths the app can open.
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
