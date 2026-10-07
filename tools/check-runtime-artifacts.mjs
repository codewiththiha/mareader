// The build's contract: every runtime artifact exists and is non-empty.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

/** A runtime artifact must be a real bundle, not a zero-byte stub. */
const RUNTIME_FLOOR_BYTES = 1024;

/** The artifact contract in the report's order; `floor` rejects stubs. */
const REQUIRED = [
  ["dist/index.html", "the Shell page — Tauri's frontendDist entry", 0],
  // The persistent Shell contains no route runtime implementation.
  ["dist/mareader.js", "the Shell artifact", RUNTIME_FLOOR_BYTES],
  ["dist/mareader_bg.wasm", "the Shell wasm module", RUNTIME_FLOOR_BYTES],
  // Independently disposable route realms.
  ["dist/library.html", "the Library route page", 0],
  ["dist/library.js", "the Library artifact", RUNTIME_FLOOR_BYTES],
  ["dist/library_bg.wasm", "the Library wasm module", RUNTIME_FLOOR_BYTES],
  ["dist/reader.html", "the disposable Reader host page", 0],
  ["dist/reader.js", "the Reader host artifact", RUNTIME_FLOOR_BYTES],
  ["dist/reader_bg.wasm", "the Reader host wasm module", RUNTIME_FLOOR_BYTES],
  ["dist/readerHost.js", "the scoped shared-raster host bridge", RUNTIME_FLOOR_BYTES],
  // The pane runtimes: `pdf.html` for a PDF, `reflow.html` for text.
  ["dist/pdf.html", "the PDF pane page", 0],
  ["dist/pdf.js", "the PDF pane artifact", RUNTIME_FLOOR_BYTES],
  ["dist/pdf_bg.wasm", "the PDF pane wasm module", RUNTIME_FLOOR_BYTES],
  ["dist/reflow.html", "the text pane page", 0],
  ["dist/reflow.js", "the text pane artifact", RUNTIME_FLOOR_BYTES],
  ["dist/reflow_bg.wasm", "the text pane wasm module", RUNTIME_FLOOR_BYTES],
  // Shared assets both runtimes fetch at start; a missing one is a failure.
  ["dist/styles.css", "the compiled stylesheet", RUNTIME_FLOOR_BYTES],
  ["dist/pdfEngine.js", "the imperative pdf.js wrapper (window.PDFReader)", RUNTIME_FLOOR_BYTES],
  ["dist/readerEngine.js", "the format-agnostic reader bundle", RUNTIME_FLOOR_BYTES],
  ["dist/rasterLane.js", "the host's weak full-page raster coordinator", RUNTIME_FLOOR_BYTES],
  ["dist/bake.worker.js", "the theme bake worker", RUNTIME_FLOOR_BYTES],
  // The Shell's cover-bake page and script; without them covers never arrive.
  ["dist/bake.html", "the Shell's cover-bake page", 0],
  ["dist/coverBake.js", "the cover-bake page's script", RUNTIME_FLOOR_BYTES],
  ["dist/shellBoot.js", "the shell page's boot watchdog", RUNTIME_FLOOR_BYTES],
  ["dist/bootPaint.js", "the shell page's remembered-paper first paint", RUNTIME_FLOOR_BYTES],
  // The iframes' Tauri facade: without it a local-file open degrades to a 404.
  ["dist/tauri-relay.js", "the frames' Tauri IPC facade", RUNTIME_FLOOR_BYTES],
  ["dist/vendor/pdfjs/pdf.min.mjs", "pdf.js itself", RUNTIME_FLOOR_BYTES],
  ["dist/vendor/pdfjs/pdf.worker.min.mjs", "the pdf.js worker", RUNTIME_FLOOR_BYTES],
  ["dist/vendor/pdfjs/pdf_viewer.css", "the pdf.js text-layer stylesheet", 0],
];

/** The shell page's boot placeholder and the id the shell removes on mount. */
const SHELL_BOOT_ID = 'id="shell-boot"';
const SHELL_BOOT_COPY = "Loading MAReader";
const SHELL_BOOT_MARK = 'class="loader shell-boot__loader"';
const SHELL_BOOT_WATCHDOG = "/shellBoot.js";
const SCRIPT_SRC = /<script(?=\s|>)(?:[^"'<>]|"[^"]*"|'[^']*')*?\ssrc\s*=\s*(?:"([^"]*)"|'([^']*)'|([^\s>]+))[^>]*>/gi;

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

// The shell page's loading state: the one hand-written file that can regress.
const indexHtml = path.join(root, "dist/index.html");
if (fs.existsSync(indexHtml)) {
  const html = fs.readFileSync(indexHtml, "utf8");
  if (!html.includes("/rasterLane.js")) {
    problems.push("dist/index.html does not load the window raster coordinator");
  }
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
  // The placeholder carries the Loader's class pair; dropping it blanks.
  if (!html.includes(SHELL_BOOT_MARK)) {
    problems.push(
      `dist/index.html's boot placeholder carries no ${SHELL_BOOT_MARK} — the boot ` +
        `screen must animate while the shell starts (styles/boot.css)`,
    );
  }
  const scripts = html.replace(/<!--[\s\S]*?-->/g, "").matchAll(SCRIPT_SRC);
  if (![...scripts].some((match) => (match[1] ?? match[2] ?? match[3]) === SHELL_BOOT_WATCHDOG)) {
    problems.push(
      `dist/index.html does not load ${SHELL_BOOT_WATCHDOG} — nothing would say ` +
        `"the shell did not start" when a launch fails, and the mark would run forever`,
    );
  }
}

// Library startup allocates no download buffers and warms no document code.
const watchdogPath = path.join(root, "dist/shellBoot.js");
if (fs.existsSync(watchdogPath) && /\bfetch\s*\(/.test(fs.readFileSync(watchdogPath, "utf8"))) {
  problems.push("Shell's boot watchdog fetches artifacts without a real Reader open");
}

// Shell and Library are format-neutral: only the PDF pane may reach PDFReader.
const distDir = path.join(root, "dist");
if (fs.existsSync(distDir)) {
  const glue = fs
    .readdirSync(distDir)
    .filter((name) => /^(mareader|library|reader|reflow)(-[0-9a-f]+)?\.js$/.test(name));
  for (const runtime of ["mareader", "library", "reader", "reflow"]) {
    if (!glue.some((name) => name.startsWith(runtime))) {
      problems.push(`dist has no ${runtime}*.js — that runtime's artifact is missing`);
    }
  }
  for (const name of glue) {
    if (fs.readFileSync(path.join(distDir, name), "utf8").includes("PDFReader")) {
      problems.push(
        `dist/${name} references PDFReader — a non-PDF runtime reaches the PDF engine, ` +
          `which only PDF pane frames may load`,
      );
    }
  }
}

// Neither route runtime nor text panes may load PDF machinery indirectly.
for (const runtime of ["index", "library", "reader", "reflow"]) {
  const page = path.join(root, `dist/${runtime}.html`);
  if (!fs.existsSync(page)) continue;
  const html = fs.readFileSync(page, "utf8");
  if (/<script[^>]+src=["'][^"']*(pdfEngine|pdf\.min\.mjs|pdf\.worker)/.test(html)) {
    problems.push(`dist/${runtime}.html loads PDF machinery — only PDF panes may load it`);
  }
}
const readerPage = path.join(root, "dist/reader.html");
if (fs.existsSync(readerPage) && !fs.readFileSync(readerPage, "utf8").includes("/readerHost.js")) {
  problems.push("dist/reader.html lacks its shared scoped raster bridge");
}
const pdfGlue = path.join(root, "dist/pdf.js");
if (fs.existsSync(pdfGlue) && !fs.readFileSync(pdfGlue, "utf8").includes("PDFReader")) {
  problems.push("dist/pdf.js has no PDFReader imports — the PDF runtime lost its engine");
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

// Per-artifact sizes ride the log; the split's promise is checked against them.
const sizes = REQUIRED.map(
  ([rel]) => `    ${rel} ${fs.statSync(path.join(root, rel)).size}`,
).join("\n");
const total = REQUIRED.reduce(
  (sum, [rel]) => sum + fs.statSync(path.join(root, rel)).size,
  0,
);
console.log(
  `runtime artifact contract OK: ${REQUIRED.length} files, ${total} bytes ` +
    `(Shell + Library + Reader host + PDF pane + reflow pane + shared assets)\n${sizes}`,
);
