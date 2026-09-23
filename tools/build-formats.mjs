// The three format artifacts and the shelf artifact. Trunk's post_build hook
// runs this, and the staging dir it writes is what becomes dist/formats and
// dist/sessions. Without these files `trunk serve` answers the glue URL with
// index.html. Importing that page is `Unexpected token '<'` in a blob the
// debugger names `source`.
//
// Three cargo processes, not one: one invocation unifies features, so a text
// artifact would still compile the PDF open arm.

import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, renameSync, statSync } from "node:fs";
import { homedir } from "node:os";
import { delimiter, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { patchGlueFile } from "./patch-wasm-glue.mjs";

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const release = process.env.TRUNK_PROFILE === "release";
const outDir = process.env.TRUNK_STAGING_DIR
  ? join(process.env.TRUNK_STAGING_DIR, "formats")
  : join(root, "dist", "formats");

const sessionDir = process.env.TRUNK_STAGING_DIR
  ? join(process.env.TRUNK_STAGING_DIR, "sessions")
  : join(root, "dist", "sessions");

const formats = [
  { pkg: "format-pdf", wasm: "format_pdf.wasm", name: "pdf", dir: outDir },
  { pkg: "format-text", wasm: "format_text.wasm", name: "text", dir: outDir },
  { pkg: "format-md", wasm: "format_md.wasm", name: "md", dir: outDir },
  { pkg: "session-library", wasm: "session_library.wasm", name: "library", dir: sessionDir },
];

function toolPath(name) {
  const exe = process.platform === "win32" ? `${name}.exe` : name;
  const cargoHome = process.env.CARGO_HOME || join(homedir(), ".cargo");
  const dirs = [...(process.env.PATH || "").split(delimiter), join(cargoHome, "bin")];
  for (const dir of dirs) {
    if (!dir) continue;
    const full = join(dir, exe);
    if (existsSync(full)) return full;
  }
  return null;
}

function run(command, args) {
  const result = spawnSync(command, args, { cwd: root, stdio: "inherit" });
  if (result.error) {
    console.error(result.error.message);
    process.exit(1);
  }
  if (result.status !== 0) process.exit(result.status ?? 1);
}

function requireTool(name, install) {
  const found = toolPath(name);
  if (found) return found;
  console.error(`${name} is not on PATH.`);
  console.error(install);
  process.exit(1);
}

const bindgen = requireTool(
  "wasm-bindgen",
  "Install the lock's version: cargo install wasm-bindgen-cli --version 0.2.127 --locked",
);
const version = spawnSync(bindgen, ["--version"], { encoding: "utf8" });
const reported = `${version.stdout || ""} ${version.stderr || ""}`;
if (!reported.includes("0.2.127")) {
  console.error(`wasm-bindgen must be 0.2.127 (the lock). Found: ${reported.trim() || "unknown"}`);
  process.exit(1);
}

const opt = release ? requireTool("wasm-opt", "Install binaryen so wasm-opt is on PATH.") : null;

mkdirSync(outDir, { recursive: true });
mkdirSync(sessionDir, { recursive: true });

const profileDir = release ? "release" : "debug";

for (const format of formats) {
  const cargoArgs = [
    "build",
    "-p",
    format.pkg,
    "--target",
    "wasm32-unknown-unknown",
    "--locked",
  ];
  if (release) cargoArgs.push("--release");
  run("cargo", cargoArgs);

  const input = join(root, "target", "wasm32-unknown-unknown", profileDir, format.wasm);
  const glue = join(format.dir, `${format.name}.js`);
  const wasm = join(format.dir, `${format.name}_bg.wasm`);
  const fresh =
    existsSync(glue) &&
    existsSync(wasm) &&
    statSync(glue).mtimeMs >= statSync(input).mtimeMs &&
    statSync(wasm).mtimeMs >= statSync(input).mtimeMs;
  if (fresh) continue;

  run(bindgen, ["--target", "web", "--out-dir", format.dir, "--out-name", format.name, input]);
  if (opt) {
    const optimized = join(format.dir, `${format.name}_bg.opt.wasm`);
    run(opt, ["-Oz", "-o", optimized, wasm]);
    renameSync(optimized, wasm);
  }
  patchGlueFile(glue);
}

// A fresh glue from an earlier build still needs release(). Skipping bindgen
// must not skip the unmount.
for (const format of formats) {
  const glue = join(format.dir, `${format.name}.js`);
  if (existsSync(glue)) patchGlueFile(glue);
}
