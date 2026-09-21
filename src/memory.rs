//! The memory probe: the numbers the app can read about itself.
//!
//! The webview's footprint is a high-water latch — WebKit hands freed arenas
//! back to the OS only under pressure, and the wasm linear memory never
//! shrinks at all (`Memory.grow` is monotonic; freeing Rust objects only
//! punches holes inside the arena). The first half is platform bookkeeping;
//! the second is exactly what the app owes it itself to keep low, and it is
//! invisible from the outside: Activity Monitor folds the heap into the
//! webview's total, where canvas surfaces and JSC dominate. So the app
//! charts the pools itself, one console line at each point that moves one —
//! boot, open, close, zoom commit, search-index build, and the reload that
//! resets it.
//!
//! The line carries the three pools a close has to explain: the wasm linear
//! memory, the webview's JavaScript heap (where the pdf.js document, its
//! render tasks, the thumbnail LRU and the canvas pool actually live), and
//! the document's node count — a listener or closure that survives a close
//! keeps its subtree attached, so the count is where a leaked holder shows
//! before anything else does. The JS figure rides `performance.memory`,
//! which only Chromium-family webviews expose: WebView2 answers, WKWebView
//! and WebKitGTK read `undefined`, and their column is a dash rather than a
//! stall — the OS's RSS stands in for it by hand, per the protocol in
//! `docs/memory-baseline.md`.
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

/// The webview's used JavaScript heap in bytes; `None` off wasm, and `None`
/// where the webview does not expose the reading at all —
/// `performance.memory` is Chromium-family only (WebView2 answers,
/// WKWebView and WebKitGTK read `undefined`), so on those platforms the
/// column reads as a dash and the OS RSS stands in for it by hand.
pub(crate) fn js_heap_bytes() -> Option<u64> {
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsValue;
        let performance = web_sys::window()?.performance()?;
        // `performance.memory` is still flagged in Chromium, so it is read by
        // reflection off the typed `Performance` rather than pulled in as a
        // web-sys surface of its own — the same answer, and the same trick
        // `wasm_heap_bytes` runs, with no half-typed API to track.
        let memory = js_sys::Reflect::get(&performance, &JsValue::from_str("memory")).ok()?;
        let used = js_sys::Reflect::get(&memory, &JsValue::from_str("usedJSHeapSize")).ok()?;
        used.as_f64().map(|b| b as u64)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        None
    }
}

/// The document's node count; `None` off wasm (host tests).
pub(crate) fn dom_node_count() -> Option<u32> {
    #[cfg(target_arch = "wasm32")]
    {
        let document = web_sys::window()?.document()?;
        Some(document.get_elements_by_tag_name("*").length())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        None
    }
}

/// One pool's segment of the line: megabytes at one decimal, or a dash where
/// the platform cannot answer.
fn pool_mb(bytes: Option<u64>) -> String {
    bytes.map_or("—".to_string(), |b| {
        format!("{:.1}", b as f64 / (1024.0 * 1024.0))
    })
}

/// Log all three pools under a tag:
/// `[mem] close: wasm 64.0 MB | js 210.3 MB | dom 1842 nodes`. Called at
/// the points that move a pool — or that must visibly NOT move one, which is
/// what makes the ratchet chartable. A pool without a reading on this
/// platform is a dash in its column, never a stall of the line. Off wasm the
/// probe is inert (see the module header): the line is suppressed rather
/// than handed to a stub that would abort.
pub(crate) fn log_heap(tag: &str) {
    if wasm_heap_bytes().is_none() {
        return;
    }
    let line = format!(
        "[mem] {tag}: wasm {} MB | js {} MB | dom {} nodes",
        pool_mb(wasm_heap_bytes()),
        pool_mb(js_heap_bytes()),
        dom_node_count().map_or("—".to_string(), |n| n.to_string()),
    );
    web_sys::console::log_1(&line.into());
}
