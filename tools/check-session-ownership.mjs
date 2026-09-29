// Session ownership, asserted from source (docs/session-ownership.md).
//
// Every document a pane shows is owned by that pane's format session — a
// `PdfSession` (crates/pdf-engine) or an `MdSession`/`TxtSession`
// (crates/reader-runtime/src/pane/session.rs) — installed per opened
// document and disposed with it. The compiler proves the types; it cannot
// prove that nothing still routes around them. This check does, rule by
// rule, so a regression back to a realm-wide document owner fails CI:
//
//   1. Reader code reaches the PDF engine only through its pane's session
//      (`pane.pdf()`, `MountedPdf`). The `pdf_engine::api` names it may
//      still use are the realm-wide ones — appearance broadcasts, engine
//      diagnostics, the error/stats types — never document work.
//   2. The paper state machine is per session: reader code names no
//      `pdf_engine::backdrop` function except the realm's in-flight sample
//      gauge (diagnostics).
//   3. Sessions are created and installed by the open flow only: a
//      `PdfSession::create` or `install_session` anywhere else would be a
//      second owner for a pane's document.
//   4. Async ownership stamps are per pane (`claim_generation` /
//      `owns_generation`); the realm-wide epoch is a diagnostics label and
//      appears nowhere else.
//   5. The legacy realm-wide ownership names stay gone.
//   6. The engine bridge is session-first: every `window.PDFReader` extern
//      takes `sid: u32` first, except the realm-wide surface listed below.
//   7. The PDF engine crate holds no realm state beyond the classified
//      realm-shared statics below (everything else lives on a session).
//
// Test code is exempt — tests build sessions directly: an inline
// `#[cfg(test)] mod tests { … }` (at the end of a file by this repo's
// convention) and a whole file declared `#[cfg(test)] mod name;`.
//
// Plain node modules only, so it runs in a lane that has not run `npm ci`.

import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, relative } from "node:path";

const ROOT = new URL("..", import.meta.url).pathname;
const RUNTIME = join(ROOT, "crates/reader-runtime/src");
const ENGINE = join(ROOT, "crates/pdf-engine/src");
const BRIDGE = join(ENGINE, "bridge.rs");

/** Rule 1: the realm-wide `pdf_engine::api` names reader code may use. */
const API_REALM = new Set([
  "EngineError", // the error type session calls return
  "EngineStats", // the diagnostics snapshot type
  "engine_stats", // realm aggregate (diagnostics)
  "set_lifecycle_log", // dev-surface narration switch
  "refresh_theme", // appearance broadcast to every live session
  "set_scrub_mode", // appearance broadcast
  "set_appearance_menu_open", // appearance broadcast
]);

/** Rule 2: the realm-wide `pdf_engine::backdrop` names. */
const BACKDROP_REALM = new Set(["pending_samples"]);

/** Rule 3: where sessions may be created / installed (production code). */
const CREATE_SITES = new Set(["services/document/open/mod.rs"]);
const INSTALL_SITES = new Set([
  "services/document/open/mod.rs",
  "services/document/open/reflow.rs",
  "pane/handle.rs", // the definition
]);

/** Rule 4: the diagnostics epoch's only readers. */
const EPOCH_SITES = new Set(["diagnostics.rs", "services/document/session.rs"]);

/** Rule 5: legacy realm-wide ownership, by name. */
const LEGACY = [
  [/\bnote_document_session\b/, "the pane's session IS the record (holds_document_session derives from it)"],
  [/\bsession::claim\b/, "document stamps are per pane: PaneHandle::claim_generation"],
  [/\bsession::owns\b/, "document stamps are per pane: PaneHandle::owns_generation"],
  [/\bscope_to_document\b/, "search scope is the session's (PdfSession::open scopes it)"],
  [/\bdocument_close\b/, "the paper state machine closes with its session's dispose"],
  [/\b__pdfDestroy\b/, "the engine destroys sessions, not a realm document (destroySession)"],
];

/** Rule 6: the bridge externs that are realm-wide by design. */
const BRIDGE_REALM = new Set([
  "version",
  "refresh_theme",
  "set_scrub_mode",
  "set_appearance_menu_open",
  "stats",
  "set_lifecycle_log",
]);

/** Rule 7: the engine crate's realm-shared statics, each safe by design. */
const ENGINE_STATICS = new Set([
  "NEXT_SID", // the sid mint: monotonic, never reused
  "SAMPLES_IN_FLIGHT", // gauge of look-ahead samples across sessions (diagnostics)
  "RETAINED", // one retained search index, adopted only by identical content
  "BUILD_ACTIVE", // gauge of index builds in flight (diagnostics)
]);

function walk(dir) {
  const out = [];
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) out.push(...walk(path));
    else if (name.endsWith(".rs")) out.push(path);
  }
  return out;
}

/** Production code lines: line comments blanked, and everything from a
 *  `#[cfg(test)]`-gated `mod tests` on dropped. Line numbers preserved. */
function productionLines(path) {
  const lines = readFileSync(path, "utf8").split("\n");
  const out = [];
  let gated = false;
  for (const raw of lines) {
    const at = raw.indexOf("//");
    const line = at === -1 ? raw : raw.slice(0, at);
    if (/^\s*#\[cfg\(test\)\]\s*$/.test(line)) {
      gated = true;
      out.push("");
      continue;
    }
    if (gated && /^\s*(pub(\(crate\))?\s+)?mod\s+tests\b/.test(line)) break;
    if (line.trim() !== "") gated = false;
    out.push(line);
  }
  return out;
}

/** Files that are test modules in their own right: declared
 *  `#[cfg(test)] mod name;` by their parent. */
function testFiles(files) {
  const out = new Set();
  for (const file of files) {
    const lines = readFileSync(file, "utf8").split("\n");
    lines.forEach((line, i) => {
      if (!/^\s*#\[cfg\(test\)\]\s*$/.test(line)) return;
      const m = (lines[i + 1] ?? "").match(/^\s*(?:pub(?:\([a-z]+\))?\s+)?mod\s+(\w+)\s*;/);
      if (!m) return;
      const dir = file.endsWith("/mod.rs") || file.endsWith("/lib.rs")
        ? file.slice(0, file.lastIndexOf("/"))
        : file.slice(0, -3);
      out.add(join(dir, `${m[1]}.rs`));
      out.add(join(dir, m[1], "mod.rs"));
    });
  }
  return out;
}

const failures = [];
const fail = (file, i, why, line) =>
  failures.push(`${relative(ROOT, file)}:${i + 1}: ${why}\n    ${line.trim()}`);

// --- Rules 1–5: the reader runtime ------------------------------------------
const runtimeAll = walk(RUNTIME);
const runtimeTests = testFiles(runtimeAll);
const runtimeFiles = runtimeAll.filter((file) => !runtimeTests.has(file));
for (const file of runtimeFiles) {
  const rel = relative(RUNTIME, file);
  productionLines(file).forEach((line, i) => {
    for (const m of line.matchAll(/\bpdf_engine::api::(\w+)/g)) {
      if (!API_REALM.has(m[1])) {
        fail(file, i, `pdf_engine::api::${m[1]} is document work — go through the pane's session (pane.pdf())`, line);
      }
    }
    if (/\bpdf_engine::api\s+as\b|\bpdf_engine::api::\{|\bpdf_engine::api::\*/.test(line)) {
      fail(file, i, "import pdf_engine::api names explicitly (only the realm-wide ones are allowed)", line);
    }
    for (const m of line.matchAll(/\bpdf_engine::backdrop::(\w+)/g)) {
      if (!BACKDROP_REALM.has(m[1])) {
        fail(file, i, `pdf_engine::backdrop::${m[1]} — the paper state machine is per session (PdfPane::paper_*)`, line);
      }
    }
    if (/\bPdfSession::create\b/.test(line) && !CREATE_SITES.has(rel)) {
      fail(file, i, "a PdfSession is created by the open flow only (services/document/open/mod.rs)", line);
    }
    if (/\binstall_session\s*\(/.test(line) && !INSTALL_SITES.has(rel)) {
      fail(file, i, "a pane's session is installed by the open flow only", line);
    }
    if (/\bcurrent_epoch\b/.test(line) && !EPOCH_SITES.has(rel)) {
      fail(file, i, "async ownership is per pane (claim_generation/owns_generation), not the realm epoch", line);
    }
    for (const [pattern, why] of LEGACY) {
      if (pattern.test(line)) fail(file, i, why, line);
    }
  });
}

// --- Rule 5 (engine side) and rule 7: the PDF engine crate ------------------
const engineAll = walk(ENGINE);
const engineTests = testFiles(engineAll);
const engineFiles = engineAll.filter((file) => !engineTests.has(file));
for (const file of engineFiles) {
  productionLines(file).forEach((line, i) => {
    for (const [pattern, why] of LEGACY) {
      if (pattern.test(line)) fail(file, i, why, line);
    }
    const s = line.match(/^\s*(?:pub(?:\([a-z]+\))?\s+)?static\s+(\w+)\s*:/);
    if (s && !s[1].startsWith("$") && !ENGINE_STATICS.has(s[1])) {
      fail(file, i, `static ${s[1]} is realm state the session model has not classified — put it on PdfSession, or classify it here with the reason it is safe`, line);
    }
  });
}

// --- Rule 6: the bridge is session-first ------------------------------------
{
  let pdfreader = false;
  let count = 0;
  readFileSync(BRIDGE, "utf8")
    .split("\n")
    .forEach((line, i) => {
      if (line.includes("#[wasm_bindgen(")) {
        pdfreader = line.includes('js_namespace = ["window", "PDFReader"]');
        return;
      }
      const m = line.match(/\bpub\s+(?:async\s+)?fn\s+(\w+)\s*\(([^,)]*)/);
      if (!m) return;
      if (pdfreader) {
        count += 1;
        if (!BRIDGE_REALM.has(m[1]) && !/^\s*sid\s*:\s*u32\s*$/.test(m[2])) {
          fail(BRIDGE, i, `PDFReader.${m[1]} is document work: its first parameter must be the session (sid: u32)`, line);
        }
      }
      pdfreader = false;
    });
  if (count === 0) failures.push(`${relative(ROOT, BRIDGE)}: no PDFReader externs parsed`);
}

if (failures.length > 0) {
  console.error(`session ownership: ${failures.length} violation(s)\n`);
  for (const failure of failures) console.error(`  ${failure}`);
  process.exit(1);
}
console.log(
  `session ownership: ${runtimeFiles.length} reader sources reach the engine through their pane's session; ` +
    `${engineFiles.length} engine sources hold only classified realm state; the bridge is session-first`,
);
