// One layout for canonical builds, Trunk staging and dev restoration.
// Trunk may preserve the target page's name or normalize it to index.html;
// neither case permits guessing another HTML file in the output directory.
import fs from "node:fs";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const project = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
export const RUNTIMES = ["library", "reader", "pdf", "reflow"].map((name) => ({
  name, page: `${name}.html`, config: `${name}.Trunk.toml`, directory: `dist-${name}`,
}));
export const RUNTIME_INPUTS = RUNTIMES.flatMap(({ page, config }) => [page, config]);
export const RUNTIME_FILES = RUNTIMES.flatMap(({ name, page }) => [page, `${name}.js`, `${name}_bg.wasm`]);

export function mergeRuntimeArtifacts(destination, {
  sourceRoot = project, required = true, onlyMissing = false,
} = {}) {
  fs.mkdirSync(destination, { recursive: true });
  const copied = [];
  for (const { name, page, directory } of RUNTIMES) {
    for (const [file, candidates] of [
      [page, [page, "index.html"]],
      [`${name}.js`, [`${name}.js`]],
      [`${name}_bg.wasm`, [`${name}_bg.wasm`]],
    ]) {
      const target = path.join(destination, file);
      if (onlyMissing && fs.existsSync(target)) continue;
      const source = candidates.map((candidate) => path.join(sourceRoot, directory, candidate))
        .find((candidate) => fs.existsSync(candidate) && fs.statSync(candidate).isFile());
      if (!source) {
        if (required) throw new Error(`${directory} has no artifact for ${file}`);
        continue; // First Shell leg precedes the runtime builds on a clean checkout.
      }
      fs.copyFileSync(source, target); // IO failures are not a successful partial merge.
      copied.push(file);
    }
  }
  return copied;
}

function build(args) {
  for (const runtime of [null, ...RUNTIMES]) {
    const config = runtime ? ["--config", runtime.config, "--dist", runtime.directory] : [];
    const child = spawnSync("trunk", ["build", ...config, ...args], { cwd: project, stdio: "inherit" });
    if (child.error) throw child.error;
    if (child.status !== 0) process.exit(child.status ?? 1);
  }
  mergeRuntimeArtifacts(path.join(project, "dist"));
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv[2] !== "--build") throw new Error("runtime-artifacts.mjs requires --build");
  build(process.argv.slice(3));
}
