//! The plain-text format artifact. One instance, one book. It does not open a PDF.

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub async fn mount(payload: String) {
    mareader::format_runtime::mount(payload).await;
}

#[wasm_bindgen]
pub fn detach() {
    mareader::format_runtime::detach();
}

#[wasm_bindgen]
pub async fn dispose() {
    mareader::format_runtime::dispose().await;
}
