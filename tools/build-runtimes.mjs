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
  for (const app of ["library", "reader-pdf", "reader-txt", "reader-md"]) {
    const destination = resolve(stage, app);
    run("trunk", ["--config", "Trunk.reader.toml", "build",
      `apps/${app}-wasm/index.html`, "--dist", destination,
      "--public-url", `/${app}/`, "--locked", ...(release ? ["--release"] : [])]);
    cpSync(destination, resolve("dist", app), { recursive: true });
  }
} finally {
  rmSync(stage, { recursive: true, force: true });
}
