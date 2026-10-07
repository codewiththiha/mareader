// Bundle the browser TypeScript to single IIFEs, spawnable by Trunk on Windows.

import * as esbuild from "esbuild";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

/** One IIFE bundle; only the entry and output differ between the three. */
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

// The host's format-neutral full-page budget, loaded by index.html alone.
await bundle(["public/rasterLane.ts"], "public/rasterLane.js");
await bundle(["public/readerHost.ts"], "public/readerHost.js");

// The pdf.js-facing engine.
await bundle(["public/pdfEngine.ts"], "public/pdfEngine.js");

// The reader bundle: the format-agnostic browser side, kept from the engine.
await bundle(["public/readerEngine.ts"], "public/readerEngine.js");

// The theme bake worker: a classic worker file, off the main thread.
await bundle(["public/engine/theme/bake.worker.ts"], "public/bake.worker.js");

// The Shell's cover-bake page script, bundled alone, loading no reader facade.
await bundle(["public/coverBake.ts"], "public/coverBake.js");
