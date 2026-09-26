#!/usr/bin/env node
// Copy the merged runtime artifacts into Trunk's staging directory before the
// staging tree is swapped over dist/ ("applying new distribution"). Trunk owns
// dist/ while serving, and every apply replaces it wholesale — a shell-only
// staging tree therefore drops reader.html / library.html / library.js / the
// wasm modules, and the window between that swap and the orchestrator's next
// restore is a 404 on exactly the file the shell is dynamic-importing (the
// "MAReader could not start the Library runtime … failed during module load"
// boot failure). Staging the runtimes inside the build that performs the swap
// makes every applied distribution complete on arrival.
//
// Wired as Trunk.toml's post_build hook. TRUNK_STAGING_DIR is one of the
// default variables Trunk inserts into a hook's environment; the
// reader/library legs run their own config files (no hooks), so this fires
// exactly where a swap can drop files: the shell build that `trunk serve`
// and build-dist.sh's first leg run.
//
// Never fails a build: on a fresh clone the shell leg builds before the
// runtime legs, so the sources simply are not there yet — skip, because
// build-dist.sh's merge step and dev.mjs's restore loop still guard the
// artifact contract. A failure here would take down every build for a
// best-effort copy.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const staging = process.env.TRUNK_STAGING_DIR;
if (!staging) process.exit(0);

/** [destination, sources in priority order] — the built page's name is
 *  Trunk's choice (the target it built, or index.html when normalized). */
const MERGED = [
  ["reader.html", ["dist-reader/reader.html", "dist-reader/index.html"]],
  ["reader.js", ["dist-reader/reader.js"]],
  ["reader_bg.wasm", ["dist-reader/reader_bg.wasm"]],
  ["library.html", ["dist-library/library.html", "dist-library/index.html"]],
  ["library.js", ["dist-library/library.js"]],
  ["library_bg.wasm", ["dist-library/library_bg.wasm"]],
];

const staged = [];
for (const [dest, sources] of MERGED) {
  const src = sources.map((rel) => path.join(root, rel)).find((abs) => fs.existsSync(abs));
  if (!src) continue;
  try {
    fs.copyFileSync(src, path.join(staging, dest));
    staged.push(dest);
  } catch (e) {
    console.warn(`[stage-merged] could not stage ${dest}: ${e.message}`);
  }
}
if (staged.length > 0) {
  console.log(`[stage-merged] distribution carries: ${staged.join(", ")}`);
}
