//! Wire-contract guard for the `window.PDFReader` facade.
//!
//! `bridge.rs` is the only place the JS engine surface is declared, and a
//! rename on either side fails at RUNTIME (the wasm shim resolves `undefined`)
//! with no build error. The browser-side smoke test covers runtime behaviour;
//! this test keeps the two surfaces textually in sync on every `cargo test`.
//!
//! The facade is the esbuild output `public/pdfEngine.js`, produced only by
//! `build:ts` — when it is missing (a bare `cargo test` on a fresh clone) the
//! check reports and skips; CI runs it with the artifact present.

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

/// The phase names the engine parses, read out of its own table.
///
/// `public/engine/motion.ts` declares them as a const tuple; this reads the
/// literals out of that declaration rather than trusting a copy, the same way
/// the facade check above reads the bridge.
fn engine_phase_names() -> Vec<String> {
    let motion = std::fs::read_to_string(repo_root().join("public/engine/motion.ts"))
        .expect("motion.ts must exist next to the test");
    let table = motion
        .split("export const PHASES = [")
        .nth(1)
        .and_then(|rest| rest.split(']').next())
        .expect("motion.ts must declare a PHASES table");
    table
        .split('"')
        .filter(|part| !part.trim().is_empty() && !part.contains(','))
        .map(str::to_string)
        .collect()
}

/// The phase names the Rust side publishes, read out of `MotionPhase::wire`.
fn bridge_phase_names() -> Vec<String> {
    let motion = std::fs::read_to_string(repo_root().join("crates/pdf-engine/src/api/motion.rs"))
        .expect("api/motion.rs must exist next to the test");
    let body = motion
        .split("pub const fn wire(self) -> &'static str {")
        .nth(1)
        .and_then(|rest| rest.split("\n    }").next())
        .expect("MotionPhase must declare a wire spelling");
    let mut names = Vec::new();
    for arm in body.split("=>") {
        if let Some(quote) = arm.split('"').nth(1) {
            names.push(quote.to_string());
        }
    }
    names
}

#[test]
fn the_motion_phases_agree_across_the_bridge() {
    // A phase the engine cannot parse is read as `idle`, so a rename on either
    // side is not an error anywhere: it is a fling that stops pacing renders,
    // with a green build and a clean console. Both tables are read out of the
    // sources rather than restated, so the drift fails here instead.
    let engine = engine_phase_names();
    let bridge = bridge_phase_names();
    assert!(!engine.is_empty(), "no PHASES parsed from motion.ts");
    assert_eq!(bridge.len(), 5, "MotionPhase::wire arms: {bridge:?}");
    assert_eq!(bridge, engine, "the two phase tables disagree");
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
