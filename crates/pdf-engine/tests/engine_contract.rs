//! Wire-contract guard for the `window.PDFReader` facade.
//!
//! `bridge.rs` is the only place the JS engine surface is declared, and a
//! rename on either side fails at RUNTIME (the wasm shim resolves
//! `undefined`) with no build error. The browser-side smoke test covers the
//! runtime behaviour; this test keeps the two surfaces textually in sync so
//! the contract is checked on every `cargo test` too.
//!
//! The facade is the esbuild output `public/pdfEngine.js`, which is only
//! produced by the `build:ts` step — when it is missing (a bare `cargo test`
//! on a fresh clone) the check reports and skips rather than failing; CI
//! runs it with the artifact present, right before this job's test step.

use std::path::PathBuf;

fn repo_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // crates/
    p.pop(); // repo root
    p
}

/// Extern names declared under `js_namespace = ["window", "PDFReader"]`.
fn bridge_pdfreader_names() -> Vec<String> {
    let bridge = std::fs::read_to_string(repo_root().join("crates/pdf-engine/src/bridge.rs"))
        .expect("bridge.rs must exist next to the test");
    let mut names = Vec::new();
    // The last `#[wasm_bindgen(...)]` attribute seen: its namespace decides
    // whether the NEXT fn declaration belongs to the PDFReader surface, and
    // its `js_name` (when present) is the JS-side spelling.
    let mut attr: Option<(bool, Option<String>)> = None; // (pdfreader ns, js_name)
    for line in bridge.lines() {
        if line.contains("#[wasm_bindgen(") {
            let pdfreader = line.contains("js_namespace = [\"window\", \"PDFReader\"]");
            // Match `js_name = "X"` ONLY: `js_namespace = ["window", ...]`
            // also contains the substring "js_name", so splitting on the
            // bare token would eat the namespace arm. The em-space spelling
            // (`js_name = "`) is unique to the attribute we want.
            let js_name = line
                .split("js_name = \"")
                .nth(1)
                .and_then(|rest| rest.split('"').next())
                .map(str::to_string);
            attr = Some((pdfreader, js_name));
            continue;
        }
        if let Some(start) = line.find("pub ") {
            let rest = &line[start + 4..];
            if let Some(fn_pos) = rest.find("fn ") {
                let name: String = rest[fn_pos + 3..]
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if let Some((true, js_name)) = attr.take() {
                    names.push(js_name.unwrap_or(name));
                }
                continue;
            }
        }
    }
    names
}

/// True when `name` appears in `facade` as a property key: a word boundary
/// before it and `,` or `:` after (the esbuild IIFE spells the facade
/// `globalThis.PDFReader = { version: ..., open, ... }`).
fn facade_has_key(facade: &str, name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    let hay: Vec<char> = facade.chars().collect();
    let needle: Vec<char> = name.chars().collect();
    for (i, window) in hay.windows(needle.len()).enumerate() {
        if window != needle.as_slice() {
            continue;
        }
        let before_ok = i == 0 || !(hay[i - 1].is_alphanumeric() || hay[i - 1] == '_');
        if !before_ok {
            continue;
        }
        let mut j = i + needle.len();
        while j < hay.len() && hay[j].is_whitespace() {
            j += 1;
        }
        if j < hay.len() && (hay[j] == ',' || hay[j] == ':' || hay[j] == '}') {
            return true;
        }
    }
    false
}

#[test]
fn engine_facade_exposes_every_pdfreader_binding() {
    let facade = std::fs::read_to_string(repo_root().join("public/pdfEngine.js"));
    let Ok(facade) = facade else {
        eprintln!(
            "public/pdfEngine.js not built — facade contract check skipped \
             (run `npm run build:ts`)"
        );
        return;
    };
    let names = bridge_pdfreader_names();
    assert!(!names.is_empty(), "no PDFReader externs parsed from bridge.rs");
    let missing: Vec<&str> = names
        .iter()
        .map(String::as_str)
        .filter(|name| !facade_has_key(&facade, name))
        .collect();
    assert!(
        missing.is_empty(),
        "public/pdfEngine.js is missing bridge bindings: {}",
        missing.join(", ")
    );
}
