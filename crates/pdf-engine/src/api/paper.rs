//! The paper pipeline's engine plumbing: frames in, colours and switches out.
//! The colour DECISIONS live in `crate::backdrop` (the session state machine);
//! this module only shuttles pixels and CSS variables across the bridge.

use wasm_bindgen::JsValue;

use super::{
    guard_pdf_reader, reflect_get, resolve, EngineError, KEY_DATA, KEY_HEIGHT, KEY_OK, KEY_PAGE,
    KEY_WIDTH,
};
use crate::bridge;

/// A raw page frame handed over by the engine: the raster downscaled to a
/// ≤96px long edge, with its pixels — the input every colour decision in the
/// `pdf-paper` crate runs on.
pub use crate::types::PaperFrame;

/// Drain the raw frame a live render of `canvas_id` stashed at the one
/// pipeline moment the page's own paper is still unbaked. `None` when the
/// canvas has nothing stashed (no render yet, or already drained).
pub fn take_paper_frame(canvas_id: &str) -> Option<PaperFrame> {
    if !guard_pdf_reader() {
        return None;
    }
    parse_frame(&bridge::take_paper_frame(canvas_id))
}

/// Render `page` offscreen at a tiny scale and return its frame — the
/// look-ahead's samples come through here.
/// `Ok(None)` when the engine has no answer for the page (render failed).
pub async fn sample_paper_page(page: u32) -> Result<Option<PaperFrame>, EngineError> {
    if !guard_pdf_reader() {
        return Ok(None);
    }
    let value = bridge::sample_paper_page(page).await;
    resolve_frame(value, &format!("samplePaperPage({page})"))
}

/// Publish (or, with `None`, clear) `--pdf-paper`.
pub fn set_paper(hex: Option<&str>) {
    if !guard_pdf_reader() {
        return;
    }
    bridge::set_paper(hex.unwrap_or(""));
}

/// Tell the engine whether the paper session wants frames at all: while
/// blend mode is off, the renderer skips the per-render ≤96px downscale +
/// readback that `stashPaperFrame` exists to pay. Called by
/// `backdrop::configure` on every settings change — the engine-side flag is
/// idempotent.
pub fn set_paper_active(on: bool) {
    if guard_pdf_reader() {
        bridge::set_paper_active(on);
    }
}

/// The shape `resolve` deserialises a frameless `{ok:false, error}` into:
/// nothing but the envelope, which `resolve` itself consumes.
#[derive(Debug, serde::Deserialize)]
struct Empty {}

/// Parse a `{ok, page, width, height, data}` frame payload. The pixels come
/// back as a typed array, not JSON, so the fields are read by hand.
fn parse_frame(value: &JsValue) -> Option<PaperFrame> {
    let ok = reflect_get(value, &KEY_OK)
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !ok {
        return None;
    }
    let number = |name: &'static std::thread::LocalKey<JsValue>| -> Option<f64> {
        reflect_get(value, name).ok().and_then(|v| v.as_f64())
    };
    let page = number(&KEY_PAGE)? as u32;
    let width = number(&KEY_WIDTH)? as u32;
    let height = number(&KEY_HEIGHT)? as u32;
    let data = reflect_get(value, &KEY_DATA).ok()?;
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
    let ok = reflect_get(&value, &KEY_OK)
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if ok {
        return Ok(None);
    }
    // `{ok:false, error}` — surface it through the shared error path (which
    // always errs here; the Ok arm is unreachable and defensive).
    match resolve::<Empty>(value, what) {
        Err(e) => Err(e),
        Ok(_) => Ok(None),
    }
}
