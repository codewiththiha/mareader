//! Typed wrappers over the JS engine (window.PDFReader). This is the ONLY module
//! that calls engine functions; views and effects never touch wasm-bindgen types.
//!
//! Every engine fn resolves to `{ok:true, ...}` or `{ok:false, error:{name,message}}`
//! including `buildSearchIndex` (`{ok, count}`). We check `ok` here and
//! surface a `Result<T, String>`.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use wasm_bindgen::JsValue;

use crate::bridge;
use crate::types::{CoverResult, OpenResult, RenderResult, ThumbResult};
use pdf_core::search::SearchResponse;
use pdf_paper::PaperArea;

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

fn require_pdf_reader() -> Result<(), EngineError> {
    if bridge::has_pdf_reader() {
        Ok(())
    } else {
        Err(EngineError {
            name: "no_engine".to_string(),
            message: "PDF engine is not loaded yet. Restart the app and try again.".to_string(),
        })
    }
}

fn guard_pdf_reader() -> bool {
    bridge::has_pdf_reader()
}

/// Parses a `{ok:bool, error?:{name,message}, ...fields}` value into `T`.
/// Pure parsing — no JS awaits — so the whole engine-answer path that needs
/// no Promise (e.g. the synchronous paper-cache lookup) can use it too.
fn resolve<T: DeserializeOwned>(value: JsValue, what: &str) -> Result<T, EngineError> {
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
    require_pdf_reader()?;
    let value = bridge::open(path).await;
    resolve::<OpenResult>(value, "open")
}

/// `{ok:true, outline}` — engine.resolveOutline.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct OutlinePayload {
    outline: Vec<crate::types::OutlineNode>,
}

/// The open document's chapter tree, flattened. The open flow asks for this
/// AFTER the reader is up: resolving every outline destination is a per-entry
/// worker round trip, and holding `open` hostage to it was most of the
/// document-opening lag on textbook-sized outlines. `Ok(empty)` when the
/// book has no outline or the engine has no answer.
pub async fn outline() -> Result<Vec<crate::types::OutlineNode>, EngineError> {
    if !guard_pdf_reader() {
        return Ok(Vec::new());
    }
    let value = bridge::resolve_outline().await;
    let payload: OutlinePayload = resolve(value, "resolveOutline")?;
    Ok(payload.outline)
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
    if !guard_pdf_reader() {
        return;
    }
    let payload = RegisterPagePayload {
        page,
        canvas_id: canvas_id.to_string(),
        host_id: host_id.map(str::to_string),
    };
    let Ok(value) = serde_wasm_bindgen::to_value(&payload) else {
        return;
    };
    bridge::register_page(value);
}

pub fn unregister_page(canvas_id: &str) {
    if !guard_pdf_reader() {
        return;
    }
    bridge::unregister_page(canvas_id);
}

pub async fn render_page(canvas_id: &str, scale: f64, render_text: bool) -> Result<RenderResult, EngineError> {
    require_pdf_reader()?;
    let value = bridge::render_page(canvas_id, scale, render_text).await;
    resolve::<RenderResult>(value, "render")
}

/// Render one thumbnail through the engine's cached thumbnail lane.
///
/// Unlike `render_page` this needs no `register_page` (the engine resolves the
/// canvas by id per call) and never builds a text layer. When the page's bitmap
/// is already cached the engine blits it synchronously and returns
/// `cached: true` — the caller must then skip its loading skeleton, because the
/// canvas is already painted on the first mounted frame.
pub async fn render_thumb(canvas_id: &str, page: u32, scale: f64) -> Result<ThumbResult, EngineError> {
    require_pdf_reader()?;
    let value = bridge::render_thumb(canvas_id, page, scale).await;
    resolve::<ThumbResult>(value, "thumb")
}

/// Cancel an in-flight thumbnail render (cell unmounted). Does NOT evict the
/// cached bitmap: a page that scrolls out and back must repaint instantly.
pub fn cancel_thumb(canvas_id: &str) {
    if !guard_pdf_reader() {
        return;
    }
    bridge::cancel_thumb(canvas_id);
}

/// Render page 1 of the book at `path` to a small JPEG for the library
/// shelf's book cover. Works whether or not that book is the open document.
pub async fn cover_data_url(path: &str, max_width: f64) -> Result<CoverResult, EngineError> {
    require_pdf_reader()?;
    let value = bridge::cover_data_url(path, max_width).await;
    resolve::<CoverResult>(value, "cover")
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
    if !guard_pdf_reader() {
        return;
    }
    _ = bridge::prefetch_thumb(page, scale).await;
}

#[derive(serde::Deserialize)]
struct SearchIndexResult {
    count: u32,
}

pub async fn build_search_index() -> Result<u32, EngineError> {
    require_pdf_reader()?;
    let value = bridge::build_search_index().await;
    resolve::<SearchIndexResult>(value, "build_search_index")
        .map(|r| r.count)
}

pub async fn search(query: &str) -> Result<SearchResponse, EngineError> {
    let value = bridge::search(query).await;
    resolve::<SearchResponse>(value, "search")
}

/// Mark occurrence `index` of `page` as the current match (`index < 0` clears).
pub fn set_active_match(page: u32, index: i32) {
    bridge::set_active_match(page, index);
}

pub fn clear_highlights() {
    if !guard_pdf_reader() {
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
    if !guard_pdf_reader() {
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

// --- Paper pipeline --------------------------------------------------------

/// A raw page frame handed over by the engine: the raster downscaled to a
/// ≤96px long edge, with its pixels — the input every colour decision in the
/// `pdf-paper` crate runs on.
pub use crate::types::PaperFrame;

/// Drain the raw frame a live render of `canvas_id` stashed at the one
/// pipeline moment the page's own paper is still unbaked. `None` when the
/// canvas has nothing stashed (no render yet, or already drained).
pub(crate) fn take_paper_frame(canvas_id: &str) -> Option<PaperFrame> {
    if !guard_pdf_reader() {
        return None;
    }
    parse_frame(&bridge::take_paper_frame(canvas_id))
}

/// Render `page` offscreen at a tiny scale and return its frame — the fixed
/// scan's samples and the continuous look-ahead both come through here.
/// `Ok(None)` when the engine has no answer for the page (render failed).
pub(crate) async fn sample_paper_page(page: u32) -> Result<Option<PaperFrame>, EngineError> {
    if !guard_pdf_reader() {
        return Ok(None);
    }
    let value = bridge::sample_paper_page(page).await;
    resolve_frame(value, &format!("samplePaperPage({page})"))
}

/// A cached fixed-mode colour, and the detection area it was computed
/// under — a cache written for whole-page detection is not valid under
/// edge detection, so the session treats an area mismatch as a miss.
#[derive(Debug, Clone, PartialEq)]
pub struct CachedPaper {
    pub hex: String,
    pub area: PaperArea,
}

/// The cached fixed-mode colour for `path`, if the engine remembers one
/// **and** it was found under `area`.
///
/// Synchronous end to end — the TS side is a plain localStorage read — so
/// the open flow can consult it BEFORE the reader view mounts without an
/// await, and a hit repaints the backdrop in the reader's very first frame.
pub(crate) fn cached_paper(path: &str, area: PaperArea) -> Result<Option<CachedPaper>, EngineError> {
    if !guard_pdf_reader() {
        return Ok(None);
    }
    let value = bridge::get_cached_paper(path);
    let payload: PaperCacheResult = resolve(value, "getCachedPaper")?;
    let Some(hex) = payload.hex.filter(|h| !h.is_empty()) else {
        return Ok(None);
    };
    if payload.area != Some(area.engine_id().to_string()) {
        return Ok(None); // cached under the other detection area: a miss
    }
    Ok(Some(CachedPaper { hex, area }))
}

/// Publish (or, with `None`, clear) `--pdf-paper`. `persist` also writes the
/// per-document cache under `area` — call it exactly once per resolved book
/// colour.
pub(crate) fn set_paper(hex: Option<&str>, persist: bool, area: PaperArea) {
    if !guard_pdf_reader() {
        return;
    }
    match hex {
        Some(hex) => bridge::set_paper(hex, persist, area.engine_id()),
        None => bridge::set_paper("", false, area.engine_id()),
    }
}

/// Tell the engine whether the paper session wants frames at all: while
/// blend mode is off, the renderer skips the per-render ≤96px downscale +
/// readback that `stashPaperFrame` exists to pay. Called by `paper::configure`
/// on every settings change — the engine-side flag is idempotent.
pub(crate) fn set_paper_active(on: bool) {
    if guard_pdf_reader() {
        bridge::set_paper_active(on);
    }
}

/// Bank `hex` as the current book's fixed colour under `area` WITHOUT
/// publishing it — the paper session's close path, when the backdrop is
/// being cleared but an interrupted scan's answer is still worth
/// remembering for the next open.
pub(crate) fn persist_paper(hex: &str, area: PaperArea) {
    if !guard_pdf_reader() {
        return;
    }
    bridge::persist_paper(hex, area.engine_id());
}

/// `{ok:true, hex, area}` — engine.getCachedPaper. `hex` is null on a miss;
/// `area` is the detection area the colour was cached under.
#[derive(Debug, Serialize, Deserialize)]
struct PaperCacheResult {
    #[serde(default)]
    hex: Option<String>,
    #[serde(default)]
    area: Option<String>,
}

/// Parse a `{ok, page, width, height, data}` frame payload. The pixels come
/// back as a typed array, not JSON, so the fields are read by hand.
fn parse_frame(value: &JsValue) -> Option<PaperFrame> {
    let ok = js_sys::Reflect::get(value, &JsValue::from_str("ok"))
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !ok {
        return None;
    }
    let number = |name: &str| -> Option<f64> {
        js_sys::Reflect::get(value, &JsValue::from_str(name))
            .ok()
            .and_then(|v| v.as_f64())
    };
    let page = number("page")? as u32;
    let width = number("width")? as u32;
    let height = number("height")? as u32;
    let data = js_sys::Reflect::get(value, &JsValue::from_str("data")).ok()?;
    let data = js_sys::Uint8ClampedArray::from(data).to_vec();
    Some(PaperFrame {
        page,
        width,
        height,
        data,
    })
}

fn resolve_frame(value: JsValue, what: &str) -> Result<Option<PaperFrame>, EngineError> {
    if let Some(frame) = parse_frame(&value) {
        return Ok(Some(frame));
    }
    // `{ok:true}` with no frame is the engine's "no answer for this page" —
    // a skipped page, not a failure to communicate.
    let ok = js_sys::Reflect::get(&value, &JsValue::from_str("ok"))
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if ok {
        return Ok(None);
    }
    // `{ok:false, error}` — surface it through the shared error path (which
    // always errs here; the Ok arm is unreachable and defensive).
    match resolve::<PaperCacheResult>(value, what) {
        Err(e) => Err(e),
        Ok(_) => Ok(None),
    }
}

/// Release rasters/caches the engine no longer needs. Call after zoom
/// commits, mode flips and scroll idle so memory drops immediately.
pub fn sweep() {
    if !guard_pdf_reader() {
        return;
    }
    bridge::sweep();
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

// --- AI word explanation --------------------------------------------------

/// Fire-and-forget start of an `explain_word` run on the backend. Nothing
/// comes back here: the backend streams its chunks over the
/// `ai-stream-chunk` event, which the frontend AI service listens to.
/// Outside Tauri (`trunk serve`) there is nothing to invoke, so this is a
/// silent no-op; a genuine invoke failure surfaces as `Err` for the caller
/// to log.
pub async fn explain_word(word: &str, context: &str) -> Result<(), String> {
    if !bridge::has_tauri() {
        return Ok(());
    }
    let args = js_sys::Object::new();
    _ = js_sys::Reflect::set(&args, &JsValue::from_str("word"), &JsValue::from_str(word));
    _ = js_sys::Reflect::set(&args, &JsValue::from_str("context"), &JsValue::from_str(context));
    bridge::tauri_invoke("explain_word", args.into())
        .await
        .map(|_| ())
        .map_err(|e| e.as_string().unwrap_or_else(|| "unknown invoke error".to_string()))
}
