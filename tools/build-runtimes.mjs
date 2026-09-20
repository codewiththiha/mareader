// Each Trunk invocation selects ONE app and ONE UI feature set. A workspace
// build would unify Cargo features and erase the dependency boundary.
import { spawnSync } from "node:child_process";
import { mkdirSync, cpSync, rmSync } from "node:fs";
import { resolve } from "node:path";
const release = !process.argv.includes("--dev");
const stage = resolve(".runtime-staging");
function run(command, args) {
  const result = spawnSync(command, args, { stdio: "inherit", shell: false });
  if (result.error) throw result.error;
  if (result.status !== 0) process.exit(result.status ?? 1);
}
// Root build runs the shared Trunk hooks (TS tooling, engines and CSS).
run("trunk", ["build", ...(release ? ["--release"] : []), "--locked"]);
mkdirSync(stage, { recursive: true });
try {
  for (const format of ["pdf", "txt", "md"]) {
    const destination = resolve(stage, `reader-${format}`);
    run("trunk", ["--config", "Trunk.reader.toml", "build",
      `apps/reader-${format}-wasm/index.html`, "--dist", destination,
      "--public-url", `/reader-${format}/`, "--locked", ...(release ? ["--release"] : [])]);
    cpSync(destination, resolve("dist", `reader-${format}`), { recursive: true });
  }
} finally {
  rmSync(stage, { recursive: true, force: true });
}
