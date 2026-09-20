// Bundle the browser-side TypeScript to single IIFEs.
// Invoked via `node` so Trunk can spawn it on Windows (no npx / .cmd).

import * as esbuild from "esbuild";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

/** One IIFE bundle. The entry and the output are the only facts that differ
 *  between the three; every other option was repeated in each. */
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

// Host lifecycle code and reader-local bridge; neither imports the PDF engine.
await bundle(["host/host.ts"], "public/host.js");
await bundle(["host/reader-runtime.ts"], "public/reader-runtime.js");
