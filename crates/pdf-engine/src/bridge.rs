//! Wasm-bindgen interop with the JS layer.
//!
//! Two JS surfaces are reachable:
//!   - `window.__TAURI__.*` (Tauri v2, `withGlobalTauri: true`) — dialog plugin.
//!   - `window.PDFReader` (public/pdfEngine.js) — the imperative pdf.js wrapper.
//!
//! This module is the ONLY place that declares `extern "C"` bindings. Callers go
//! through `crate::api::engine` for the engine, never here directly (except the
//! `bridge` module itself). The async fns mirror the existing `invoke` pattern:
//! wasm-bindgen awaits the underlying Promise and yields the resolved JsValue.
//!
//! CONTRACT: do not change these signatures.

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

#[wasm_bindgen]
extern "C" {
    // --- Tauri dialog plugin: window.__TAURI__.dialog.open({...}) -> Promise<string|null> ---
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "dialog"], js_name = open)]
    pub async fn pick_file(options: JsValue) -> JsValue;

    // --- PDF engine: window.PDFReader ---
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"])]
    pub fn version() -> String;

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"])]
    pub async fn open(path: &str) -> JsValue;

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"])]
    pub async fn destroy() -> JsValue;

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "registerPage")]
    pub fn register_page(payload: JsValue);

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "unregisterPage")]
    pub fn unregister_page(canvas_id: &str);

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "cancelPage")]
    pub fn cancel_page(canvas_id: &str);

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "renderPage")]
    pub async fn render_page(canvas_id: &str, scale: f64, render_text: bool) -> JsValue;

    // Thumbnail lane: a separate, cheap render path
    // with a bitmap cache. `renderThumb` resolves `{ok, width, height, scale,
    // cached}`; `cached:true` means the bitmap was blitted synchronously from
    // the cache, so the caller must NOT show a loading skeleton over it.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "renderThumb")]
    pub async fn render_thumb(canvas_id: &str, page: u32, scale: f64) -> JsValue;

    /// Render page 1 of the book at `path` to a JPEG data URL (library shelf
    /// cover art). Resolves `{ok, dataUrl, width, height}`.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "coverDataUrl")]
    pub async fn cover_data_url(path: &str, max_width: f64) -> JsValue;

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "cancelThumb")]
    pub fn cancel_thumb(canvas_id: &str);

    /// SYNCHRONOUS cache probe, read while a thumbnail cell builds its view so
    /// a cache-hit cell can mount without a skeleton at all.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "hasThumb")]
    pub fn has_thumb(page: u32, scale: f64) -> bool;

    /// Paint the cached thumbnail of `page` into `canvas_id` as a blurry
    /// placeholder. Best-effort: returns false when there is nothing cached.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "blitThumb")]
    pub fn blit_thumb(canvas_id: &str, page: u32) -> bool;

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "buildSearchIndex")]
    pub async fn build_search_index() -> JsValue;

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"])]
    pub async fn search(query: &str) -> JsValue;

    /// Emphasise occurrence `index` of `page` as the current match. `index < 0`
    /// clears the marker without touching the other highlights.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "setActiveMatch")]
    pub fn set_active_match(page: u32, index: i32);

    /// Switch the painted highlights between "live" and "stale".
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "setHighlightMode")]
    pub fn set_highlight_mode(mode: &str);

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "clearHighlights")]
    pub fn clear_highlights();

    // --- Tauri v2 window/event surface ---
    // Tauri v2 window handle (used by MoreMenu fullscreen, phase 3) and event
    // listener (used by ReaderView drag-drop, phase 5). js_name mapping is load-bearing.
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "window"], js_name = "getCurrentWindow")]
    #[allow(dead_code)]
    pub fn tauri_get_current_window() -> JsValue;

    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "event"], js_name = "listen")]
    pub async fn tauri_listen(event: &str, handler: js_sys::Function) -> JsValue;

    // Generic Tauri IPC invoke (window.__TAURI__.core.invoke). Used to reach
    // backend commands (e.g. the macOS traffic-light toggle); `catch` so a
    // rejected invoke resolves as an Err instead of unwinding the wasm future.
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "core"], js_name = invoke, catch)]
    pub async fn tauri_invoke(cmd: &str, args: JsValue) -> Result<JsValue, JsValue>;

    // Collect the pending OS-opened PDF path (double-click / "Open with" /
    // default-app launch) from the backend. Lives in the engine's JS layer
    // because it wraps `__TAURI__.core.invoke` in a catch: a rejected JS
    // promise cannot be represented in a wasm future (it unwinds as a panic),
    // so the engine resolves null instead of ever rejecting.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "takePendingFile")]
    pub async fn take_pending_file() -> JsValue;

    // --- Engine: theme re-bake + scrub mode ---
    // The engine bakes the theme (filter + paper blend) into every page and
    // thumbnail raster so canvases are plain opaque textures. On an appearance
    // change it must re-bake the rasters it already holds; the theme applier
    // calls `refresh_theme` after writing the new CSS variables. During a
    // slider scrub the variables change every frame, so the theme applier
    // switches the engine into scrub mode instead: raw rasters + the live
    // CSS pipeline, exactly like the pre-baking behaviour, for the duration
    // of the drag.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "refreshTheme")]
    pub fn refresh_theme();

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "setScrubMode")]
    pub fn set_scrub_mode(on: bool);
}

/// Subscribe to a Tauri event. `handler` receives the event object (a JsValue,
/// e.g. Tauri's `tauri://drag-drop` event). Returns the unlisten handle — keep
/// it to unsubscribe later, or drop it to keep the listener registered for the
/// lifetime of the webview. Thin wrapper so views don't call the wasm-bindgen
/// extern directly.
pub async fn listen(event: &str, handler: js_sys::Function) -> JsValue {
    tauri_listen(event, handler).await
}

/// True when the app runs inside Tauri (`window.__TAURI__` is present).
///
/// Must be checked BEFORE any `window.__TAURI__.*` call: the wasm-bindgen shim
/// evaluates the global chain directly and throws a TypeError when the global
/// is absent (e.g. `trunk serve` in a plain browser). See more_menu.rs for the
/// same probe.
/// True when `window.PDFReader` exists. Must be checked before any engine
/// call: a missing global makes the wasm-bindgen shim throw, which panics
/// the reactive owner and freezes menus / theme / open.
pub fn has_pdf_reader() -> bool {
    web_sys::window()
        .map(|w| {
            let g: js_sys::Object = w.unchecked_into();
            js_sys::Reflect::get(&g, &JsValue::from_str("PDFReader"))
                .map(|v| !(v.is_undefined() || v.is_null()))
                .unwrap_or(false)
        })
        .unwrap_or(false)
}

/// True when the app runs inside Tauri (`window.__TAURI__` is present).
///
/// Must be checked BEFORE any `window.__TAURI__.*` call: the wasm-bindgen shim
/// evaluates the global chain directly and throws a TypeError when the global
/// is absent (e.g. `trunk serve` in a plain browser). See more_menu.rs for the
/// same probe.
pub fn has_tauri() -> bool {
    web_sys::window()
        .map(|w| {
            let g: js_sys::Object = w.unchecked_into();
            js_sys::Reflect::get(&g, &JsValue::from_str("__TAURI__"))
                .map(|v| !(v.is_undefined() || v.is_null()))
                .unwrap_or(false)
        })
        .unwrap_or(false)
}
