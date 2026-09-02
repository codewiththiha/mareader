//! The Tauri-side kickoff of a word explanation.
//!
//! The raw `window.__TAURI__` externs come from `tauri-bridge`; this module
//! owns the `explain_word` protocol shape (the argument keys and the
//! fire-and-forget contract) so no other crate hand-rolls them.
//!
//! The other half of the protocol — the streamed chunks — is NOT here: it
//! rides the app-lifetime Tauri listener that re-broadcasts
//! `ai-stream-chunk` as a window event (see the app's `services::ai`),
//! because parking that closure in the app's reactive owner is load-bearing.

use wasm_bindgen::JsValue;

/// Fire-and-forget start of an `explain_word` run on the backend. Nothing
/// comes back here: the backend streams its chunks over the
/// `ai-stream-chunk` event, which the frontend AI service listens to.
/// Outside Tauri (`trunk serve`) there is nothing to invoke, so this is a
/// silent no-op; a genuine invoke failure surfaces as `Err` for the caller
/// to log.
///
/// `run` is the caller's id for this request; the backend echoes it on every
/// chunk so the listener can drop the chunks of a run it has moved on from.
///
/// The argument keys are built per call (unlike the engine's hoisted keys):
/// a word is explained at most once per selection, so there is no hot loop to
/// save allocations for.
pub async fn explain_word(word: &str, context: &str, run: &str) -> Result<(), String> {
    if !tauri_bridge::has_tauri() {
        return Ok(());
    }
    let args: JsValue = js_sys::Object::new().into();
    let _ = js_sys::Reflect::set(&args, &"word".into(), &JsValue::from_str(word));
    let _ = js_sys::Reflect::set(&args, &"context".into(), &JsValue::from_str(context));
    let _ = js_sys::Reflect::set(&args, &"run".into(), &JsValue::from_str(run));
    tauri_bridge::invoke("explain_word", args)
        .await
        .map(|_| ())
        .map_err(|e| e.as_string().unwrap_or_else(|| "unknown invoke error".to_string()))
}
