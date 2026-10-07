//! The reader artifact's entry: standalone boot (reader.html).

fn main() {
    // A wasm-bindgen bin runs `main` on init; standalone means no
    // frame marker.
    if !reader_runtime::frame::boot_if_hosted() {
        reader_runtime::run_standalone();
    }
}
