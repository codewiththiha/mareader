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

#[wasm_bindgen]
extern "C" {
    // --- Tauri dialog plugin: window.__TAURI__.dialog.open({...}) -> Promise<string|null> ---
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "dialog"], js_name = open)]
    pub async fn pick_file(options: JsValue) -> JsValue;

    // --- PDF engine: window.PDFReader ---
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"])]
    pub fn version() -> String;

    // Rust fns are snake_case; js_name maps each to the engine's camelCase API.
    // Without the override wasm-bindgen would emit
    // `window.PDFReader.storage_get`, which does not exist and panics the mount.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "storageGet")]
    pub fn storage_get(key: &str) -> JsValue;

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "storageSet")]
    pub fn storage_set(key: &str, value: &str);

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"])]
    pub async fn open(path: &str) -> JsValue;

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"])]
    pub async fn destroy() -> JsValue;

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "pageCount")]
    pub fn page_count() -> u32;

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "registerPage")]
    pub fn register_page(payload: JsValue);

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "unregisterPage")]
    pub fn unregister_page(canvas_id: &str);

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "cancelPage")]
    pub fn cancel_page(canvas_id: &str);

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "renderPage")]
    pub async fn render_page(canvas_id: &str, scale: f64, render_text: bool) -> JsValue;

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "renderPages")]
    pub async fn render_pages(entries: JsValue, scale: f64) -> JsValue;

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "updatePage")]
    pub async fn update_page(canvas_id: &str, scale: f64) -> JsValue;

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "buildSearchIndex")]
    pub async fn build_search_index() -> JsValue;

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"])]
    pub async fn search(query: &str) -> JsValue;

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "clearHighlights")]
    pub fn clear_highlights();

    // --- Tauri v2 window/event surface ---
    // Tauri v2 window handle (used by MoreMenu fullscreen, phase 3) and event
    // listener (used by ReaderView drag-drop, phase 5). js_name mapping is load-bearing.
    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "window"], js_name = "getCurrentWindow")]
    #[allow(dead_code)]
    pub fn tauri_get_current_window() -> JsValue;

    #[wasm_bindgen(js_namespace = ["window", "__TAURI__", "event"], js_name = "listen")]
    #[allow(dead_code)]
    pub async fn tauri_listen(event: &str, handler: js_sys::Function) -> JsValue;
}
