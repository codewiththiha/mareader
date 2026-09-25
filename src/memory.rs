//! The wasm heap probe: the one memory number the app can read about itself.
//!
//! The webview's footprint is a high-water latch — WebKit hands freed arenas
//! back to the OS only under pressure, and the wasm linear memory never
//! shrinks at all (`Memory.grow` is monotonic; freeing Rust objects only
//! punches holes inside the arena). The first half is platform bookkeeping;
//! the second is exactly what the app owes it itself to keep low, and it is
//! invisible from the outside: Activity Monitor folds the heap into the
//! webview's total, where canvas surfaces and JSC dominate. So the app
//! charts the heap itself, one console line at each point that moves it —
//! open, close, zoom commit, search-index build, and the reload that resets
//! it.
//!
//! The trace is the leak-versus-latch test: a heap that steps up once per
//! book and never steps down is the ratchet working as the platform
//! dictates; a heap that climbs per open/close CYCLE is a leak, and this log
//! is where that shows up first. Mareader.md, "The memory model".
//!
//! Read the shape, not the level, and only against the same build: a dev
//! wasm heap carries bookkeeping a release one does not, so a number taken
//! under `trunk serve` and one taken from a bundled app are two different
//! instruments. The step-per-book and flat-across-zooms shape holds in both.
//!
//! Off wasm the probe is inert rather than a panic: the wasm-bindgen stubs
//! abort when called natively — the same rule `crate::time` runs its clock
//! under — and a host test has no heap to report.

/// The wasm linear memory's current size in bytes; `None` off wasm (host
/// tests).
pub(crate) fn wasm_heap_bytes() -> Option<u64> {
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsValue;
        // `wasm_bindgen::memory()` is this instance's `WebAssembly.Memory`;
        // reading `buffer.byteLength` by reflection keeps the probe off
        // js-sys's WebAssembly bindings — the same answer with one less
        // typed surface to depend on.
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

/// Log the heap's size under a tag: `[mem] open: wasm heap 64.0 MB`. Called
/// at the points that move the heap — or that must visibly NOT move it,
/// which is what makes the ratchet chartable. Every sample also folds into
/// the diagnostics surface's high-water mark.
pub(crate) fn log_heap(tag: &str) {
    if let Some(bytes) = wasm_heap_bytes() {
        crate::diagnostics::note_heap_sample(bytes);
        let mb = bytes as f64 / (1024.0 * 1024.0);
        web_sys::console::log_1(&format!("[mem] {tag}: wasm heap {mb:.1} MB").into());
    }
}
