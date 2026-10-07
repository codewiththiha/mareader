//! The raw `window.__TAURI__` surface, declared once.

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

// Every async extern carries `catch`, so rejections resolve as `Err`.
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

/// The path an OS-level open handed the app, dequeued by each call.
pub async fn take_pending_file() -> Option<String> {
    if !has_tauri() {
        return None;
    }
    let value = invoke("take_pending_file", JsValue::UNDEFINED).await.ok()?;
    value.as_string().filter(|s| !s.is_empty())
}

/// True when the app runs inside Tauri; off wasm, `false` is also truthful.
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
