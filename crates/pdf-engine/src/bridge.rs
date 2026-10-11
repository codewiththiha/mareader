//! Wasm-bindgen interop with the imperative PDF engine.
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    // --- Session lifecycle ------------------------------------------------

    /// Register engine session `sid`; a sid is never reused.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "createSession")]
    pub fn create_session(sid: u32) -> bool;

    /// Tear session `sid` down; idempotent.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "destroySession")]
    pub async fn destroy_session(sid: u32) -> JsValue;

    /// Make `sid` the session whose paper the root backdrop shows.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "presentSession")]
    pub fn present_session(sid: u32);

    /// One session's gauges and counters (`Stats`), or null for an unknown
    /// sid.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "sessionStats")]
    pub fn session_stats(sid: u32) -> JsValue;

    // --- Document ---------------------------------------------------------

    /// Open `path` in session `sid` — a session holds exactly one document.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"])]
    pub async fn open(sid: u32, path: &str) -> JsValue;

    // The chapter tree is resolved after open, one round trip per entry.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "resolveOutline")]
    pub async fn resolve_outline(sid: u32) -> JsValue;

    /// Render page 1 of the book to a JPEG data URL, for the shelf cover.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "coverDataUrl")]
    pub async fn cover_data_url(sid: u32, path: &str, max_width: f64) -> JsValue;

    // --- Pages ------------------------------------------------------------

    /// Register a page's canvas with the session, typed and id-free.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "registerPage")]
    pub fn register_page(
        sid: u32,
        page: u32,
        canvas_id: &str,
        host_id: &str,
        canvas: Option<&web_sys::Element>,
        host: Option<&web_sys::Element>,
    );

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "unregisterPage")]
    pub fn unregister_page(sid: u32, canvas_id: &str);

    /// Cancel every in-flight page render of the session in one call.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "cancelPageRenders")]
    pub fn cancel_page_renders(sid: u32);

    /// Queue one page's raster; `rank` orders the lane.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "renderPage")]
    pub async fn render_page(
        sid: u32,
        canvas_id: &str,
        scale: f64,
        render_text: bool,
        rank: u32,
    ) -> JsValue;

    /// Stop one page's queued or in-flight raster; it stays registered.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "cancelPage")]
    pub fn cancel_page(sid: u32, canvas_id: &str);

    /// Re-rank one page's queued raster; a rank read at dequeue time.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "reprioritizePage")]
    pub fn reprioritize_page(sid: u32, canvas_id: &str, rank: u32);

    /// One page's intrinsic box, without rasterising it.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "probePageSize")]
    pub async fn probe_page_size(sid: u32, page: u32) -> JsValue;

    // --- Thumbnails -------------------------------------------------------

    // Thumbnail lane: a cheap render path with a per-session bitmap cache.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "renderThumb")]
    pub async fn render_thumb(sid: u32, canvas_id: &str, page: u32, scale: f64) -> JsValue;

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "cancelThumb")]
    pub fn cancel_thumb(sid: u32, canvas_id: &str);

    /// SYNCHRONOUS cache probe, read while a thumbnail cell builds.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "hasThumb")]
    pub fn has_thumb(sid: u32, page: u32, scale: f64) -> bool;

    /// Render a page into the thumbnail cache with no DOM canvas.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "prefetchThumb")]
    pub async fn prefetch_thumb(sid: u32, page: u32, scale: f64) -> JsValue;

    /// The pane left the screen: abandon the idle prefetches.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "suspendPrefetches")]
    pub fn suspend_prefetches(sid: u32);

    /// The pane is on screen again: the session's idle prefetch may run.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "resumePrefetches")]
    pub fn resume_prefetches(sid: u32);

    // --- Search -----------------------------------------------------------

    /// Extract one page's text runs (`{ok, page, items:[{str,x,y,w,h}]}`)
    /// for the session's Rust-owned search index.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "extractPageText")]
    pub async fn extract_page_text(sid: u32, page: u32) -> JsValue;

    /// Publish the active query to the session's text layers.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "setSearchContext")]
    pub fn set_search_context(sid: u32, query: &str);

    /// Emphasise occurrence `index` of `page` as the current match.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "setActiveMatch")]
    pub fn set_active_match(sid: u32, page: u32, index: i32);

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "clearHighlights")]
    pub fn clear_highlights(sid: u32);

    // --- Paper ------------------------------------------------------------

    // The paper pipeline's eyes: the engine owns the canvases, not the colours.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "setPaper")]
    pub fn set_paper(sid: u32, hex: &str);

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "setPaperActive")]
    pub fn set_paper_active(sid: u32, on: bool);

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "takePaperFrame")]
    pub fn take_paper_frame(sid: u32, canvas_id: &str) -> JsValue;

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "samplePaperPage")]
    pub async fn sample_paper_page(sid: u32, page: u32) -> JsValue;

    // --- Memory -----------------------------------------------------------

    /// Release rasters and caches the session no longer needs.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "sweep")]
    pub fn sweep(sid: u32);

    /// Drop the `.page-snapshot` scrub covers of the session's page hosts.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "sweepSnapshots")]
    pub fn sweep_snapshots(sid: u32);

    // --- Realm: appearance broadcast --------------------------------------

    // Appearance is global; the rasters are session-owned.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "refreshTheme")]
    pub fn refresh_theme();

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "setScrubMode")]
    pub fn set_scrub_mode(on: bool);

    // The appearance popover is open: each session retains its raws.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "setAppearanceMenuOpen")]
    pub fn set_appearance_menu_open(on: bool);

    // --- Realm: diagnostics -----------------------------------------------

    /// The realm aggregate: every session's gauges plus the totals.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"])]
    pub fn stats() -> JsValue;

    /// Turn the engine's lifecycle event narration on/off.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "setLifecycleLog")]
    pub fn set_lifecycle_log(on: bool);
}

/// True when `window.PDFReader` exists; check before any call.
pub fn has_pdf_reader() -> bool {
    if !cfg!(target_arch = "wasm32") {
        return false;
    }
    web_sys::window()
        .map(|w| {
            let g: js_sys::Object = w.unchecked_into();
            js_sys::Reflect::get(&g, &JsValue::from_str("PDFReader"))
                .map(|v| !(v.is_undefined() || v.is_null()))
                .unwrap_or(false)
        })
        .unwrap_or(false)
}

/// Release engine session `sid` without awaiting it.
pub fn release_session_detached(sid: u32) {
    if !cfg!(target_arch = "wasm32") {
        return;
    }
    let Some(window) = web_sys::window() else {
        return;
    };
    let global: js_sys::Object = window.unchecked_into();
    let Ok(reader) = js_sys::Reflect::get(&global, &JsValue::from_str("PDFReader")) else {
        return;
    };
    let Ok(destroy) = js_sys::Reflect::get(&reader, &JsValue::from_str("destroySession")) else {
        return;
    };
    if let Some(destroy) = destroy.dyn_ref::<js_sys::Function>() {
        let _ = destroy.call1(&reader, &JsValue::from_f64(f64::from(sid)));
    }
}
