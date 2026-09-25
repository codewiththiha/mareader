//! The library artifact's entry: standalone boot (library.html). Hosted
//! boots come through the wasm exports in the lib, which the Shell's runtime
//! manager calls.

fn main() {
    // A wasm-bindgen bin runs `main` when its instance initializes, so a
    // hosted import reaches here too. Boot standalone only when nothing
    // hosts this artifact: hosted, the Shell calls `mareaderLibraryStart`
    // with its own mount target (§12).
    if !library_runtime::shell_hosted() {
        library_runtime::run_standalone();
    }
}
