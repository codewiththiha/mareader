//! Page surfaces: registration, live renders, thumbnail lane.

use crate::bridge;
use crate::types::{RenderResult, ThumbResult};

use super::{EngineError, guard_pdf_reader, require_pdf_reader, resolve};

/// Register a page's canvas with the engine (virtualized rows call this on
/// mount). Typed end to end: the bridge takes primitives, so a mount allocates
/// no serde payload — on a fast scroll a windowful of mounts used to build one
/// `{ok:...}`-shaped object each.
///
/// `host_id` is optional: `None` means the canvas id derives the host id, and
/// travels as `""` (which the engine treats exactly like `undefined`).
pub fn register_page(page: u32, canvas_id: &str, host_id: Option<&str>) {
    if !guard_pdf_reader() {
        return;
    }
    bridge::register_page(page, canvas_id, host_id.unwrap_or(""));
}

pub fn unregister_page(canvas_id: &str) {
    if !guard_pdf_reader() {
        return;
    }
    bridge::unregister_page(canvas_id);
}

pub async fn render_page(
    canvas_id: &str,
    scale: f64,
    render_text: bool,
) -> Result<RenderResult, EngineError> {
    require_pdf_reader()?;
    let value = bridge::render_page(canvas_id, scale, render_text).await;
    resolve::<RenderResult>(value, "render")
}

/// Render one thumbnail through the engine's cached thumbnail lane. Unlike
/// `render_page` this needs no `register_page` (the engine resolves the canvas
/// by id per call) and never builds a text layer. When the page's bitmap is
/// already cached the engine blits it synchronously, so the canvas is painted
/// on the first mounted frame — a caller that needs to know BEFORE that frame
/// asks [`has_thumb`] instead of waiting on this promise.
pub async fn render_thumb(
    canvas_id: &str,
    page: u32,
    scale: f64,
) -> Result<ThumbResult, EngineError> {
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
/// is ready. Callers warm pages AROUND the reader while idle so a later grid
/// jump mounts every cell as a synchronous cache blit.
pub async fn prefetch_thumb(page: u32, scale: f64) {
    if !guard_pdf_reader() {
        return;
    }
    _ = bridge::prefetch_thumb(page, scale).await;
}

/// Park (or reopen) the engine's full-page render lane. Fired by the reader
/// on every scroll-phase transition: parked while a fling is in flight so a
/// full-page raster is never issued for a page the reader is sweeping past.
pub fn set_render_gate(parked: bool) {
    if !guard_pdf_reader() {
        return;
    }
    bridge::set_render_gate(parked);
}

/// Jump the queued renders of `pages` to the front of the engine lane — the
/// settle flush: the pages the reader landed on rasterise first.
pub fn promote_pages(pages: &[u32]) {
    if !guard_pdf_reader() {
        return;
    }
    bridge::promote_pages(pages);
}

/// Drop the farthest-held raw rasters, measured out from `center_page`,
/// until the engine's retained raw bytes are inside `budget_bytes`.
pub fn enforce_page_budget(center_page: u32, budget_bytes: u32) {
    if !guard_pdf_reader() {
        return;
    }
    bridge::enforce_page_budget(center_page, budget_bytes);
}

/// Render the document's ghost placeholder (the modal page, tiny and
/// desaturated) and resolve its blob URL, or `None` when it cannot. The
/// engine dedupes against its theme pipeline, so a repeat call with an
/// unchanged appearance resolves the cached URL.
pub async fn render_ghost(page: u32, height_px: f64) -> Option<String> {
    if !guard_pdf_reader() {
        return None;
    }
    bridge::render_ghost(page, height_px).await.as_string()
}
