//! The Tauri-side kickoff of a word explanation, over `tauri-bridge` externs.

use std::thread::LocalKey;

use wasm_bindgen::JsValue;

/// Hoisted `explain_word` argument keys, built once as `thread_local!` consts.
thread_local! {
    static KEY_WORD: JsValue = JsValue::from_str("word");
    static KEY_CONTEXT: JsValue = JsValue::from_str("context");
    static KEY_RUN: JsValue = JsValue::from_str("run");
}

/// `args[key] = value`, with the key read once.
fn set_arg(args: &JsValue, key: &'static LocalKey<JsValue>, value: &str) {
    key.with(|k| {
        let _ = js_sys::Reflect::set(args, k, &JsValue::from_str(value));
    });
}

/// Fire-and-forget `explain_word` kickoff: the run's chunks stream back
/// over `ai-stream-chunk`.
pub async fn explain_word(word: &str, context: &str, run: &str) -> Result<(), String> {
    if !tauri_bridge::has_tauri() {
        return Ok(());
    }
    let args: JsValue = js_sys::Object::new().into();
    set_arg(&args, &KEY_WORD, word);
    set_arg(&args, &KEY_CONTEXT, context);
    set_arg(&args, &KEY_RUN, run);
    tauri_bridge::invoke("explain_word", args)
        .await
        .map(|_| ())
        .map_err(|e| {
            e.as_string()
                .unwrap_or_else(|| "unknown invoke error".to_string())
        })
}
