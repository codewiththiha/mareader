//! The memory probe: the three numbers the app can read about itself, one
//! console line per pool.
//!
//! The webview's footprint is a high-water latch — WebKit hands freed arenas
//! back to the OS only under pressure, and the wasm linear memory never
//! shrinks at all (`Memory.grow` is monotonic; freeing Rust objects only
//! punches holes inside the arena). The first half is platform bookkeeping;
//! the second is exactly what the app owes it itself to keep low, and it is
//! invisible from the outside: Activity Monitor folds the heap into the
//! webview's total, where canvas surfaces and JSC dominate. So the app
//! charts the heap itself, one console line at each point that moves it —
//! boot, open, close, zoom commit, search-index build, and the reload that
//! resets it.
//!
//! Three columns, because no single one answers both questions. The wasm
//! column is the ratchet `memory.grow` makes one-way; the js column is the
//! pdf.js half — document, render tasks, thumbnail cache, canvas pool — which
//! a correct teardown should hand back; the dom column is the cheapest leak
//! detector, since a close that leaves the tree at reader size has left nodes
//! pinned by a closure. The js column reads `—` where the platform cannot
//! say: `performance.memory` is Chromium-only (WebView2 on Windows, and the
//! Chrome/Edge dev consoles).
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

/// The JS heap's used size in bytes; `None` off wasm, or where the platform
/// does not expose it.
///
/// `performance.memory.usedJSHeapSize` is the one number the app can read
/// about the JavaScript half of the webview: the pdf.js document, its page
/// render tasks and thumbnail cache, the canvas pool, and the DOM's backing
/// store. It is Chromium-only — WebKit and Firefox hand the name back
/// undefined, and the probe reads that as `None` rather than a number, so the
/// js column prints `—` where the platform cannot say. Read by reflection,
/// like `wasm_heap_bytes`, so the `Performance` feature flag is the only
/// typed surface this costs.
pub(crate) fn js_heap_bytes() -> Option<u64> {
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsValue;
        // `performance.memory.usedJSHeapSize`, read by reflection like
        // `wasm_heap_bytes` — the `Performance` feature flag is the only typed
        // surface this costs, and off Chromium the `memory` key simply reads
        // back `undefined`, which `as_f64` drops to `None`.
        let perf = web_sys::window()?.performance()?;
        // The typed reference first, then the reflective read — the same two
        // steps every reflective probe here takes.
        let perf_ref: &JsValue = perf.as_ref();
        let mem = js_sys::Reflect::get(perf_ref, &JsValue::from_str("memory")).ok()?;
        let used = js_sys::Reflect::get(&mem, &JsValue::from_str("usedJSHeapSize")).ok()?;
        used.as_f64().map(|b| b as u64)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        None
    }
}

/// The live DOM node count; `None` off wasm.
///
/// The cheapest way to see whether a document left anything behind in the
/// DOM. The engine's teardown should return the tree to the library's size; a
/// count that stays at reader level after a close means some listener or
/// closure is pinning nodes — in this codebase the parked Tauri listener
/// closures are the prime suspect. `getElementsByTagName("*")` reads the
/// whole tree in one call, and the result is a count, not a list, so the
/// probe itself costs nothing it would then have to explain.
pub(crate) fn dom_node_count() -> Option<u32> {
    #[cfg(target_arch = "wasm32")]
    {
        let doc = web_sys::window()?.document()?;
        Some(doc.get_elements_by_tag_name("*").length())
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        None
    }
}

/// Log every pool the app can see, in one line:
/// `[mem] open: wasm 64.0 MB | js 58.5 MB | dom 12,341 nodes`. Called at
/// the points that move the heap — or that must visibly NOT move it, which
/// is what makes the ratchet chartable.
///
/// The columns are read together, not one at a time: the wasm column is the
/// one-instance ratchet; the js column says whether the pdf.js teardown
/// reclaimed its half of the session; the dom column says whether anything is
/// still pinned after the reader unmounts. Off wasm the probe stays inert
/// rather than log a line of three `—` among host tests.
pub(crate) fn log_heap(tag: &str) {
    let Some(wasm) = wasm_heap_bytes() else {
        return;
    };
    let wasm_text = format_mb(wasm);
    let js_text = match js_heap_bytes() {
        Some(bytes) => format_mb(bytes),
        None => "—".to_string(),
    };
    let dom_text = match dom_node_count() {
        Some(nodes) => nodes.to_string(),
        None => "—".to_string(),
    };
    web_sys::console::log_1(
        &format!("[mem] {tag}: wasm {wasm_text} MB | js {js_text} MB | dom {dom_text} nodes")
            .into(),
    );
}

/// `bytes` in mebibytes, one decimal place — the unit every `[mem]` column
/// is read in.
fn format_mb(bytes: u64) -> String {
    format!("{:.1}", bytes as f64 / (1024.0 * 1024.0))
}
