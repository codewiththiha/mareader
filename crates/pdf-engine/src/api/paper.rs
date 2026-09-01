//! The paper pipeline's engine plumbing: frames in, colours and switches out.
//! The colour DECISIONS live in `crate::paper` (the session state machine);
//! this module only shuttles pixels and CSS variables across the bridge.

use wasm_bindgen::JsValue;

use pdf_paper::PaperArea;

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

/// Render `page` offscreen at a tiny scale and return its frame — the fixed
/// scan's samples and the continuous look-ahead both come through here.
/// `Ok(None)` when the engine has no answer for the page (render failed).
pub async fn sample_paper_page(page: u32) -> Result<Option<PaperFrame>, EngineError> {
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
pub fn cached_paper(path: &str, area: PaperArea) -> Result<Option<CachedPaper>, EngineError> {
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
pub fn set_paper(hex: Option<&str>, persist: bool, area: PaperArea) {
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
pub fn set_paper_active(on: bool) {
    if guard_pdf_reader() {
        bridge::set_paper_active(on);
    }
}

/// Bank `hex` as the current book's fixed colour under `area` WITHOUT
/// publishing it — the paper session's close path, when the backdrop is
/// being cleared but an interrupted scan's answer is still worth
/// remembering for the next open.
pub fn persist_paper(hex: &str, area: PaperArea) {
    if !guard_pdf_reader() {
        return;
    }
    bridge::persist_paper(hex, area.engine_id());
}

/// `{ok:true, hex, area}` — engine.getCachedPaper. `hex` is null on a miss;
/// `area` is the detection area the colour was cached under.
#[derive(Debug, serde::Serialize, serde::Deserialize)]
struct PaperCacheResult {
    #[serde(default)]
    hex: Option<String>,
    #[serde(default)]
    area: Option<String>,
}

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
    match resolve::<PaperCacheResult>(value, what) {
        Err(e) => Err(e),
        Ok(_) => Ok(None),
    }
}
