//! The library artifact's entry: standalone boot (library.html). A hosted
//! boot never reaches `run_standalone`: the URL's frame marker routes it into
//! the frame handshake instead (`crate::frame`, §6).
//!
//! Neither deployment loads a PDF engine. The hosted frame asks the Shell
//! for its covers; the standalone page has no Shell and simply shows the
//! covers it already holds (the reader's open pipeline files one on every
//! first open). Putting an engine script on this page for the standalone
//! case would put it in the hosted frame too — the page is shared — and the
//! shelf's whole dependency graph is meant to carry zero PDF execution code.

fn main() {
    // A wasm-bindgen bin runs `main` when its instance initializes. Standalone
    // means NO frame marker in the URL (§6 — the marker is the whole hosted
    // boot descriptor, never a window-object sniff): hosted, the frame boot
    // answers the Shell's channel offer and `run_standalone` never runs.
    if !library_runtime::frame::boot_if_hosted() {
        library_runtime::run_standalone();
    }
}
