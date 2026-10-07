// The Tauri ⇄ CI contract: one canonical build, one dev URL, one port.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import os from "node:os";
import assert from "node:assert/strict";
import { RUNTIMES, RUNTIME_INPUTS, mergeRuntimeArtifacts } from "./runtime-artifacts.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

const CANONICAL_BUILDER = "tools/build-dist.sh";
const DEV_ORCHESTRATOR = "tools/dev.mjs";

const problems = [];
const fail = (message) => problems.push(message);

function readJson(rel) {
  try {
    return JSON.parse(fs.readFileSync(path.join(root, rel), "utf8"));
  } catch (e) {
    fail(`cannot read ${rel}: ${e.message}`);
    return null;
  }
}

function readText(rel) {
  try {
    return fs.readFileSync(path.join(root, rel), "utf8");
  } catch (e) {
    fail(`cannot read ${rel}: ${e.message}`);
    return null;
  }
}

const conf = readJson("src-tauri/tauri.conf.json");
const pkg = readJson("package.json");

const build = conf?.build ?? {};
const scripts = pkg?.scripts ?? {};

// 1. frontendDist is the canonical builder's output directory.
if (build.frontendDist !== "../dist") {
  fail(
    `tauri.conf.json build.frontendDist is ${JSON.stringify(build.frontendDist)}, ` +
      `expected "../dist" (the directory ${CANONICAL_BUILDER} writes)`,
  );
}

// 2. Both commands go through the canonical paths, by npm script name.
const beforeBuild = String(build.beforeBuildCommand ?? "");
const beforeDev = String(build.beforeDevCommand ?? "");

const buildIndirect = /npm run build:dist\b/.test(beforeBuild);
// The freshness gate is canonical only if it runs the dev orchestrator.
const buildGated = /npm run build:gate\b/.test(beforeBuild);
const buildDirect = beforeBuild.includes(CANONICAL_BUILDER);
if (!buildIndirect && !buildGated && !buildDirect) {
  fail(
    `tauri.conf.json build.beforeBuildCommand is ${JSON.stringify(beforeBuild)} — ` +
      `it must run the canonical multi-artifact build (npm run build:dist, i.e. ` +
      `${CANONICAL_BUILDER}). Tauri must not build the frontend by another path.`,
  );
}
if (buildGated) {
  const gateScript = String(scripts["build:gate"] ?? "");
  if (!gateScript.includes(DEV_ORCHESTRATOR) || !gateScript.includes("--build-only")) {
    fail(
      `package.json scripts.build:gate is ${JSON.stringify(gateScript)} — the gated ` +
        `beforeBuildCommand must run ${DEV_ORCHESTRATOR} --build-only (the freshness ` +
        `gate around ${CANONICAL_BUILDER}), nothing else`,
    );
  }
  const devOrchestrator = readText(DEV_ORCHESTRATOR) ?? "";
  if (!devOrchestrator.includes(CANONICAL_BUILDER)) {
    fail(
      `${DEV_ORCHESTRATOR} never invokes ${CANONICAL_BUILDER} — the gate would replace ` +
        `the canonical build instead of deciding whether to run it`,
    );
  }
}
if (/trunk build/.test(beforeBuild)) {
  fail(
    `tauri.conf.json build.beforeBuildCommand runs a bare \`trunk build\` — that ` +
      `produces the shell page only and is exactly how the blank window shipped`,
  );
}

if (scripts["build:dist"] && !scripts["build:dist"].includes(CANONICAL_BUILDER)) {
  fail(
    `package.json scripts.build:dist is ${JSON.stringify(scripts["build:dist"])} — ` +
      `expected it to invoke ${CANONICAL_BUILDER}`,
  );
}
if (buildIndirect && !scripts["build:dist"]) {
  fail("tauri.conf.json calls `npm run build:dist` but package.json defines no such script");
}

const devIndirect = /npm run dev:frontend\b/.test(beforeDev);
const devDirect = beforeDev.includes(DEV_ORCHESTRATOR);
if (!devIndirect && !devDirect) {
  fail(
    `tauri.conf.json build.beforeDevCommand is ${JSON.stringify(beforeDev)} — ` +
      `it must run ${DEV_ORCHESTRATOR} (which builds both runtime artifacts and ` +
      `proves they are served before the shell boots), not a bare \`trunk serve\``,
  );
}
if (scripts["dev:frontend"] && !scripts["dev:frontend"].includes(DEV_ORCHESTRATOR)) {
  fail(
    `package.json scripts.dev:frontend is ${JSON.stringify(scripts["dev:frontend"])} — ` +
      `expected it to invoke ${DEV_ORCHESTRATOR}`,
  );
}
if (devIndirect && !scripts["dev:frontend"]) {
  fail("tauri.conf.json calls `npm run dev:frontend` but package.json defines no such script");
}
if (/trunk serve/.test(beforeDev)) {
  fail(
    `tauri.conf.json build.beforeDevCommand runs a bare \`trunk serve\` — it can ` +
      `serve a shell whose dynamic imports 404 (no runtime artifacts built yet)`,
  );
}

// devUrl's port is Trunk's: a divergence means Tauri waits on nothing.
const trunk = readText("Trunk.toml");
const servePort = /\[serve\][\s\S]*?port\s*=\s*(\d+)/.exec(trunk ?? "")?.[1];
const devUrlPort = /^https?:\/\/[^/:]+:(\d+)/.exec(String(build.devUrl ?? ""))?.[1];
if (!servePort) {
  fail("Trunk.toml has no [serve] port — the dev server's port is unpinned");
} else if (servePort !== devUrlPort) {
  fail(
    `tauri.conf.json devUrl is ${JSON.stringify(build.devUrl)} (port ${devUrlPort}) but ` +
      `Trunk.toml serves on ${servePort} — Tauri would wait on a URL nothing serves`,
  );
}

// 4. CI runs the canonical builder, and the builder verifies its own output.
let ciRunsBuilder = false;
const workflows = fs.existsSync(path.join(root, ".github/workflows"))
  ? fs.readdirSync(path.join(root, ".github/workflows")).filter((f) => f.endsWith(".yml"))
  : [];
for (const wf of workflows) {
  const text = readText(`.github/workflows/${wf}`) ?? "";
  if (text.includes(CANONICAL_BUILDER)) ciRunsBuilder = true;
}
if (!ciRunsBuilder) {
  fail(
    `.github/workflows/*.yml never runs ${CANONICAL_BUILDER} — CI would validate a ` +
      `different frontend than Tauri packages`,
  );
}

const builderScript = readText(CANONICAL_BUILDER) ?? "";
if (!builderScript.includes("check-runtime-artifacts")) {
  fail(
    `${CANONICAL_BUILDER} does not run tools/check-runtime-artifacts.mjs — the build ` +
      `would not fail on a missing runtime artifact`,
  );
}

// Neither command may assemble the frontend by hand; no second build path.
const NATIVE_SMOKE = "tools/tauri-smoke.mjs";
for (const [name, command] of [
  ["beforeBuildCommand", beforeBuild],
  ["beforeDevCommand", beforeDev],
]) {
  if (/\bcp\b|\bcopy\b|xcopy|robocopy/.test(command)) {
    fail(
      `tauri.conf.json build.${name} copies files by hand (${JSON.stringify(command)}) — ` +
        `the artifact merge belongs to ${CANONICAL_BUILDER}, not to the Tauri config`,
    );
  }
}

// The native smoke turns "the window opened and showed nothing" into a red run.
if (!fs.existsSync(path.join(root, NATIVE_SMOKE))) {
  fail(`${NATIVE_SMOKE} is missing — nothing would fail on a native blank window`);
}
let ciRunsNativeSmoke = false;
const deepText = readText(".github/workflows/deep-ci.yml") ?? "";
for (const wf of workflows) {
  const text = readText(`.github/workflows/${wf}`) ?? "";
  if (text.includes(NATIVE_SMOKE)) ciRunsNativeSmoke = true;
}
if (!ciRunsNativeSmoke) {
  fail(
    `no workflow runs ${NATIVE_SMOKE} — the packaged app's boot (the blank ` +
      `window's side of the contract) would go unchecked`,
  );
}

// The smoke's lane must not skip the shell, the Tauri config or the builder.
const requiredPaths = ["src/**", "src-tauri/**", "index.html", CANONICAL_BUILDER];
for (const required of requiredPaths) {
  if (!deepText.includes(`"${required}"`)) {
    fail(
      `.github/workflows/deep-ci.yml's push paths do not include ${required} — a ` +
        `boot-affecting change could skip the lane that tests it`,
    );
  }
}

// Every route/pane entry and Trunk config participates in dev invalidation.
const devSource = readText(DEV_ORCHESTRATOR) ?? "";
assert.deepEqual(RUNTIMES.map((r) => r.name), ["library", "reader", "pdf", "reflow"]);
assert.equal(RUNTIME_INPUTS.length, 8);
if (!/const WATCHED_FILES[^;]+\.\.\.RUNTIME_INPUTS/.test(devSource)) {
  fail("dev watcher does not include every route/pane entry and Trunk config");
}
if (!builderScript.includes("runtime-artifacts.mjs --build")) {
  fail("canonical builder does not use the shared runtime artifact layout");
}

// Copy-policy fixtures: strict naming, Trunk normalization, first-leg staging.
const fixture = fs.mkdtempSync(path.join(os.tmpdir(), "mareader-artifacts-"));
try {
  for (const { name, page, directory } of RUNTIMES) {
    const dir = path.join(fixture, directory);
    fs.mkdirSync(dir);
    for (const file of [page, `${name}.js`, `${name}_bg.wasm`]) {
      fs.writeFileSync(path.join(dir, file), file);
    }
    fs.writeFileSync(path.join(dir, "index.html"), "normalized");
  }
  const output = path.join(fixture, "merged");
  assert.equal(mergeRuntimeArtifacts(output, { sourceRoot: fixture }).length, 12);
  assert.equal(fs.readFileSync(path.join(output, "reader.html"), "utf8"), "reader.html");
  fs.unlinkSync(path.join(fixture, "dist-reader/reader.html"));
  mergeRuntimeArtifacts(output, { sourceRoot: fixture });
  assert.equal(fs.readFileSync(path.join(output, "reader.html"), "utf8"), "normalized");
  fs.unlinkSync(path.join(fixture, "dist-reader/index.html"));
  fs.writeFileSync(path.join(fixture, "dist-reader/unrelated.html"), "not the Reader");
  assert.throws(() => mergeRuntimeArtifacts(output, { sourceRoot: fixture }), /dist-reader has no artifact for reader.html/);
  fs.rmSync(output, { recursive: true });
  const staged = mergeRuntimeArtifacts(output, { sourceRoot: fixture, required: false });
  assert.equal(staged.length, 11);
  assert(!fs.existsSync(path.join(output, "reader.html")));
  assert.deepEqual(mergeRuntimeArtifacts(output, { sourceRoot: fixture, required: false, onlyMissing: true }), []);
} finally {
  fs.rmSync(fixture, { recursive: true, force: true });
}

if (problems.length > 0) {
  console.error("tauri build contract FAILED:");
  for (const problem of problems) console.error(`  - ${problem}`);
  process.exit(1);
}

console.log(
  "tauri build contract OK: Tauri and CI share one canonical frontend build " +
    `(${CANONICAL_BUILDER}), dev boots through ${DEV_ORCHESTRATOR}, devUrl matches Trunk, ` +
    `and ${NATIVE_SMOKE} is wired into CI`,
);
