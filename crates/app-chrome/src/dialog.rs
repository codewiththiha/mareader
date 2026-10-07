//! The native open-file dialog, admitting every format the reader opens.

use wasm_bindgen::JsValue;

/// The sentence a cancelled pick answers with.
pub const CANCELLED: &str = "Open cancelled";

/// Native open-file dialog, filtered to the reader's formats.
pub async fn pick_document() -> Result<String, String> {
    if !tauri_bridge::has_tauri() {
        return Err(
            "Open dialog only available in the desktop app. Drag and drop a document instead."
                .to_string(),
        );
    }

    let opts: JsValue = js_sys::Object::new().into();
    let _ = js_sys::Reflect::set(&opts, &JsValue::from_str("multiple"), &JsValue::FALSE);
    let filter: JsValue = js_sys::Object::new().into();
    let _ = js_sys::Reflect::set(
        &filter,
        &JsValue::from_str("name"),
        &JsValue::from_str("Documents"),
    );
    let exts = js_sys::Array::new();
    for ext in reader_core::format::extensions() {
        exts.push(&JsValue::from_str(ext));
    }
    let _ = js_sys::Reflect::set(&filter, &JsValue::from_str("extensions"), &exts);
    let filters = js_sys::Array::new();
    filters.push(&filter);
    let _ = js_sys::Reflect::set(&opts, &JsValue::from_str("filters"), &filters);

    let value = tauri_bridge::open(opts).await.map_err(|error| {
        let detail = error.as_string().unwrap_or_else(|| format!("{error:?}"));
        format!("Open dialog failed: {detail}")
    })?;
    match value.as_string() {
        Some(path) if !path.is_empty() => Ok(path),
        _ => Err(CANCELLED.to_string()),
    }
}
