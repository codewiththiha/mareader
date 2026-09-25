//! The dynamic import the shell loads runtime artifacts through. One inline
//! JS helper: `import()` is not callable from wasm directly, and this keeps
//! the ESM semantics (module map caching = compiled-code caching, per the
//! runtime-split doc).

use wasm_bindgen::prelude::*;

#[wasm_bindgen(inline_js = "export function dyn_import(p) { return import(p); }")]
extern "C" {
    #[wasm_bindgen(js_name = dyn_import)]
    pub fn dyn_import(path: &str) -> js_sys::Promise;
}
