//! The standalone shelf's cover renderer (deployment-only, never hosted).
//!
//! A hosted library frame has no engine: its bake queue crosses the Shell
//! boundary (`ShellApi::bake_cover`) and answers arrive as `coverBaked`
//! commands. The STANDALONE artifact (`library.html` on its own) deploys the
//! engine bundle beside itself, because there is no Shell to ask. That is a
//! property of the page, not of this crate: this file speaks to
//! `window.PDFReader` through reflection alone, exactly the way the engine
//! facade has always been reached from JS, so the library's dependency graph
//! carries zero PDF execution code in both deployments.
//!
//! Absence of the global is a normal failure answer, not a panic: the hosted
//! queue never calls this, and a standalone page whose engine script did not
//! load still runs the whole shelf — those covers simply wait for their
//! books' first open, which bakes them reader-side anyway.

use runtime_contract::covers::CoverImage;

/// The result of one bake attempt. Failures are unreported by design: the
/// caller's queue policy owns retries.
pub struct Baked(pub Option<CoverImage>);

/// Bake page 1 of `path` at the shared cover width through the page's engine
/// global. Answers `Baked(None)` when the engine is absent, the call rejects,
/// or the payload is not a cover.
#[cfg(target_arch = "wasm32")]
pub async fn bake(path: &str, max_width: f64) -> Baked {
    use wasm_bindgen::{JsCast, JsValue};

    let Some(window) = web_sys::window() else {
        return Baked(None);
    };
    let target: js_sys::Object = window.unchecked_into();
    let Ok(engine) = js_sys::Reflect::get(&target, &JsValue::from_str("PDFReader")) else {
        return Baked(None);
    };
    if engine.is_undefined() || engine.is_null() {
        return Baked(None);
    }
    let engine: js_sys::Object = engine.unchecked_into();
    let Ok(call) = js_sys::Reflect::get(&engine, &JsValue::from_str("coverDataUrl")) else {
        return Baked(None);
    };
    let call: js_sys::Function = call.unchecked_into();
    let Ok(promise) = call.call2(
        &engine,
        &JsValue::from_str(path),
        &JsValue::from_f64(max_width),
    ) else {
        return Baked(None);
    };
    let Ok(future) = promise.dyn_into::<js_sys::Promise>() else {
        return Baked(None);
    };
    let Ok(value) = wasm_bindgen_futures::JsFuture::from(future).await else {
        return Baked(None);
    };
    match serde_wasm_bindgen::from_value::<CoverImage>(value) {
        Ok(image) => Baked(Some(image)),
        Err(_) => Baked(None),
    }
}

/// The host path: no engine global exists, so every bake answers failure.
/// Keeps the crate's host lanes (clippy, unit tests) compiling the same call
/// sites the wasm target sees.
#[cfg(not(target_arch = "wasm32"))]
pub async fn bake(_path: &str, _max_width: f64) -> Baked {
    Baked(None)
}
