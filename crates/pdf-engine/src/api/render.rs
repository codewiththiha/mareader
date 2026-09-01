//! Page surfaces: registration, live renders, thumbnail lane.

use crate::bridge;
use crate::types::{RenderResult, ThumbResult};

use super::{guard_pdf_reader, require_pdf_reader, resolve, EngineError};

/// Register a page's canvas with the engine (virtualized rows call this on
/// mount). Typed end to end: the bridge takes `(page, canvas_id, host_id)`
/// primitives, so a mount allocates no serde payload — on a fast scroll a
/// windowful of mounts used to build one `{ok:…}`-shaped object each.
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

/// Render one thumbnail through the engine's cached thumbnail lane.
///
/// Unlike `render_page` this needs no `register_page` (the engine resolves the
/// canvas by id per call) and never builds a text layer. When the page's bitmap
/// is already cached the engine blits it synchronously and returns
/// `cached: true` — the caller must then skip its loading skeleton, because the
/// canvas is already painted on the first mounted frame.
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
/// is ready. Callers use it to warm pages AROUND the reader while idle so a
/// later grid jump mounts every cell as a synchronous cache blit.
pub async fn prefetch_thumb(page: u32, scale: f64) {
    if !guard_pdf_reader() {
        return;
    }
    _ = bridge::prefetch_thumb(page, scale).await;
}
