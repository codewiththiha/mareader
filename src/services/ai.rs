//! The frontend half of the AI word-explanation feature.
//!
//! Owns the wire protocol with the Tauri backend: [`invoke_explain_word`]
//! starts a run (fire-and-forget — results never come back through the
//! invoke) and [`install_ai_chunk_bridge`] registers ONE app-lifetime Tauri
//! listener that re-broadcasts each chunk as a window `CustomEvent`
//! (`pdfreader:ai-chunk`). The gloss popover (and anything else) then
//! listens on the window, so document switches never stack dead Tauri
//! handlers or drop the live one.

use leptos::prelude::*;
use leptos::task::spawn_local;
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsValue;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::Event;

use crate::components::ai::types::{AiError, WordInfo};

/// Window event the app-lifetime bridge re-broadcasts every AI chunk on.
/// The gloss popover listens for this (not for the Tauri event directly).
pub const AI_CHUNK_EVENT: &str = "pdfreader:ai-chunk";

/// The wire format of one `ai-stream-chunk` payload. Mirrors `AiChunk` in
/// `src-tauri/src/ai/traits.rs` (same `type`/`data` tagging) — keep in sync.
///
/// `Serialize` lets the bridge park the same shape on a window CustomEvent
/// detail so per-mount UI can subscribe without touching Tauri again.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum AiChunkEvent {
    Snapshot(WordInfo),
    Done,
    /// A typed failure; see [`AiError`] for the cause/retry contract.
    Error(AiError),
}

/// Starts an `explain_word` run on the backend. The streamed results arrive
/// as `ai-stream-chunk` events, re-broadcast by [`install_ai_chunk_bridge`].
pub fn invoke_explain_word(word: String, context: String) {
    spawn_local(async move {
        if let Err(e) = pdf_engine::api::explain_word(&word, &context).await {
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
/// Must be called from inside the app reactive owner (e.g. `App`): the
/// `StoredValue` that parks the JS Closure lives in that owner, so dropping
/// the local Copy handle is fine — disposal only happens when the app
/// unmounts.
///
/// Outside Tauri (`trunk serve` in a plain browser) this is a no-op: there is
/// no `window.__TAURI__`, and the wasm-bindgen shim for `__TAURI__.event.listen`
/// walks that global chain eagerly, so calling it throws a TypeError. Because
/// this runs from the app root, that throw took the whole mount down with it —
/// the probe is the same one every other Tauri surface uses (see
/// `bridge::has_tauri` and `services::document::open::init_open_file_handling`).
/// With no backend there are no chunks to bridge, so skipping is the correct
/// behaviour, not a degraded one.
pub fn install_ai_chunk_bridge() {
    if !pdf_engine::has_tauri() {
        return;
    }

    let handle = StoredValue::new_local(None::<Closure<dyn FnMut(Event)>>);

    let cb: Closure<dyn FnMut(Event)> = Closure::wrap(Box::new(move |ev: Event| {
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

        let Some(win) = web_sys::window() else {
            return;
        };
        let Ok(detail) = serde_wasm_bindgen::to_value(&chunk) else {
            return;
        };
        let init = web_sys::CustomEventInit::new();
        init.set_detail(&detail);
        if let Ok(ev) = web_sys::CustomEvent::new_with_event_init_dict(AI_CHUNK_EVENT, &init) {
            let _ = win.dispatch_event(&ev);
        }
    }) as Box<dyn FnMut(Event)>);

    let f: js_sys::Function = cb.as_ref().unchecked_ref::<js_sys::Function>().clone();
    spawn_local(async move {
        // The unlisten handle is deliberately discarded — this listener lives
        // for the app's lifetime (same pattern as drag_drop.rs).
        _ = pdf_engine::listen("ai-stream-chunk", f).await;
    });

    handle.set_value(Some(cb));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::ai::types::AiErrorKind;

    // The exact JSON shapes the backend's `app.emit("ai-stream-chunk",
    // &chunk)` produces from `AiChunk`'s serde tagging. If these parse, the
    // enum mirror above is wire-compatible.
    #[test]
    fn chunk_wire_shapes_parse() {
        let snapshot: AiChunkEvent = serde_json::from_str(
            r#"{"type":"Snapshot","data":{"pos":"noun","meaning":"lasting briefly","synonyms":["fleeting"],"usages":["ephemeral beauty"]}}"#,
        )
        .unwrap();
        match snapshot {
            AiChunkEvent::Snapshot(info) => {
                assert_eq!(info.pos, "noun");
                assert_eq!(info.synonyms, vec!["fleeting"]);
            }
            other => panic!("expected Snapshot, got {other:?}"),
        }

        let done: AiChunkEvent = serde_json::from_str(r#"{"type":"Done"}"#).unwrap();
        assert!(matches!(done, AiChunkEvent::Done));

        let error: AiChunkEvent = serde_json::from_str(
            r#"{"type":"Error","data":{"kind":"model_not_ready","message":"the on-device model is still downloading","retryable":true}}"#,
        )
        .unwrap();
        match error {
            AiChunkEvent::Error(err) => {
                assert_eq!(err.kind, AiErrorKind::ModelNotReady);
                assert!(err.retryable);
            }
            other => panic!("expected Error, got {other:?}"),
        }

        // The escape-hatch variant carries its summary inline.
        let other: AiChunkEvent = serde_json::from_str(
            r#"{"type":"Error","data":{"kind":{"other":"helper crashed"},"message":"helper crashed","retryable":false}}"#,
        )
        .unwrap();
        match other {
            AiChunkEvent::Error(err) => {
                assert_eq!(err.kind, AiErrorKind::Other("helper crashed".into()));
                assert!(!err.retryable);
            }
            other => panic!("expected Error(Other), got {other:?}"),
        }
    }
}
