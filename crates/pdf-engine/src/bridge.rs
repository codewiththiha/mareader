//! Wasm-bindgen interop with the imperative PDF engine.
//!
//! The ONLY place that declares the `window.PDFReader` externs
//! (public/pdfEngine.js). Callers go through [`crate::session::PdfSession`]
//! (document work) or `crate::api` (the realm-level calls), never here
//! directly (except the probes re-exported at the crate root). The
//! `window.__TAURI__` externs belong to the `tauri-bridge` crate, so no
//! format crate owns chrome's IPC surface. The async fns mirror the `invoke`
//! pattern: wasm-bindgen awaits the underlying Promise and yields the
//! resolved JsValue.
//!
//! SESSION CONTRACT: every document call names its session id (`sid`) first.
//! The engine holds no current document — a sid resolves to the one engine
//! session a `PdfSession` registered, an unknown or retired sid resolves to
//! nothing (async calls answer `{ok:false, error:{name:"no_session"}}`, sync
//! calls are no-ops). The realm calls (version, stats, the appearance
//! broadcast, diagnostics) name no sid. The contract is textual too:
//! `tools/check-versions.ts` and `tests/engine_contract.rs` check every name
//! below against the compiled facade.

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    // --- Realm ------------------------------------------------------------

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"])]
    pub fn version() -> String;

    // --- Session lifecycle ------------------------------------------------

    /// Register engine session `sid`. False when the engine refuses it — a
    /// sid is registered once and never reused (sids are monotonic).
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "createSession")]
    pub fn create_session(sid: u32) -> bool;

    /// Tear session `sid` down: cancel its work, destroy its document and its
    /// pdf.js worker, release its caches, forget the sid. Other sessions are
    /// untouched. Idempotent.
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

    // The chapter tree, resolved AFTER open: flattening it means one worker
    // round trip per outline destination, so open() returns without it and
    // the shell asks for it separately once the reader is already up.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "resolveOutline")]
    pub async fn resolve_outline(sid: u32) -> JsValue;

    /// Render page 1 of the book at `path` to a JPEG data URL (the shelf
    /// cover for the document the session opened). Resolves
    /// `{ok, dataUrl, width, height}`.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "coverDataUrl")]
    pub async fn cover_data_url(sid: u32, path: &str, max_width: f64) -> JsValue;

    // --- Pages ------------------------------------------------------------

    /// Register a page's canvas with the session. Typed on purpose: the
    /// caller passes primitives, so a virtualized row's mount allocates no
    /// serde payload object. `host_id` is the page host element id, or ""
    /// when the caller has none — the engine treats "" exactly like
    /// undefined.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "registerPage")]
    pub fn register_page(sid: u32, page: u32, canvas_id: &str, host_id: &str);

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "unregisterPage")]
    pub fn unregister_page(sid: u32, canvas_id: &str);

    /// Cancel every in-flight page render of the session in one call. The
    /// reader's close path issues it synchronously with the click, before
    /// the navigate command crosses to the Shell — teardown itself is
    /// unchanged.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "cancelPageRenders")]
    pub fn cancel_page_renders(sid: u32);

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "renderPage")]
    pub async fn render_page(sid: u32, canvas_id: &str, scale: f64, render_text: bool) -> JsValue;

    // --- Thumbnails -------------------------------------------------------

    // Thumbnail lane: a separate, cheap render path with a per-session
    // bitmap cache. `renderThumb` resolves `{ok, width, height, scale}`. A
    // cache hit still blits synchronously, but a caller that must know
    // BEFORE its first frame asks `hasThumb` below.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "renderThumb")]
    pub async fn render_thumb(sid: u32, canvas_id: &str, page: u32, scale: f64) -> JsValue;

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "cancelThumb")]
    pub fn cancel_thumb(sid: u32, canvas_id: &str);

    /// SYNCHRONOUS cache probe, read while a thumbnail cell builds its view so
    /// a cache-hit cell can mount without a skeleton at all.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "hasThumb")]
    pub fn has_thumb(sid: u32, page: u32, scale: f64) -> bool;

    /// Paint the cached thumbnail of `page` into `canvas_id` as a blurry
    /// placeholder. Best-effort: returns false when there is nothing cached.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "blitThumb")]
    pub fn blit_thumb(sid: u32, canvas_id: &str, page: u32) -> bool;

    /// Render a page into the session's thumbnail cache with no DOM canvas
    /// (idle prefetch). Best-effort; resolves after the raster lands.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "prefetchThumb")]
    pub async fn prefetch_thumb(sid: u32, page: u32, scale: f64) -> JsValue;

    /// The pane left the screen with its document still loaded: abandon the
    /// session's queued and in-flight idle prefetches (they settle as drops)
    /// and refuse new ones until [`resume_prefetches`].
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

    /// Publish the active query to the session's text layers so its mounted
    /// pages repaint their highlight boxes.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "setSearchContext")]
    pub fn set_search_context(sid: u32, query: &str);

    /// Emphasise occurrence `index` of `page` as the current match. `index < 0`
    /// clears the marker without touching the other highlights.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "setActiveMatch")]
    pub fn set_active_match(sid: u32, page: u32, index: i32);

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "clearHighlights")]
    pub fn clear_highlights(sid: u32);

    // --- Paper ------------------------------------------------------------

    // The paper pipeline's eyes: the engine owns the CANVASES; the session's
    // paper state machine (`crate::backdrop`) owns every colour decision.
    //
    // * `setPaper` records (or, with "", clears) the session's paper; the
    //   root `--pdf-paper` shows the presenting session's.
    // * `setPaperActive` gates the per-render frame stash on the blend switch.
    // * `takePaperFrame` drains the raw frame a live render stashed at the one
    //   pipeline moment the page's own paper is still unbaked.
    // * `samplePaperPage` renders `page` offscreen at a tiny scale and resolves
    //   its frame — the look-ahead samples through it.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "setPaper")]
    pub fn set_paper(sid: u32, hex: &str);

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "setPaperActive")]
    pub fn set_paper_active(sid: u32, on: bool);

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "takePaperFrame")]
    pub fn take_paper_frame(sid: u32, canvas_id: &str) -> JsValue;

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "samplePaperPage")]
    pub async fn sample_paper_page(sid: u32, page: u32) -> JsValue;

    // --- Memory -----------------------------------------------------------

    /// Release rasters/caches the session no longer needs (advisory
    /// `pdf.cleanup`). Fired when reading work ends: zoom commit, mode flip,
    /// scroll idle.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "sweep")]
    pub fn sweep(sid: u32);

    /// Drop the `.page-snapshot` scrub covers the session's live page hosts
    /// still carry, zeroing their backing stores first.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "sweepSnapshots")]
    pub fn sweep_snapshots(sid: u32);

    // --- Realm: appearance broadcast --------------------------------------

    // Appearance is a global setting; the rasters it is baked into are
    // session-owned. Each call below fans out to EVERY live session, which
    // re-derives its OWN raster theme on its own queue.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "refreshTheme")]
    pub fn refresh_theme();

    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "setScrubMode")]
    pub fn set_scrub_mode(on: bool);

    // Whether the appearance popover is open. Each session retains its
    // rendered pages' unbaked raws while it is true — the menu is where a
    // scrub is born (public/engine/state.ts, scrubIsPlausible).
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "setAppearanceMenuOpen")]
    pub fn set_appearance_menu_open(on: bool);

    // --- Realm: diagnostics -----------------------------------------------

    /// The realm aggregate of every session's gauges plus the realm counter
    /// totals (the diagnostics snapshot's engine half). Returns the plain
    /// `Stats` object — no `{ok,...}` envelope.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"])]
    pub fn stats() -> JsValue;

    /// Turn the engine's lifecycle event narration on/off.
    #[wasm_bindgen(js_namespace = ["window", "PDFReader"], js_name = "setLifecycleLog")]
    pub fn set_lifecycle_log(on: bool);
}

/// True when `window.PDFReader` exists. Must be checked before any engine
/// call: a missing global makes the wasm-bindgen shim throw, which panics the
/// reactive owner and freezes menus / theme / open.
///
/// The non-wasm short-circuit keeps the check callable from host `cargo test`
/// (the paper session's tests reach this guard); on the host there is no
/// engine, so `false` is also the truthful answer.
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
