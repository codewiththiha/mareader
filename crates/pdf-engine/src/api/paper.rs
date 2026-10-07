//! The paper pipeline's frame parser; colours live in `crate::backdrop`.
use wasm_bindgen::JsValue;

use super::{EngineError, KEY_DATA, KEY_HEIGHT, KEY_OK, KEY_PAGE, KEY_WIDTH, reflect_get, resolve};

/// A raw page frame: the raster downscaled to a ≤96px long edge.
pub use crate::types::PaperFrame;

/// The shape a frameless error envelope deserialises into.
#[derive(Debug, serde::Deserialize)]
struct Empty {}

/// Parse a `{ok, page, width, height, data}` frame payload.
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
    // `{ok:true}` with no frame is "no answer for this page".
    let ok = reflect_get(&value, &KEY_OK)
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if ok {
        return Ok(None);
    }
    // `{ok:false, error}`: surface it through the shared error path.
    resolve::<Empty>(value, what)?;
    Ok(None)
}
