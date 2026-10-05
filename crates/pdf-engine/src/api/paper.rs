//! The paper pipeline's frame parser. The colour DECISIONS live in
//! `crate::backdrop` (each session's state machine); the bridge calls that
//! carry frames and papers are [`crate::session::PdfSession`]'s. This module
//! only turns the engine's frame payloads into [`PaperFrame`]s.

use wasm_bindgen::JsValue;

use super::{EngineError, KEY_DATA, KEY_HEIGHT, KEY_OK, KEY_PAGE, KEY_WIDTH, reflect_get, resolve};

/// A raw page frame handed over by the engine: the raster downscaled to a
/// ≤96px long edge, with its pixels — the input every colour decision in the
/// `pdf-paper` crate runs on.
pub use crate::types::PaperFrame;

/// The shape `resolve` deserialises a frameless `{ok:false, error}` into:
/// nothing but the envelope, which `resolve` itself consumes.
#[derive(Debug, serde::Deserialize)]
struct Empty {}

/// Parse a `{ok, page, width, height, data}` frame payload. The pixels come
/// back as a typed array, not JSON, so the fields are read by hand.
pub(crate) fn parse_frame(value: &JsValue) -> Option<PaperFrame> {
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

pub(crate) fn resolve_frame(value: JsValue, what: &str) -> Result<Option<PaperFrame>, EngineError> {
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
    // `{ok:false, error}` — surface it through the shared error path.
    // `resolve` errs whenever the envelope's `ok` is false, which the check
    // above has just established, so the success arm never runs and its
    // payload is discarded.
    resolve::<Empty>(value, what)?;
    Ok(None)
}
