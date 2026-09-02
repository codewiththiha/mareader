//! The raw `window.__TAURI__` surface, declared once.
//!
//! Tauri v2 with `withGlobalTauri: true` publishes its API as a global:
//! `__TAURI__.core.invoke` (IPC), `__TAURI__.event.listen` (events),
//! `__TAURI__.window.getCurrentWindow` (the window-method handle) and the
//! plugin namespaces (`__TAURI__.dialog`, …). Every frontend crate that
//! touches that global — `app-chrome` for the window commands, `pdf-engine`
//! for the file dialog and the AI kickoff — declares those externs here
//! instead of in its own bridge, so the wasm-bindgen declarations (whose
//! attribute spellings are load-bearing) live in exactly one place, and no
//! format crate needs to own chrome's IPC surface.
//!
//! Every caller must probe [`has_tauri`] BEFORE any call: the wasm-bindgen
//! shim dereferences the `window.__TAURI__` chain eagerly and throws a
//! TypeError when the global is absent (a plain browser under `trunk
//! serve`), which would panic whatever future awaited it.
//!
//! CONTRACT: do not change these signatures.

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

#[wasm_bindgen]
extern "C" {
    // Generic Tauri IPC invoke (window.__TAURI__.core.invoke). Used to reach
    // backend commands (the macOS traffic-light toggle, the AI kickoff);
    // `catch` so a rejected invoke resolves as an Err instead of unwinding
    // the wasm future.
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"], js_name = invoke, catch)]
    pub async fn invoke(cmd: &str, args: JsValue) -> Result<JsValue, JsValue>;

    // Tauri event listener (window.__TAURI__.event.listen). Resolves to the
    // unlisten handle.
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "event"], js_name = "listen", catch)]
    pub async fn listen(event: &str, handler: js_sys::Function) -> Result<JsValue, JsValue>;

    // Tauri v2 window handle (window methods: minimize, toggleMaximize,
    // close, isMaximized, …).
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "window"], js_name = "getCurrentWindow")]
    pub fn get_current_window() -> JsValue;

    // Tauri dialog plugin: window.__TAURI__.dialog.open({...}) -> Promise<string|null>
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "dialog"], js_name = open, catch)]
    pub async fn open(options: JsValue) -> Result<JsValue, JsValue>;
}

/// True when the app runs inside Tauri (`window.__TAURI__` is present).
///
/// Must be checked BEFORE any `window.__TAURI__.*` call: the wasm-bindgen
/// shim evaluates the global chain directly and throws a TypeError when the
/// global is absent (e.g. `trunk serve` in a plain browser). The non-wasm
/// short-circuit keeps the probe callable from host `cargo test`; on the
/// host there is no Tauri, so `false` is also the truthful answer.
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
