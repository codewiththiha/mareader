//! The native open-file dialog (Tauri dialog plugin).

use wasm_bindgen::JsValue;

use super::{
    reflect_set, KEY_DIRECTORY, KEY_EXTENSIONS, KEY_FILTERS, KEY_MULTIPLE, KEY_NAME, KEY_PDF,
};

/// Native open-file dialog (Tauri dialog plugin). Returns the chosen path, or
/// `Err` on cancel / no plugin.
pub async fn pick_pdf() -> Result<String, String> {
    if !tauri_bridge::has_tauri() {
        return Err(
            "Open dialog only available in the desktop app. Drag and drop a PDF instead."
                .to_string(),
        );
    }

    let opts: JsValue = js_sys::Object::new().into();
    _ = reflect_set(&opts, &KEY_MULTIPLE, &JsValue::FALSE);
    _ = reflect_set(&opts, &KEY_DIRECTORY, &JsValue::FALSE);
    let filter: JsValue = js_sys::Object::new().into();
    let pdf_name = KEY_PDF.with(|v| v.clone());
    _ = reflect_set(&filter, &KEY_NAME, &pdf_name);
    let exts = js_sys::Array::new();
    exts.push(&JsValue::from_str("pdf"));
    _ = reflect_set(&filter, &KEY_EXTENSIONS, &exts);
    let filters = js_sys::Array::new();
    filters.push(&filter);
    _ = reflect_set(&opts, &KEY_FILTERS, &filters);

    let value = tauri_bridge::open(opts).await.map_err(|error| {
        let detail = error.as_string().unwrap_or_else(|| format!("{error:?}"));
        format!("Open dialog failed: {detail}")
    })?;
    match value.as_string() {
        Some(path) if !path.is_empty() => Ok(path),
        _ => Err("Open cancelled".to_string()),
    }
}
