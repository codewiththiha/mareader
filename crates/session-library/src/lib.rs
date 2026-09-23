//! The shelf artifact. One instance, no open book. The reader host is a
//! different binary and does not start on this page.

use wasm_bindgen::prelude::*;

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub async fn mount() {
    mareader::session_library::mount().await;
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn detach() {
    mareader::session_library::detach();
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub async fn dispose() {
    mareader::session_library::dispose().await;
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
