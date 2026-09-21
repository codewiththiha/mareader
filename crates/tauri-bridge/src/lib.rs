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

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

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

/// True when the app runs inside Tauri (`window.__TAURI__.core.invoke` is a function).
/// Off wasm there is no window and `false` is the truthful answer.
/// The check is stricter than mere `__TAURI__` presence: the wasm-bindgen
/// `invoke` shim dereferences `window.__TAURI__.core.invoke` eagerly and
/// throws `TypeError: Reflect.get called on non-object` if the chain is not
/// an object (e.g. a browser mock that sets `__TAURI__ = {}` without `core`).
/// Callers must probe this before any `invoke` to avoid panicking the awaiting future.
pub fn has_tauri() -> bool {
    if !cfg!(target_arch = "wasm32") {
        return false;
    }
    let Some(window) = web_sys::window() else {
        return false;
    };
    let g: js_sys::Object = window.unchecked_into();
    let Ok(tauri) = js_sys::Reflect::get(&g, &JsValue::from_str("__TAURI__")) else {
        return false;
    };
    if tauri.is_undefined() || tauri.is_null() || !tauri.is_object() {
        return false;
    }
    let Ok(core) = js_sys::Reflect::get(&tauri, &JsValue::from_str("core")) else {
        return false;
    };
    if core.is_undefined() || core.is_null() || !core.is_object() {
        return false;
    }
    let Ok(invoke) = js_sys::Reflect::get(&core, &JsValue::from_str("invoke")) else {
        return false;
    };
    !invoke.is_undefined() && !invoke.is_null()
}
