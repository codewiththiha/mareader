// Bundle the browser-side TypeScript to single IIFEs.
// Invoked via `node` so Trunk can spawn it on Windows (no npx / .cmd).

import * as esbuild from "esbuild";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

/** One IIFE bundle. The entry and the output are the only facts that differ
 *  between the engine bundles; every other option was repeated in each. */
function bundle(entryPoints, outfile) {
  return esbuild.build({
    absWorkingDir: root,
    entryPoints,
    bundle: true,
    format: "iife",
    outfile,
    target: "es2022",
    logLevel: "info",
  });
}

// The pdf.js-facing engine.
await bundle(["public/pdfEngine.ts"], "public/pdfEngine.js");

// The reader bundle: the format-agnostic browser side (the selection
// tracker). Separate from the engine so a document that never touches pdf.js
// does not carry it, and so nothing in here can import the pdf.js-facing
// modules.
await bundle(["public/readerEngine.ts"], "public/readerEngine.js");

// The theme bake worker: a separate classic worker file so the per-pixel
// filter loop runs off the main thread. Shares the filter kernel module with
// the main bundle, so worker and inline fallback cannot drift. Emitted next
// to pdfEngine.js so index.html can copy-file it to the dist root — copying
// public/engine/ wholesale would ship the TypeScript sources.
await bundle(["public/engine/theme/bake.worker.ts"], "public/bake.worker.js");

// The session boot. ESM, not an IIFE: it top-level-awaits the engines before
// the wasm module evaluates, and an IIFE cannot await. Dynamic import() of a
// variable is left as an import, so pdf.js is not inlined into this file.
await esbuild.build({
  absWorkingDir: root,
  entryPoints: ["public/session/boot.ts"],
  bundle: true,
  format: "esm",
  outfile: "public/sessionBoot.js",
  platform: "browser",
  target: "es2022",
  logLevel: "info",
});

// The handoff decisions, emitted for the web-lane test. No DOM.
await esbuild.build({
  absWorkingDir: root,
  entryPoints: ["public/session/handoff.ts"],
  bundle: true,
  format: "esm",
  outfile: "scripts/session-handoff.js",
  platform: "neutral",
  target: "es2022",
  logLevel: "info",
});
