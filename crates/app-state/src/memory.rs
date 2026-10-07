//! The wasm heap probe: the one memory number the app can read about itself.

/// The wasm linear memory's current size in bytes; `None` off wasm (host
/// tests).
pub fn wasm_heap_bytes() -> Option<u64> {
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsValue;
        // `wasm_bindgen::memory()`'s buffer length read by reflection.
        let memory = wasm_bindgen::memory();
        let buffer = js_sys::Reflect::get(&memory, &JsValue::from_str("buffer")).ok()?;
        let bytes = js_sys::Reflect::get(&buffer, &JsValue::from_str("byteLength")).ok()?;
        bytes.as_f64().map(|b| b as u64)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        None
    }
}

/// Log the heap's size under a tag at the points that move it.
pub fn log_heap(tag: &str) {
    if let Some(bytes) = wasm_heap_bytes() {
        let mb = bytes as f64 / (1024.0 * 1024.0);
        web_sys::console::log_1(&format!("[mem] {tag}: wasm heap {mb:.1} MB").into());
    }
}
