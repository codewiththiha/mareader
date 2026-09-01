//! The frontend half of the AI word-explanation feature.
//!
//! Owns the wire protocol with the Tauri backend: [`invoke_explain_word`]
//! starts a run (fire-and-forget — results never come back through the
//! invoke) and [`install_ai_chunk_bridge`] registers ONE app-lifetime Tauri
//! listener that re-broadcasts each chunk as a window `CustomEvent`
//! (`pdfreader:ai-chunk`). The gloss popover (and anything else) then
//! listens on the window, so document switches never stack dead Tauri
//! handlers or drop the live one.

use leptos::task::spawn_local;
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsValue;

use crate::components::ai::types::{AiError, WordInfo};

pub use crate::events::AI_CHUNK_EVENT;

/// One chunk of an explanation. Mirrors `AiChunk` in
/// `src-tauri/src/ai/traits.rs` (same `type`/`data` tagging) — keep in sync.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum AiChunk {
    Snapshot(WordInfo),
    Done,
    /// A typed failure; see [`AiError`] for the cause/retry contract.
    Error(AiError),
}

/// The wire format of one `ai-stream-chunk` payload: a chunk plus the id of
/// the run that produced it. Mirrors `AiStreamEvent` in
/// `src-tauri/src/ai/traits.rs` — keep in sync.
///
/// The run id is what makes concurrent glosses safe. Runs are never cancelled
/// backend-side, so a reader who glosses a second word while the first is
/// still thinking has two runs emitting on one event name; without the id the
/// abandoned run's answer would be rendered against — and cached under — the
/// word that is on screen now.
///
/// `Serialize` lets the bridge park the same shape on a window CustomEvent
/// detail so per-mount UI can subscribe without touching Tauri again.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiChunkEvent {
    /// The id passed to [`invoke_explain_word`], echoed by the backend.
    pub run: String,
    pub chunk: AiChunk,
}

/// Starts an `explain_word` run on the backend, tagged with `run`. The
/// streamed results arrive as `ai-stream-chunk` events carrying that same id,
/// re-broadcast by [`install_ai_chunk_bridge`].
pub fn invoke_explain_word(word: String, context: String, run: String) {
    spawn_local(async move {
        if let Err(e) = pdf_engine::api::explain_word(&word, &context, &run).await {
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
/// `bridge::has_tauri` and `services::document::open::init_open_file_handling`).
/// With no backend there are no chunks to bridge, so skipping is the correct
/// behaviour, not a degraded one.
pub fn install_ai_chunk_bridge() {
    if !pdf_engine::has_tauri() {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::ai::types::AiErrorKind;

    // The exact JSON shapes the backend's `app.emit("ai-stream-chunk",
    // &chunk)` produces from `AiChunk`'s serde tagging. If these parse, the
    // enum mirror above is wire-compatible.
    #[test]
    fn chunk_wire_shapes_parse() {
        let snapshot: AiChunk = serde_json::from_str(
            r#"{"type":"Snapshot","data":{"pos":"noun","meaning":"lasting briefly","synonyms":["fleeting"],"usages":["ephemeral beauty"]}}"#,
        )
        .unwrap();
        match snapshot {
            AiChunk::Snapshot(info) => {
                assert_eq!(info.pos, "noun");
                assert_eq!(info.synonyms, vec!["fleeting"]);
            }
            other => panic!("expected Snapshot, got {other:?}"),
        }

        let done: AiChunk = serde_json::from_str(r#"{"type":"Done"}"#).unwrap();
        assert!(matches!(done, AiChunk::Done));

        let error: AiChunk = serde_json::from_str(
            r#"{"type":"Error","data":{"kind":"model_not_ready","message":"the on-device model is still downloading","retryable":true}}"#,
        )
        .unwrap();
        match error {
            AiChunk::Error(err) => {
                assert_eq!(err.kind, AiErrorKind::ModelNotReady);
                assert!(err.retryable);
            }
            other => panic!("expected Error, got {other:?}"),
        }

        // The escape-hatch variant carries its summary inline.
        let other: AiChunk = serde_json::from_str(
            r#"{"type":"Error","data":{"kind":{"other":"helper crashed"},"message":"helper crashed","retryable":false}}"#,
        )
        .unwrap();
        match other {
            AiChunk::Error(err) => {
                assert_eq!(err.kind, AiErrorKind::Other("helper crashed".into()));
                assert!(!err.retryable);
            }
            other => panic!("expected Error(Other), got {other:?}"),
        }
    }

    /// The envelope the backend actually emits: the chunk nested under
    /// `chunk`, the run id beside it. If this drifts, every chunk is dropped
    /// by the listener's run gate and the card never opens.
    #[test]
    fn the_envelope_carries_the_run_id() {
        let event: AiChunkEvent = serde_json::from_str(
            r#"{"run":"g3-1712#4","chunk":{"type":"Snapshot","data":{"pos":"adj","meaning":"short-lived","synonyms":[],"usages":[]}}}"#,
        )
        .unwrap();
        assert_eq!(event.run, "g3-1712#4");
        match event.chunk {
            AiChunk::Snapshot(info) => assert_eq!(info.meaning, "short-lived"),
            other => panic!("expected Snapshot, got {other:?}"),
        }
    }
}
