//! The frontend half of the AI word-explanation feature.
//!
//! Owns the app-lifetime side of the wire protocol:
//! [`install_ai_chunk_bridge`] registers ONE Tauri listener that
//! re-broadcasts each chunk as a window `CustomEvent` (`pdfreader:ai-chunk`),
//! and [`invoke_explain_word`] starts a run through `ai_core::bridge` (the
//! format-agnostic kickoff). The gloss popover (and anything else) listens
//! on the window, so document switches never stack dead Tauri handlers or
//! drop the live one.

pub use ai_core::types::{AiChunk, AiChunkEvent};
use leptos::task::spawn_local;
use wasm_bindgen::JsValue;

pub use crate::events::AI_CHUNK_EVENT;

/// Starts an `explain_word` run on the backend, tagged with `run`. The
/// streamed results arrive as `ai-stream-chunk` events carrying that same id,
/// re-broadcast by [`install_ai_chunk_bridge`].
pub fn invoke_explain_word(word: String, context: String, run: String) {
    spawn_local(async move {
        if let Err(e) = ai_core::bridge::explain_word(&word, &context, &run).await {
            web_sys::console::warn_1(&format!("[ai] explain_word invoke failed: {e}").into());
        }
    });
}

/// Register the Tauri `ai-stream-chunk` listener ONCE for the app's life and
/// re-broadcast every chunk as a window [`AI_CHUNK_EVENT`]. Survives every
/// document switch; per-mount UI only adds/removes a plain window listener.
///
/// This is deliberately the ONLY Tauri-side registration: a per-mount
/// listener would stack handlers whose closures die with the owner,
/// poisoning the dispatch chain on later document switches.
///
/// Must be called from inside the app reactive owner (e.g. `App`):
/// `tauri_listen` parks the JS closure in that owner, so the listener lives
/// exactly as long as the app does.
///
/// Outside Tauri (`trunk serve` in a plain browser) this is a no-op: there is
/// no `window.__TAURI__`, and the wasm-bindgen shim for `__TAURI__.event.listen`
/// walks that global chain eagerly, so calling it throws a TypeError. Because
/// this runs from the app root, that throw took the whole mount down with it —
/// the probe is the same one every other Tauri surface uses (see
/// `tauri_bridge::has_tauri` and `services::document::open::init_open_file_handling`).
/// With no backend there are no chunks to bridge, so skipping is the correct
/// behaviour, not a degraded one.
pub fn install_ai_chunk_bridge() {
    if !tauri_bridge::has_tauri() {
        return;
    }

    crate::services::tauri_listen("ai-stream-chunk", move |ev: web_sys::Event| {
        // Tauri event object; the AiChunk payload is under `.payload`.
        let value: &JsValue = ev.as_ref();
        let payload = js_sys::Reflect::get(value, &"payload".into()).unwrap_or(JsValue::UNDEFINED);

        let chunk: AiChunkEvent = match serde_wasm_bindgen::from_value(payload) {
            Ok(chunk) => chunk,
            Err(e) => {
                web_sys::console::warn_1(&format!("[ai] bad chunk payload: {e}").into());
                return;
            }
        };

        crate::events::dispatch_typed_event(AI_CHUNK_EVENT, &chunk);
    });
}
