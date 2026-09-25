//! The reader artifact's entry: standalone boot (reader.html). Hosted boots
//! come through the wasm exports in the lib (`mareaderReaderStart`), which
//! the Shell's runtime manager calls.

fn main() {
    // A wasm-bindgen bin runs `main` when its instance initializes, so a
    // hosted import reaches here too. Boot standalone only when nothing
    // hosts this artifact: hosted, the Shell calls `mareaderReaderStart`
    // with its own mount target, and booting here as well would put a
    // second, invisible reader in the page (§12).
    if !reader_runtime::shell_hosted() && !reader_runtime::frame::boot_if_hosted() {
        reader_runtime::run_standalone();
    }
}
