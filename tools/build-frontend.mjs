// Trunk builds the host. This builds the three format artifacts after it and
// copies them into dist/formats. Not a second Trunk target. The dist directory
// does not move.
//
// Windows can run this; a shell script cannot be Tauri's beforeBuildCommand.

import { spawnSync } from "node:child_process";
import { mkdirSync, renameSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");

function run(command, args) {
  const result = spawnSync(command, args, { cwd: root, stdio: "inherit" });
  if (result.error) {
    console.error(result.error.message);
    process.exit(1);
  }
  if (result.status !== 0) {
    process.exit(result.status ?? 1);
  }
}

const formats = [
  { pkg: "format-pdf", wasm: "format_pdf.wasm", name: "pdf" },
  { pkg: "format-text", wasm: "format_text.wasm", name: "text" },
  { pkg: "format-md", wasm: "format_md.wasm", name: "md" },
];

run("trunk", ["build", "--release"]);

mkdirSync(join(root, "dist", "formats"), { recursive: true });

for (const format of formats) {
  run("cargo", [
    "build",
    "-p",
    format.pkg,
    "--release",
    "--target",
    "wasm32-unknown-unknown",
    "--locked",
  ]);
  const input = join(
    root,
    "target",
    "wasm32-unknown-unknown",
    "release",
    format.wasm,
  );
  const outDir = join(root, "dist", "formats");
  run("wasm-bindgen", [
    "--target",
    "web",
    "--out-dir",
    outDir,
    "--out-name",
    format.name,
    input,
  ]);
  const wasm = join(outDir, `${format.name}_bg.wasm`);
  const opt = join(outDir, `${format.name}_bg.opt.wasm`);
  // -Oz is the size pass. A missing wasm-opt is a failed release, not a
  // larger artifact shipped quietly.
  run("wasm-opt", ["-Oz", "-o", opt, wasm]);
  renameSync(opt, wasm);
}
