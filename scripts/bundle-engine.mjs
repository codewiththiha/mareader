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
