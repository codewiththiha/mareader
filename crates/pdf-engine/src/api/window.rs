//! Window chrome (traffic lights) and the AI word-explanation kickoff.

use wasm_bindgen::JsValue;

use super::{reflect_set, KEY_CONTEXT, KEY_HEADER_HEIGHT, KEY_RUN, KEY_VISIBLE, KEY_WORD};
use crate::bridge;

/// Show/hide the native macOS traffic lights via the backend command. The
/// backend is a no-op outside macOS, and outside Tauri there is nothing to
/// invoke, so this is safe to call unconditionally (the reader-view effect
/// drives it from the hover-reveal signal).
pub async fn set_traffic_lights(visible: bool, header_height: f64) {
    if !bridge::has_tauri() {
        return;
    }
    let args: JsValue = js_sys::Object::new().into();
    _ = reflect_set(&args, &KEY_VISIBLE, &JsValue::from_bool(visible));
    _ = reflect_set(&args, &KEY_HEADER_HEIGHT, &JsValue::from_f64(header_height));
    _ = bridge::tauri_invoke("set_traffic_lights", args).await;
}

/// Fire-and-forget start of an `explain_word` run on the backend. Nothing
/// comes back here: the backend streams its chunks over the
/// `ai-stream-chunk` event, which the frontend AI service listens to.
/// Outside Tauri (`trunk serve`) there is nothing to invoke, so this is a
/// silent no-op; a genuine invoke failure surfaces as `Err` for the caller
/// to log.
///
/// `run` is the caller's id for this request; the backend echoes it on every
/// chunk so the listener can drop the chunks of a run it has moved on from.
pub async fn explain_word(word: &str, context: &str, run: &str) -> Result<(), String> {
    if !bridge::has_tauri() {
        return Ok(());
    }
    let args: JsValue = js_sys::Object::new().into();
    _ = reflect_set(&args, &KEY_WORD, &JsValue::from_str(word));
    _ = reflect_set(&args, &KEY_CONTEXT, &JsValue::from_str(context));
    _ = reflect_set(&args, &KEY_RUN, &JsValue::from_str(run));
    bridge::tauri_invoke("explain_word", args)
        .await
        .map(|_| ())
        .map_err(|e| e.as_string().unwrap_or_else(|| "unknown invoke error".to_string()))
}
