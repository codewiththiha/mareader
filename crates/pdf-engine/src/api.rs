//! Typed wrappers over the JS engine (window.PDFReader). This is the ONLY module
//! that calls engine functions; views and effects never touch wasm-bindgen types.
//!
//! Every engine fn resolves to `{ok:true, ...}` or `{ok:false, error:{name,message}}`
//! (except `buildSearchIndex` which resolves to a number). We check `ok` here and
//! surface a `Result<T, String>`.

use serde::de::DeserializeOwned;
use serde::Serialize;
use wasm_bindgen::JsValue;

use crate::bridge;
use crate::types::{CoverResult, OpenResult, RenderResult, ThumbResult};
use pdf_core::search::SearchResponse;

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
    if !bridge::has_tauri() {
        return Err(
            "Open dialog only available in the desktop app. Drag and drop a PDF instead."
                .to_string(),
        );
    }

    let opts = js_sys::Object::new();
    _ = js_sys::Reflect::set(&opts, &JsValue::from_str("multiple"), &JsValue::FALSE);
    _ = js_sys::Reflect::set(&opts, &JsValue::from_str("directory"), &JsValue::FALSE);
    let filters = js_sys::Array::new();
    let filter = js_sys::Object::new();
    _ = js_sys::Reflect::set(&filter, &JsValue::from_str("name"), &JsValue::from_str("PDF"));
    let exts = js_sys::Array::new();
    exts.push(&JsValue::from_str("pdf"));
    _ = js_sys::Reflect::set(&filter, &JsValue::from_str("extensions"), &exts);
    filters.push(&filter);
    _ = js_sys::Reflect::set(&opts, &JsValue::from_str("filters"), &filters);

    let value = bridge::pick_file(opts.into()).await;
    match value.as_string() {
        Some(path) if !path.is_empty() => Ok(path),
        _ => Err("Open cancelled".to_string()),
    }
}

pub async fn open(path: &str) -> Result<OpenResult, EngineError> {
    if !bridge::has_pdf_reader() {
        return Err(EngineError {
            name: "no_engine".to_string(),
            message: "PDF engine is not loaded yet. Restart the app and try again.".to_string(),
        });
    }
    let value = bridge::open(path).await;
    resolve::<OpenResult>(value, "open").await
}

/// Tear the engine document down (used when returning to the library shelf).
pub async fn destroy() {
    let _ = bridge::destroy().await;
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
    if !bridge::has_pdf_reader() {
        return;
    }
    let payload = RegisterPagePayload {
        page,
        canvas_id: canvas_id.to_string(),
        host_id: host_id.map(str::to_string),
    };
    let value = serde_wasm_bindgen::to_value(&payload).unwrap();
    bridge::register_page(value);
}

pub fn unregister_page(canvas_id: &str) {
    if !bridge::has_pdf_reader() {
        return;
    }
    bridge::unregister_page(canvas_id);
}

pub async fn render_page(canvas_id: &str, scale: f64, render_text: bool) -> Result<RenderResult, EngineError> {
    if !bridge::has_pdf_reader() {
        return Err(EngineError {
            name: "no_engine".to_string(),
            message: "PDF engine is not loaded yet. Restart the app and try again.".to_string(),
        });
    }
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
    if !bridge::has_pdf_reader() {
        return Err(EngineError {
            name: "no_engine".to_string(),
            message: "PDF engine is not loaded yet. Restart the app and try again.".to_string(),
        });
    }
    let value = bridge::render_thumb(canvas_id, page, scale).await;
    resolve::<ThumbResult>(value, "thumb").await
}

/// Cancel an in-flight thumbnail render (cell unmounted). Does NOT evict the
/// cached bitmap: a page that scrolls out and back must repaint instantly.
pub fn cancel_thumb(canvas_id: &str) {
    if !bridge::has_pdf_reader() {
        return;
    }
    bridge::cancel_thumb(canvas_id);
}

/// Render page 1 of the book at `path` to a small JPEG for the library
/// shelf's book cover. Works whether or not that book is the open document.
pub async fn cover_data_url(path: &str, max_width: f64) -> Result<CoverResult, EngineError> {
    let value = bridge::cover_data_url(path, max_width).await;
    resolve::<CoverResult>(value, "cover").await
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

/// Render a page into the thumbnail cache with no DOM canvas (idle prefetch).
/// Best-effort: fires and forgets — the cache entry lands whenever the raster
/// is ready. Callers use it to warm pages AROUND the reader while idle so a
/// later grid jump mounts every cell as a synchronous cache blit.
pub async fn prefetch_thumb(page: u32, scale: f64) {
    if !bridge::has_pdf_reader() {
        return;
    }
    _ = bridge::prefetch_thumb(page, scale).await;
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
    if !bridge::has_pdf_reader() {
        return;
    }
    bridge::clear_highlights();
}

// --- OS file opening + theme re-bake -------------------------------------

/// Collect the pending OS-opened PDF path from the backend (double-click,
/// "Open with", default-app launch), if any. Consumes it, so a stray double
/// wake-up can never open the same file twice. Resolves None (never errors)
/// outside Tauri and whenever the backend has nothing queued.
pub async fn take_pending_file() -> Option<String> {
    if !bridge::has_tauri() || !bridge::has_pdf_reader() {
        return None;
    }
    let value = bridge::take_pending_file().await;
    value.as_string().filter(|s| !s.is_empty())
}

/// Re-bake the theme into every raster the engine already holds (mounted
/// pages + cached thumbnails). Called by the theme applier right after it
/// writes the new CSS variables; pages render with the new look without a
/// pdf.js re-render.
pub fn refresh_theme() {
    if !bridge::has_pdf_reader() {
        return;
    }
    bridge::refresh_theme();
}

/// Enter/leave appearance-scrub mode. While a slider drag repaints the theme
/// variables every frame, the engine shows the RAW rasters under the live
/// CSS filter/blend (the pre-baking pipeline) so the page re-colours per
/// frame; leaving re-bakes from the raws. The engine swaps canvas contents
/// and the CSS class in the same task, so no frame is ever double-filtered
/// or unfiltered.
pub fn set_scrub_mode(on: bool) {
    if !bridge::has_pdf_reader() {
        return;
    }
    bridge::set_scrub_mode(on);
}

// --- Window chrome ------------------------------------------------------

/// Show/hide the native macOS traffic lights via the backend command. The
/// backend is a no-op outside macOS, and outside Tauri there is nothing to
/// invoke, so this is safe to call unconditionally (the reader-view effect
/// drives it from the hover-reveal signal).
pub async fn set_traffic_lights(visible: bool) {
    if !bridge::has_tauri() {
        return;
    }
    let args = js_sys::Object::new();
    _ = js_sys::Reflect::set(&args, &JsValue::from_str("visible"), &JsValue::from_bool(visible));
    _ = bridge::tauri_invoke("set_traffic_lights", args.into()).await;
}
