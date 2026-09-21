//! The Tauri-side kickoff of a word explanation.
//!
//! The raw `window.__TAURI__` externs come from `tauri-bridge`; this module
//! owns the `explain_word` protocol shape (argument keys, fire-and-forget
//! contract, the logged kickoff) so no other crate hand-rolls them. The
//! streamed-chunk half is
//! NOT here: it rides the app-lifetime Tauri listener that re-broadcasts
//! `ai-stream-chunk` as a window event (the app's `services::ai`), because
//! parking that closure in the app's reactive owner is load-bearing.

use std::thread::LocalKey;

use wasm_bindgen::JsValue;

// Hoisted `explain_word` argument keys, built once — the pattern the engine
// API uses for its hottest keys, kept uniform here. `thread_local!` emits
// `const NAME: LocalKey<JsValue>`, so `&KEY_WORD` at a call site is a
// promoted `'static` reference, which is what `LocalKey::with` requires. (A
// `///` block above a macro invocation is rejected as an unused doc comment,
// hence plain comments.)
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

/// The gloss wiring's one kickoff entry point: start the run, log a
/// failure, move on. Lives beside the protocol rather than in the shell's
/// AI service so the reader crate can start a run without knowing the
/// shell — the kickoff IS wire protocol.
///
/// `spawn_local` rather than a leptos task: this crate has no leptos
/// dependency, and a fire-and-forget invoke has no reactive owner to
/// outlive. Off wasm the future stub aborts — the same inertness rule as
/// [`explain_word`], and host tests never start a run.
pub fn invoke_explain_word(word: String, context: String, run: String) {
    wasm_bindgen_futures::spawn_local(async move {
        if let Err(e) = explain_word(&word, &context, &run).await {
            web_sys::console::warn_1(&format!("[ai] explain_word invoke failed: {e}").into());
        }
    });
}

/// Fire-and-forget start of an `explain_word` run on the backend. Nothing
/// comes back here: the backend streams chunks over the `ai-stream-chunk`
/// event, which the frontend AI service listens to. Outside Tauri
/// (`trunk serve`) this is a silent no-op; a genuine invoke failure surfaces
/// as `Err` for the caller to log.
///
/// `run` is the caller's id for this request; the backend echoes it on every
/// chunk so the listener can drop the chunks of a run it has moved on
/// from.
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
        .map_err(|e| e.as_string().unwrap_or_else(|| "unknown invoke error".to_string()))
}
