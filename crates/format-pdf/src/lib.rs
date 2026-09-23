//! The PDF format artifact. One instance, one book. The host does not link this crate.

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
pub async fn mount(payload: String) {
    mareader::format_runtime::mount(payload).await;
}

#[wasm_bindgen]
pub async fn dispose() {
    mareader::format_runtime::dispose().await;
}
