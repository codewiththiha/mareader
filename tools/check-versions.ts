// Version-sync check — the cheapest insurance in the release pipeline.
//
// The app version lives in FOUR places in this repo (the workspace root plus
// the Tauri shell crate):
//   - package.json            (.version)
//   - src-tauri/tauri.conf.json (.version)   <- what Tauri bundles
//   - Cargo.toml              (pdf-reader, [package].version)
//   - src-tauri/Cargo.toml    (pdf,        [package].version)
//
// Release tags and artifact filenames are derived from it, so if any of the
// four drift, releases break invisibly. This script fails CI when they
// disagree. (Tag-vs-version agreement is validated by the release workflow's
// metadata job, which is the only place a tag is in hand.)
//
// This is the TypeScript source; Trunk's pre-build hook compiles it to
// `scripts/check-versions.js` so CI can run it with plain `node`.

import { isFile, read } from "./repo.js";

type VersionSource = [file: string, version: string];

function readJson(rel: string): unknown {
  return JSON.parse(read(rel));
}

function readCargoVersion(rel: string): string {
  const text = read(rel);
  const m = /^version\s*=\s*"([^"]+)"/m.exec(text);
  if (!m || !m[1]) throw new Error(`no [package] version found in ${rel}`);
  return m[1];
}

const sources: VersionSource[] = [
  ["package.json", (readJson("package.json") as { version: string }).version],
  [
    "src-tauri/tauri.conf.json",
    (readJson("src-tauri/tauri.conf.json") as { version: string }).version,
  ],
  ["Cargo.toml", readCargoVersion("Cargo.toml")],
  ["src-tauri/Cargo.toml", readCargoVersion("src-tauri/Cargo.toml")],
];

const versions = new Set(sources.map(([, v]) => v));
if (versions.size !== 1) {
  console.error("::error::Version mismatch across files:");
  for (const [file, v] of sources) console.error(`  ${file}: ${v}`);
  process.exit(1);
}

const version = sources[0]![1];
console.log(`versions agree: ${version}`);

// ---------------------------------------------------------------------------
// Engine facade vs. Rust bridge contract
// ---------------------------------------------------------------------------
// crates/pdf-engine/src/bridge.rs is the sole place `window.PDFReader.*` is
// declared. A rename on either side fails at RUNTIME with no build error
// (the wasm shim just gets `undefined`), so this step cross-checks the
// compiled facade (public/pdfEngine.js, produced by the build:ts step above)
// against every extern the bridge declares under the PDFReader namespace.
const BRIDGE = "crates/pdf-engine/src/bridge.rs";
const FACADE = "public/pdfEngine.js";

function bridgePdfReaderNames(bridgeSrc: string): string[] {
  // Only `extern "C"` declarations: `listen`/`has_pdf_reader` are plain pub
  // fns and must not be counted as window.PDFReader surface. The attribute
  // comes on its own line before the fn, so carry it until the fn it
  // decorates, then consume it (an unused attribute would misattribute the
  // NEXT fn).
  const names: string[] = [];
  const lines = bridgeSrc.split(/\r?\n/);
  let inExtern = false;
  let attr: { pdfreader: boolean; jsName: string | null } | null = null;
  for (const line of lines) {
    if (line.includes('extern "C"')) {
      inExtern = true;
      continue;
    }
    if (inExtern && /^\s*}\s*$/.test(line)) {
      inExtern = false;
      attr = null;
      continue;
    }
    if (!inExtern) continue;
    const am = /#\[wasm_bindgen\(([^)]*)\)\]/.exec(line);
    if (am) {
      const a = am[1]!;
      attr = {
        pdfreader: a.includes('js_namespace = ["window", "PDFReader"]'),
        jsName: (/js_name\s*=\s*"(\w+)"/.exec(a) ?? [])[1] ?? null,
      };
      continue;
    }
    const fm = /pub\s+(?:async\s+)?fn\s+(\w+)/.exec(line);
    if (fm && attr) {
      if (attr.pdfreader) names.push(attr.jsName ?? fm[1]!);
      attr = null;
      continue;
    }
  }
  return names;
}

if (isFile(FACADE)) {
  const bridgeSrc = read(BRIDGE);
  const facadeSrc = read(FACADE);
  const missing: string[] = [];
  for (const name of bridgePdfReaderNames(bridgeSrc)) {
    // The facade is a shorthand object literal; the LAST property can carry
    // no trailing comma, so accept `,`, `:` or the closing `}`.
    const re = new RegExp(`\\b${name}\\s*[,:}]`);
    if (!re.test(facadeSrc)) missing.push(name);
  }
  if (missing.length > 0) {
    console.error(`::error::PDFReader facade is missing bridge bindings: ${missing.join(", ")}`);
    process.exit(1);
  }
  console.log(`engine facade matches the Rust bridge (${bridgePdfReaderNames(bridgeSrc).length} bindings)`);
} else {
  console.log("public/pdfEngine.js not built yet — facade check skipped");
}
