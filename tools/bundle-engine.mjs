// Bundle public/pdfEngine.ts (+ public/engine/*) to a single IIFE.
// Invoked via `node` so Trunk can spawn it on Windows (no npx / .cmd).

import * as esbuild from "esbuild";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

await esbuild.build({
  absWorkingDir: root,
  entryPoints: ["public/pdfEngine.ts"],
  bundle: true,
  format: "iife",
  outfile: "public/pdfEngine.js",
  target: "es2022",
  logLevel: "info",
});

// The reader bundle: the format-agnostic browser side (the selection
// tracker). Separate from the engine so a document that never touches pdf.js
// does not carry it, and so nothing in here can import the pdf.js-facing
// modules.
await esbuild.build({
  absWorkingDir: root,
  entryPoints: ["public/readerEngine.ts"],
  bundle: true,
  format: "iife",
  outfile: "public/readerEngine.js",
  target: "es2022",
  logLevel: "info",
});

// The theme bake worker: a separate classic worker file so the per-pixel
// filter loop runs off the main thread. Shares the filter kernel module with
// the main bundle, so worker and inline fallback cannot drift. Emitted next
// to pdfEngine.js so index.html can copy-file it to the dist root — copying
// public/engine/ wholesale would ship the TypeScript sources.
await esbuild.build({
  absWorkingDir: root,
  entryPoints: ["public/engine/theme/bake.worker.ts"],
  bundle: true,
  format: "iife",
  outfile: "public/bake.worker.js",
  target: "es2022",
  logLevel: "info",
});
