// The canonical frontend build's artifact contract.
//
// `tools/build-dist.sh` produces THREE artifacts — the shell page plus the two
// runtime artifacts the shell dynamically imports — and merges them into
// `dist/`. Tauri packages exactly that directory
// (`build.frontendDist: "../dist"`), and so does the browser suite's server.
// The failure this check exists for is the one that shipped a blank window:
// Tauri ran `trunk build --release`, which produces only the shell page, so
// the packaged app served an `index.html` whose `import("/library.js")`
// resolved to nothing and left the runtime host empty — with no build error
// anywhere, because a missing file is not a build failure in a static bundle.
//
// So: after the build, the artifact SET is asserted, not assumed. Every entry
// must exist and be non-empty; the runtime artifacts carry a floor as well, so
// a zero-byte stub or an HTML error page saved as `.wasm` cannot pass as a
// build. Failures print the exact path the guide's incident reports — example:
//
//     Missing runtime artifact: dist/library.js
//
// Run it after the build (build-dist.sh calls it) and in CI (the deep lane's
// build step inherits it, and the Web/contracts lane runs it against nothing
// only by omission — it has no dist to check).
//
// JavaScript, not TypeScript: this one has to run inside the build itself
// (build-dist.sh) and in lanes that have not installed node_modules yet, so it
// uses node's own modules and nothing else. `tools/*.mjs` is the repo's
// existing pattern for exactly that (bundle-engine.mjs, wasm-smoke.mjs).

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

/** A runtime artifact must be a real bundle, not a placeholder. Both runtime
 *  wasm modules are megabytes in release; this floor only rejects stubs. */
const RUNTIME_FLOOR_BYTES = 1024;

/** The artifact contract, in the order the incident report names it.
 *  `floor` is 0 for anything whose only requirement is "exists and is not
 *  empty"; it is RUNTIME_FLOOR_BYTES for the four runtime bundles the shell
 *  loads and the shared assets the runtimes fetch at boot. */
const REQUIRED = [
  ["dist/index.html", "the Shell page — Tauri's frontendDist entry", 0],
  ["dist/library.js", "the Library Runtime artifact (imported by the shell)", RUNTIME_FLOOR_BYTES],
  ["dist/library_bg.wasm", "the Library Runtime wasm module", RUNTIME_FLOOR_BYTES],
  ["dist/reader.js", "the Reader Runtime artifact (imported by the shell)", RUNTIME_FLOOR_BYTES],
  ["dist/reader_bg.wasm", "the Reader Runtime wasm module", RUNTIME_FLOOR_BYTES],
  // Shared assets. The shell page links these and both runtimes fetch the
  // engine bundles at session start; a missing one is a runtime failure with
  // the same shape as a missing runtime artifact.
  ["dist/styles.css", "the compiled stylesheet", RUNTIME_FLOOR_BYTES],
  ["dist/pdfEngine.js", "the imperative pdf.js wrapper (window.PDFReader)", RUNTIME_FLOOR_BYTES],
  ["dist/readerEngine.js", "the format-agnostic reader bundle", RUNTIME_FLOOR_BYTES],
  ["dist/bake.worker.js", "the theme bake worker", RUNTIME_FLOOR_BYTES],
  ["dist/shellBoot.js", "the shell page's boot watchdog", RUNTIME_FLOOR_BYTES],
  ["dist/vendor/pdfjs/pdf.min.mjs", "pdf.js itself", RUNTIME_FLOOR_BYTES],
  ["dist/vendor/pdfjs/pdf.worker.min.mjs", "the pdf.js worker", RUNTIME_FLOOR_BYTES],
  ["dist/vendor/pdfjs/pdf_viewer.css", "the pdf.js text-layer stylesheet", 0],
];

/** The shell page's own boot contract: the placeholder the user sees before
 *  any runtime loads (§5) and the id the shell removes on mount. Checked here
 *  because it is the same class of silent regression — the page still builds
 *  and still boots, and the user gets a blank window while the runtime loads. */
const SHELL_BOOT_ID = 'id="shell-boot"';
const SHELL_BOOT_COPY = "Loading MAReader";

const problems = [];

for (const [rel, what, floor] of REQUIRED) {
  const abs = path.join(root, rel);
  let stat;
  try {
    stat = fs.statSync(abs);
  } catch {
    problems.push(`Missing runtime artifact: ${rel}\n  (${what})`);
    continue;
  }
  if (!stat.isFile()) {
    problems.push(`Runtime artifact is not a file: ${rel}\n  (${what})`);
    continue;
  }
  if (stat.size === 0) {
    problems.push(`Empty runtime artifact: ${rel}\n  (${what} — 0 bytes)`);
    continue;
  }
  if (floor > 0 && stat.size < floor) {
    problems.push(
      `Runtime artifact is a stub: ${rel}\n` +
        `  (${what} — ${stat.size} bytes, expected at least ${floor})`,
    );
  }
}

// The shell page carries the immediate loading state, and it is the only file
// in the contract that is hand-written rather than generated — so it is also
// the one that can regress by edit.
const indexHtml = path.join(root, "dist/index.html");
if (fs.existsSync(indexHtml)) {
  const html = fs.readFileSync(indexHtml, "utf8");
  if (!html.includes(SHELL_BOOT_ID)) {
    problems.push(
      `dist/index.html has no ${SHELL_BOOT_ID} placeholder — the shell would show a ` +
        `blank window until the first runtime mounts`,
    );
  }
  if (!html.includes(SHELL_BOOT_COPY)) {
    problems.push(
      `dist/index.html's boot placeholder does not say "${SHELL_BOOT_COPY}" — ` +
        `the loading state is the shell's own copy (see index.html)`,
    );
  }
}

if (problems.length > 0) {
  console.error("runtime artifact contract FAILED:");
  for (const problem of problems) console.error(`  - ${problem}`);
  console.error(
    "\nBuild with `npm run build:dist` (tools/build-dist.sh) — the ONE canonical " +
      "frontend build. `trunk build` alone produces the shell page only.",
  );
  process.exit(1);
}

const total = REQUIRED.reduce(
  (sum, [rel]) => sum + fs.statSync(path.join(root, rel)).size,
  0,
);
console.log(
  `runtime artifact contract OK: ${REQUIRED.length} files, ${total} bytes ` +
    `(shell + library + reader + shared assets)`,
);
