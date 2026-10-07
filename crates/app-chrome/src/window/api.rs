//! The window commands: minimize, maximize, close, the maximized probe
//! and the macOS lights switch.

use wasm_bindgen::JsValue;

// Hoisted invoke-arg keys: re-created per call would allocate per key.
thread_local! {
    static KEY_VISIBLE: JsValue = JsValue::from_str("visible");
    static KEY_HEADER_HEIGHT: JsValue = JsValue::from_str("headerHeight");
}

/// The current Tauri window handle, or `None` outside Tauri.
fn window() -> Option<JsValue> {
    if !tauri_bridge::has_tauri() {
        return None;
    }
    let win = tauri_bridge::get_current_window();
    if win.is_undefined() || win.is_null() {
        None
    } else {
        Some(win)
    }
}

/// Call a no-arg window method and return its RESOLVED value.
async fn invoke_method(win: &JsValue, name: &str) -> Option<JsValue> {
    let method = js_sys::Reflect::get(win, &JsValue::from_str(name)).ok()?;
    if !method.is_function() {
        return None;
    }
    let func: js_sys::Function = method.into();
    let result = js_sys::Reflect::apply(&func, win, &js_sys::Array::new()).ok()?;
    if result.is_undefined() || result.is_null() {
        return Some(result);
    }
    // The cast is unchecked: `Promise::try_from` cannot fail.
    let promise = js_sys::Promise::from(result);
    wasm_bindgen_futures::JsFuture::from(promise).await.ok()
}

/// Minimize to the taskbar. No-op outside Tauri.
pub async fn minimize_window() {
    if let Some(win) = window() {
        invoke_method(&win, "minimize").await;
    }
}

/// Maximize and restore, the same command behind two triggers.
pub async fn toggle_maximize_window() {
    if let Some(win) = window() {
        invoke_method(&win, "toggleMaximize").await;
    }
}

/// Close the window. This app runs a single window, so this is quit.
pub async fn close_window() {
    if let Some(win) = window() {
        invoke_method(&win, "close").await;
    }
}

/// Whether the window is maximized, for the glyph.
pub async fn is_window_maximized() -> Option<bool> {
    let win = window()?;
    invoke_method(&win, "isMaximized").await?.as_bool()
}

/// Show or hide the native macOS traffic lights.
pub async fn set_traffic_lights(visible: bool, header_height: f64) {
    if !tauri_bridge::has_tauri() {
        return;
    }
    let args: JsValue = js_sys::Object::new().into();
    KEY_VISIBLE.with(|k| {
        let _ = js_sys::Reflect::set(&args, k, &JsValue::from_bool(visible));
    });
    KEY_HEADER_HEIGHT.with(|k| {
        let _ = js_sys::Reflect::set(&args, k, &JsValue::from_f64(header_height));
    });
    _ = tauri_bridge::invoke("set_traffic_lights", args).await;
}
