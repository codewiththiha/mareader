//! The raw `window.__TAURI__` surface, declared once.
//!
//! Tauri v2 with `withGlobalTauri: true` publishes its API as a global:
//! `__TAURI__.core.invoke` (IPC), `__TAURI__.event.listen` (events),
//! `__TAURI__.window.getCurrentWindow` and the plugin namespaces
//! (`__TAURI__.dialog`, ...). Every frontend crate that touches that global —
//! `app-chrome` for the window commands, `pdf-engine` for the file dialog and
//! the AI kickoff — declares its externs here, so the wasm-bindgen
//! declarations (whose attribute spellings are load-bearing) live in exactly
//! one place and no format crate owns chrome's IPC surface.
//!
//! Every caller must probe [`has_tauri`] BEFORE any call: the wasm-bindgen
//! shim dereferences the `window.__TAURI__` chain eagerly and throws a
//! TypeError when the global is absent (a plain browser under `trunk serve`),
//! which would panic whatever future awaited it.
//!
//! CONTRACT: do not change these signatures.

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

// Every async extern carries `catch`: a rejected JS promise cannot be
// represented in a wasm future (it unwinds as a panic), so rejections must
// resolve as `Err` instead.
#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"], js_name = invoke, catch)]
    pub async fn invoke(cmd: &str, args: JsValue) -> Result<JsValue, JsValue>;

    /// Resolves to the unlisten handle.
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "event"], js_name = "listen", catch)]
    pub async fn listen(event: &str, handler: js_sys::Function) -> Result<JsValue, JsValue>;

    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "window"], js_name = "getCurrentWindow")]
    pub fn get_current_window() -> JsValue;

    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "dialog"], js_name = open, catch)]
    pub async fn open(options: JsValue) -> Result<JsValue, JsValue>;
}

/// The path an OS-level open handed to the app, taken once.
///
/// The native host queues the path a document was launched with — a
/// file-manager double-click, an "open with", a launch argument — and every
/// `take_pending_file` invocation DEQUEUES it. Which runtime consumes the
/// path is not this crate's call: it only reaches the host. On the web, with
/// nothing queued, or on a rejected IPC, this is `None`.
pub async fn take_pending_file() -> Option<String> {
    if !has_tauri() {
        return None;
    }
    let value = invoke("take_pending_file", JsValue::UNDEFINED).await.ok()?;
    value.as_string().filter(|s| !s.is_empty())
}

/// True when the app runs inside Tauri (`window.__TAURI__` is present). Off
/// wasm there is no window to ask and `false` is also the truthful answer,
/// which keeps the probe callable from host `cargo test`.
pub fn has_tauri() -> bool {
    if !cfg!(target_arch = "wasm32") {
        return false;
    }
    web_sys::window()
        .map(|w| {
            let g: js_sys::Object = w.unchecked_into();
            js_sys::Reflect::get(&g, &JsValue::from_str("__TAURI__"))
                .map(|v| !(v.is_undefined() || v.is_null()))
                .unwrap_or(false)
        })
        .unwrap_or(false)
}
