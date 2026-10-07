// One layout for canonical builds, Trunk staging and dev restoration.
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

export const BUILD_TARGETS = ["shell", ...RUNTIMES.map(({ name }) => name)];

export function parseBuildArgs(argv) {
  const args = [];
  let requestedTargets = null;
  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    if (arg === "--only") {
      if (requestedTargets !== null || !argv[index + 1]) {
        throw new Error("--only requires one comma-separated target list");
      }
      requestedTargets = argv[index + 1].split(",");
      index += 1;
    } else if (arg.startsWith("--only=")) {
      if (requestedTargets !== null) throw new Error("--only may be specified once");
      requestedTargets = arg.slice("--only=".length).split(",");
    } else {
      args.push(arg);
    }
  }

  const targets = requestedTargets ?? BUILD_TARGETS;
  const unknown = targets.filter((target) => !BUILD_TARGETS.includes(target));
  if (targets.length === 0 || targets.some((target) => !target) || unknown.length > 0) {
    throw new Error(
      `Invalid build targets: ${targets.join(", ") || "<empty>"}. ` +
        `Choose from ${BUILD_TARGETS.join(", ")}.`,
    );
  }
  return { args, targets: BUILD_TARGETS.filter((target) => targets.includes(target)) };
}

function build(args, targets) {
  const startedAt = Date.now();
  for (const target of targets) {
    const runtime = RUNTIMES.find(({ name }) => name === target);
    const config = runtime ? ["--config", runtime.config, "--dist", runtime.directory] : [];
    const targetStartedAt = Date.now();
    const child = spawnSync("trunk", ["build", ...config, ...args], { cwd: project, stdio: "inherit" });
    if (child.error) throw child.error;
    if (child.status !== 0) process.exit(child.status ?? 1);
    console.log(`[frontend] ${target} build: ${((Date.now() - targetStartedAt) / 1000).toFixed(1)}s`);
  }
  mergeRuntimeArtifacts(path.join(project, "dist"));
  console.log(`[frontend] selected builds + merge: ${((Date.now() - startedAt) / 1000).toFixed(1)}s`);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  if (process.argv[2] !== "--build") throw new Error("runtime-artifacts.mjs requires --build");
  const { args, targets } = parseBuildArgs(process.argv.slice(3));
  build(args, targets);
}
