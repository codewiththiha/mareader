//! Typed wrappers over the JS engine (window.PDFReader). This is the ONLY module
//! that calls engine functions; views and effects never touch wasm-bindgen types.
//!
//! Every engine fn resolves to `{ok:true, ...}` or `{ok:false, error:{name,message}}`
//! (except `buildSearchIndex` which resolves to a number). We check `ok` here and
//! surface a `Result<T, String>`.

use serde::de::DeserializeOwned;
use serde::Serialize;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;

use crate::core::bridge;
use crate::core::document::{OpenResult, RenderResult, ThumbResult};
use crate::core::search::SearchResponse;

#[derive(Debug, Clone)]
pub struct EngineError {
    pub name: String,
    pub message: String,
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.name, self.message)
    }
}

fn js_str(v: JsValue) -> String {
    v.as_string().unwrap_or_default()
}

/// Parses a `{ok:bool, error?:{name,message}, ...fields}` value into `T`.
async fn resolve<T: DeserializeOwned>(value: JsValue, what: &str) -> Result<T, EngineError> {
    let is_ok = js_sys::Reflect::get(&value, &JsValue::from_str("ok"))
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if is_ok {
        serde_wasm_bindgen::from_value(value)
            .map_err(|e| EngineError {
                name: "parse".to_string(),
                message: format!("{what}: bad engine payload ({e})"),
            })
    } else {
        let err = js_sys::Reflect::get(&value, &JsValue::from_str("error")).unwrap_or(JsValue::UNDEFINED);
        let name = js_sys::Reflect::get(&err, &JsValue::from_str("name"))
            .map(js_str)
            .unwrap_or_default();
        let message = js_sys::Reflect::get(&err, &JsValue::from_str("message"))
            .map(js_str)
            .unwrap_or_else(|_| "unknown engine error".to_string());
        Err(EngineError { name, message })
    }
}

/// Native open-file dialog (Tauri dialog plugin). Returns the chosen path, or
/// `Err` on cancel / no plugin.
pub async fn pick_pdf() -> Result<String, String> {
    let opts = js_sys::Object::new();
    js_sys::Reflect::set(&opts, &JsValue::from_str("multiple"), &JsValue::FALSE).unwrap();
    js_sys::Reflect::set(&opts, &JsValue::from_str("directory"), &JsValue::FALSE).unwrap();
    let filters = js_sys::Array::new();
    let filter = js_sys::Object::new();
    js_sys::Reflect::set(&filter, &JsValue::from_str("name"), &JsValue::from_str("PDF")).unwrap();
    let exts = js_sys::Array::new();
    exts.push(&JsValue::from_str("pdf"));
    js_sys::Reflect::set(&filter, &JsValue::from_str("extensions"), &exts).unwrap();
    filters.push(&filter);
    js_sys::Reflect::set(&opts, &JsValue::from_str("filters"), &filters).unwrap();

    let value = bridge::pick_file(opts.into()).await;
    match value.as_string() {
        Some(path) if !path.is_empty() => Ok(path),
        _ => Err("Open cancelled".to_string()),
    }
}

pub async fn open(path: &str) -> Result<OpenResult, EngineError> {
    let value = bridge::open(path).await;
    resolve::<OpenResult>(value, "open").await
}

/// Contract API: explicit teardown for a future "close document"
/// action. Not called yet — opening a new doc already destroys the previous one
/// inside the engine — but kept wired to the JS surface so the contract holds.
#[allow(dead_code)]
pub async fn destroy() {
    let _ = bridge::destroy().await;
}

/// Contract API: the engine's own page count. The UI reads
/// `doc.num_pages` (set from `open`) instead, but the call stays wired so the
/// documented JS surface remains complete and callable.
#[allow(dead_code)]
pub fn page_count() -> u32 {
    bridge::page_count()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RegisterPagePayload {
    pub page: u32,
    pub canvas_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_id: Option<String>,
}

pub fn register_page(page: u32, canvas_id: &str, host_id: Option<&str>) {
    let payload = RegisterPagePayload {
        page,
        canvas_id: canvas_id.to_string(),
        host_id: host_id.map(str::to_string),
    };
    let value = serde_wasm_bindgen::to_value(&payload).unwrap();
    bridge::register_page(value);
}

pub fn unregister_page(canvas_id: &str) {
    bridge::unregister_page(canvas_id);
}

pub async fn render_page(canvas_id: &str, scale: f64, render_text: bool) -> Result<RenderResult, EngineError> {
    let value = bridge::render_page(canvas_id, scale, render_text).await;
    resolve::<RenderResult>(value, "render").await
}

/// Render one thumbnail through the engine's cached thumbnail lane.
///
/// Unlike `render_page` this needs no `register_page` (the engine resolves the
/// canvas by id per call) and never builds a text layer. When the page's bitmap
/// is already cached the engine blits it synchronously and returns
/// `cached: true` — the caller must then skip its loading skeleton, because the
/// canvas is already painted on the first mounted frame.
pub async fn render_thumb(canvas_id: &str, page: u32, scale: f64) -> Result<ThumbResult, EngineError> {
    let value = bridge::render_thumb(canvas_id, page, scale).await;
    resolve::<ThumbResult>(value, "thumb").await
}

/// Cancel an in-flight thumbnail render (cell unmounted). Does NOT evict the
/// cached bitmap: a page that scrolls out and back must repaint instantly.
pub fn cancel_thumb(canvas_id: &str) {
    bridge::cancel_thumb(canvas_id);
}

/// Synchronous probe: is this page's thumbnail already cached at `scale`?
/// Read while a cell builds its view so a hit can mount with no skeleton.
pub fn has_thumb(page: u32, scale: f64) -> bool {
    bridge::has_thumb(page, scale)
}

/// Paint the cached thumbnail of `page` into `canvas_id`, upscaled, as a
/// placeholder while the real render is in flight. Returns true if painted.
pub fn blit_thumb(canvas_id: &str, page: u32) -> bool {
    bridge::blit_thumb(canvas_id, page)
}

/// Re-render one canvas at a new scale without a full remount (cancel + render).
/// Engine API contract; unused while PageCanvas handles
/// scale changes itself.
#[allow(dead_code)]
pub async fn update_page(canvas_id: &str, scale: f64) -> Result<RenderResult, EngineError> {
    let value = bridge::update_page(canvas_id, scale).await;
    resolve::<RenderResult>(value, "update").await
}

/// One canvas entry for `render_pages`. Engine API contract.
#[allow(dead_code)]
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderEntry {
    pub page: u32,
    pub canvas_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub render_text: Option<bool>,
}

/// Batch render for the continuous view. Returns one result per entry.
/// Engine API contract; the continuous view currently relies
/// on per-page re-render, but this stays as the future batching path.
#[allow(dead_code)]
pub async fn render_pages(entries: &[RenderEntry], scale: f64) -> Vec<Result<RenderResult, EngineError>> {
    let payload = serde_wasm_bindgen::to_value(&entries).unwrap();
    let value = bridge::render_pages(payload, scale).await;
    let arr = value.unchecked_into::<js_sys::Array>();
    let mut out = Vec::with_capacity(arr.length() as usize);
    for item in arr.iter() {
        out.push(resolve::<RenderResult>(item, "render").await);
    }
    out
}

pub async fn build_search_index() -> Result<u32, EngineError> {
    let value = bridge::build_search_index().await;
    value
        .as_f64()
        .map(|n| n as u32)
        .ok_or_else(|| EngineError {
            name: "parse".to_string(),
            message: "build_search_index: bad payload".to_string(),
        })
}

pub async fn search(query: &str) -> Result<SearchResponse, EngineError> {
    let value = bridge::search(query).await;
    resolve::<SearchResponse>(value, "search").await
}

/// Mark occurrence `index` of `page` as the current match (`index < 0` clears).
pub fn set_active_match(page: u32, index: i32) {
    bridge::set_active_match(page, index);
}

/// Mute the painted highlights (`stale`) or restore them (`live`).
pub fn set_highlight_mode(stale: bool) {
    bridge::set_highlight_mode(if stale { "stale" } else { "live" });
}

pub fn clear_highlights() {
    bridge::clear_highlights();
}
