// Version-sync check — the cheapest insurance in the release pipeline.
//
// The app version lives in FOUR places in this repo (the workspace root plus
// the Tauri shell crate):
//   - package.json            (.version)
//   - src-tauri/tauri.conf.json (.version)   <- what Tauri bundles
//   - Cargo.toml              (pdf-reader, [package].version)
//   - src-tauri/Cargo.toml    (pdf,        [package].version)
//
// Release tags and artifact filenames are derived from it, so if any of the
// four drift, releases break invisibly. This script fails CI when they
// disagree. (Tag-vs-version agreement is validated by the release workflow's
// metadata job, which is the only place a tag is in hand.)
//
// This is the TypeScript source; Trunk's pre-build hook compiles it to
// `scripts/check-versions.js` so CI can run it with plain `node`.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

type VersionSource = [file: string, version: string];

function readJson(rel: string): unknown {
  return JSON.parse(fs.readFileSync(path.join(root, rel), "utf8"));
}

function readCargoVersion(rel: string): string {
  const text = fs.readFileSync(path.join(root, rel), "utf8");
  const m = /^version\s*=\s*"([^"]+)"/m.exec(text);
  if (!m || !m[1]) throw new Error(`no [package] version found in ${rel}`);
  return m[1];
}

const sources: VersionSource[] = [
  ["package.json", (readJson("package.json") as { version: string }).version],
  [
    "src-tauri/tauri.conf.json",
    (readJson("src-tauri/tauri.conf.json") as { version: string }).version,
  ],
  ["Cargo.toml", readCargoVersion("Cargo.toml")],
  ["src-tauri/Cargo.toml", readCargoVersion("src-tauri/Cargo.toml")],
];

const versions = new Set(sources.map(([, v]) => v));
if (versions.size !== 1) {
  console.error("::error::Version mismatch across files:");
  for (const [file, v] of sources) console.error(`  ${file}: ${v}`);
  process.exit(1);
}

const version = sources[0]![1];
console.log(`versions agree: ${version}`);
