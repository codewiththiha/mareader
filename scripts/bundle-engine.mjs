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

// The theme bake worker: a separate classic worker file so the per-pixel
// filter loop runs off the main thread. It shares the filter kernel module
// with the main bundle, so worker and fallback cannot drift. Emitted to the
// repo root of the public dir (next to pdfEngine.js) so index.html can
// copy-file it to the dist root — copying the whole public/engine/ dir
// would ship the TypeScript sources.
await esbuild.build({
  absWorkingDir: root,
  entryPoints: ["public/engine/theme/bake.worker.ts"],
  bundle: true,
  format: "iife",
  outfile: "public/bake.worker.js",
  target: "es2022",
  logLevel: "info",
});
