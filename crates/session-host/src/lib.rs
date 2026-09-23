//! The reader-host artifact. Chrome only. The shelf is a different binary and
//! is not running while this one is mounted.

use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub async fn mount() {
    mareader::session_host::mount().await;
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn detach() {
    mareader::session_host::detach();
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub async fn dispose() {
    mareader::session_host::dispose().await;
}

#[cfg(not(target_arch = "wasm32"))]
#[wasm_bindgen]
pub fn mount() {}

#[cfg(not(target_arch = "wasm32"))]
#[wasm_bindgen]
pub fn detach() {}

#[cfg(not(target_arch = "wasm32"))]
#[wasm_bindgen]
pub fn dispose() {}
