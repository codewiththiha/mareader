fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    // One alias for "this compilation is a format artifact". The host bin
    // enables none of the features, so the view is not in that artifact.
    // A workspace build unifies the features; the per-crate wasm check does not.
    // Always declared, even on the host build that sets nothing. An
    // undeclared cfg is a warning, and warnings are denied.
    println!("cargo:rustc-check-cfg=cfg(format_runtime)");
    let pdf = std::env::var_os("CARGO_FEATURE_FORMAT_PDF").is_some();
    let text = std::env::var_os("CARGO_FEATURE_FORMAT_TEXT").is_some();
    let md = std::env::var_os("CARGO_FEATURE_FORMAT_MD").is_some();
    if pdf || text || md {
        println!("cargo:rustc-cfg=format_runtime");
    }
}
