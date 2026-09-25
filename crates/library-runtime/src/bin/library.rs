//! The library artifact's entry: standalone boot (library.html). A hosted
//! boot never reaches `run_standalone`: the URL's frame marker routes it into
//! the frame handshake instead (`crate::frame`, §6).
//!
//! The standalone page deploys no engine scripts (the page is shared with
//! the hosted frame, which must be engine-free), but the standalone bake
//! facade (`services::cover_engine`) reads the engine's global when a cover
//! is requested. So a standalone boot mounts the engine first — its script
//! injected from JS, because the page's CSP forbids inline script blocks and
//! the artifact's own build can name the file; a plain script tag would put
//! the engine back in the hosted frame too.

/// Where the engine bundle sits when it exists. Known by name, not
/// discovered: the build that ships the artifact decides its layout, and a
/// guessed probe would mask a missing deployment as "no covers".
#[cfg(target_arch = "wasm32")]
const ENGINE_SCRIPT: &str = "/pdfEngine.js";

/// Load the engine bundle next to the standalone artifact. Best-effort and
/// fire-and-forget: the bake facade reports an absent global as a plain
/// failure answer (the import queue retries once, then waits), so the shelf
/// never gates on this.
#[cfg(target_arch = "wasm32")]
fn deploy_standalone_engine() {
    use wasm_bindgen::JsCast;

    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let Some(head) = document.head() else {
        return;
    };
    let Ok(el) = document.create_element("script") else {
        return;
    };
    let script: web_sys::HtmlScriptElement = el.unchecked_into();
    script.set_src(ENGINE_SCRIPT);
    script.set_type("module");
    let _ = head.append_child(&script);
}

#[cfg(not(target_arch = "wasm32"))]
fn deploy_standalone_engine() {}

fn main() {
    // A wasm-bindgen bin runs `main` when its instance initializes. Standalone
    // means NO frame marker in the URL (§6 — the marker is the whole hosted
    // boot descriptor, never a window-object sniff): hosted, the frame boot
    // answers the Shell's channel offer and `run_standalone` never runs.
    if !library_runtime::frame::boot_if_hosted() {
        deploy_standalone_engine();
        library_runtime::run_standalone();
    }
}
