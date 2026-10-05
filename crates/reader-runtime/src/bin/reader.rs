//! The reader artifact's entry: standalone boot (reader.html). A hosted boot
//! never reaches `run_standalone`: the URL's frame marker routes it into the
//! frame handshake instead (`crate::frame`, §6).

fn main() {
    // A wasm-bindgen bin runs `main` when its instance initializes. Standalone
    // means NO frame marker in the URL (§6 — the marker is the whole hosted
    // boot descriptor, never a window-object sniff): hosted, the frame boot
    // answers the Shell's channel offer and `run_standalone` never runs.
    if !reader_runtime::frame::boot_if_hosted() {
        reader_runtime::run_standalone();
    }
}
