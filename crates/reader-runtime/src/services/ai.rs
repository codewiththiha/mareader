//! The frontend half of the AI word-explanation feature.

pub use ai_core::types::{AiChunk, AiChunkEvent};
use leptos::task::spawn_local;
use wasm_bindgen::JsValue;

pub use app_ui::events::AI_CHUNK_EVENT;

/// Starts an `explain_word` run, tagged with `run`.
pub fn invoke_explain_word(word: String, context: String, run: String) {
    spawn_local(async move {
        if let Err(e) = ai_core::bridge::explain_word(&word, &context, &run).await {
            web_sys::console::warn_1(&format!("[ai] explain_word invoke failed: {e}").into());
        }
    });
}

/// Register the Tauri chunk listener once; chunks re-broadcast as
/// window events.
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

        app_ui::events::dispatch_typed_event(AI_CHUNK_EVENT, &chunk);
    });
}
