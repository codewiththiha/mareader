// The Tauri ⇄ CI contract: one canonical build, one dev URL, one port.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import os from "node:os";
import assert from "node:assert/strict";
import {
  BUILD_TARGETS,
  RUNTIMES,
  RUNTIME_INPUTS,
  mergeRuntimeArtifacts,
  parseBuildArgs,
} from "./runtime-artifacts.mjs";
import {
  cargoDependencyRoots,
  hasReleaseTargetManifest,
  mergedReleaseTargets,
  RELEASE_FINGERPRINT_SCHEMA,
  staleReleaseTargets,
} from "./release-freshness.mjs";

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
  if (/\brun:\s*(?:npm run build:dist|sh tools\/build-dist\.sh)\b/.test(text)) {
    ciRunsBuilder = true;
  }
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
function workflowJob(name) {
  const marker = `  ${name}:\n`;
  const start = deepText.indexOf(marker);
  if (start < 0) return "";
  const body = deepText.slice(start + marker.length);
  const next = body.search(/\n  [A-Za-z][A-Za-z0-9_-]*:\n/);
  return next < 0 ? body : body.slice(0, next);
}
const frontendJob = workflowJob("frontend-build");
if (!frontendJob.includes("npm run build:dist") ||
    !frontendJob.includes("actions/upload-artifact@v5") ||
    !frontendJob.includes("name: frontend-dist")) {
  fail("Deep CI must build and upload one canonical `frontend-dist` artifact");
}
for (const jobName of ["browser-lifecycle", "tauri-smoke", "memory-replay"]) {
  const job = workflowJob(jobName);
  if (!job.includes("frontend-build") && jobName !== "memory-replay") {
    fail(`Deep CI ${jobName} must depend on the shared frontend build`);
  }
  if (!job.includes("actions/download-artifact@v5") || !job.includes("name: frontend-dist")) {
    fail(`Deep CI ${jobName} must download the shared frontend-dist artifact`);
  }
  if (/npm run build:dist|tools\/build-dist\.sh/.test(job)) {
    fail(`Deep CI ${jobName} must not build a second production frontend`);
  }
}
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
const cargoToml = readText("Cargo.toml") ?? "";
function tomlProfile(name) {
  const start = cargoToml.indexOf(`[profile.${name}]`);
  if (start < 0) return "";
  const body = cargoToml.slice(start);
  const next = body.search(/\n\[/);
  return next < 0 ? body : body.slice(0, next);
}
const nativeRelease = tomlProfile("release");
const wasmRelease = tomlProfile("wasm-release");
if (!/opt-level\s*=\s*"z"/.test(nativeRelease) ||
    !/lto\s*=\s*true/.test(nativeRelease) ||
    !/codegen-units\s*=\s*1/.test(nativeRelease)) {
  fail("the native release profile must keep size optimisation, fat LTO and one codegen unit");
}
if (!/inherits\s*=\s*"release"/.test(wasmRelease) ||
    !/lto\s*=\s*"thin"/.test(wasmRelease) ||
    !/codegen-units\s*=\s*16/.test(wasmRelease) ||
    !/incremental\s*=\s*true/.test(wasmRelease)) {
  fail("the frontend wasm-release profile must inherit release with thin LTO, 16 units and incremental builds");
}
for (const page of ["index.html", "library.html", "reader.html", "pdf.html", "reflow.html"]) {
  const html = readText(page) ?? "";
  if (!html.includes('data-cargo-profile-release="wasm-release"') ||
      !html.includes('data-wasm-opt="z"')) {
    fail(`${page} must use wasm-release and retain release wasm-opt=z`);
  }
}
if (!deepText.includes('CARGO_INCREMENTAL: "0"')) {
  fail("Deep CI must disable incremental artifacts to keep the hosted build cache lean");
}
const releaseFreshnessSource = readText("tools/release-freshness.mjs") ?? "";
if (!devSource.includes('"metadata"') || !devSource.includes('"--no-deps"') ||
    !devSource.includes('"--only"') ||
    !releaseFreshnessSource.includes("function cargoDependencyRoots") ||
    !releaseFreshnessSource.includes("function staleReleaseTargets")) {
  fail("the release freshness gate must fingerprint Cargo dependencies and rebuild selected targets");
}
assert.deepEqual(RUNTIMES.map((r) => r.name), ["library", "reader", "pdf", "reflow"]);
assert.equal(RUNTIME_INPUTS.length, 8);
assert.deepEqual(BUILD_TARGETS, ["shell", "library", "reader", "pdf", "reflow"]);
assert.deepEqual(parseBuildArgs(["--release", "--only=pdf,shell"]), {
  args: ["--release"],
  targets: ["shell", "pdf"],
});
assert.deepEqual(parseBuildArgs([]), { args: [], targets: BUILD_TARGETS });
assert.throws(() => parseBuildArgs(["--only=unknown"]), /Invalid build targets/);
const targetFingerprints = Object.fromEntries(BUILD_TARGETS.map((target) => [target, `${target}-v1`]));
const releaseManifest = {
  profile: "release",
  schema: RELEASE_FINGERPRINT_SCHEMA,
  targets: targetFingerprints,
};
const allOutputsPresent = Object.fromEntries(BUILD_TARGETS.map((target) => [target, true]));
assert.equal(hasReleaseTargetManifest(releaseManifest), true);
assert.deepEqual(staleReleaseTargets({
  manifest: releaseManifest,
  targetFingerprints,
  outputsPresent: allOutputsPresent,
}), []);
const changedFingerprints = { ...targetFingerprints, reader: "reader-v2" };
const oneMissingOutput = { ...allOutputsPresent, pdf: false };
assert.deepEqual(staleReleaseTargets({
  manifest: releaseManifest,
  targetFingerprints: changedFingerprints,
  outputsPresent: oneMissingOutput,
}), ["reader", "pdf"]);
assert.deepEqual(staleReleaseTargets({
  manifest: null,
  targetFingerprints,
  outputsPresent: allOutputsPresent,
}), BUILD_TARGETS);
assert.deepEqual(staleReleaseTargets({
  manifest: { ...releaseManifest, schema: 0 },
  targetFingerprints,
  outputsPresent: allOutputsPresent,
}), BUILD_TARGETS);
assert.deepEqual(staleReleaseTargets({
  manifest: releaseManifest,
  targetFingerprints,
  outputsPresent: allOutputsPresent,
  force: true,
}), BUILD_TARGETS);
assert.deepEqual(mergedReleaseTargets(releaseManifest, ["reader"], changedFingerprints), {
  ...targetFingerprints,
  reader: "reader-v2",
});
if (!/const WATCHED_FILES[^;]+\.\.\.RUNTIME_INPUTS/.test(devSource)) {
  fail("dev watcher does not include every route/pane entry and Trunk config");
}
if (!builderScript.includes("runtime-artifacts.mjs --build")) {
  fail("canonical builder does not use the shared runtime artifact layout");
}

// Copy-policy fixtures: strict naming, Trunk normalization, first-leg staging.
const fixture = fs.mkdtempSync(path.join(os.tmpdir(), "mareader-artifacts-"));
try {
  const workspace = path.join(fixture, "workspace");
  const pkg = (name, manifest, dependencies = []) => ({
    id: `${name} 1.0.0`,
    name,
    manifest_path: path.join(workspace, manifest),
    dependencies,
  });
  const graph = [
    pkg("mareader", "Cargo.toml", [{ name: "app-ui" }, { name: "reader-core" }]),
    pkg("app-ui", "crates/app-ui/Cargo.toml", [{ name: "runtime-contract" }]),
    pkg("runtime-contract", "crates/runtime-contract/Cargo.toml"),
    pkg("reader-core", "crates/reader-core/Cargo.toml"),
    pkg("library-runtime", "crates/library-runtime/Cargo.toml", [{ name: "library-core" }]),
    pkg("library-core", "crates/library-core/Cargo.toml"),
    pkg("reader-runtime", "crates/reader-runtime/Cargo.toml", [{ name: "reader-core" }]),
    pkg("unrelated", "crates/unrelated/Cargo.toml"),
  ];
  assert.deepEqual(cargoDependencyRoots("shell", graph, workspace).sort(), [
    "build.rs",
    "crates/app-ui",
    "crates/reader-core",
    "crates/runtime-contract",
    "src",
  ]);
  assert.deepEqual(cargoDependencyRoots("library", graph, workspace).sort(), [
    "crates/library-core",
    "crates/library-runtime",
  ]);
  assert.deepEqual(cargoDependencyRoots("reader", graph, workspace).sort(), [
    "crates/reader-core",
    "crates/reader-runtime",
  ]);
  assert.deepEqual(cargoDependencyRoots("pdf", graph, workspace).sort(),
    cargoDependencyRoots("reader", graph, workspace).sort());
  assert.deepEqual(cargoDependencyRoots("reflow", graph, workspace).sort(),
    cargoDependencyRoots("reader", graph, workspace).sort());
  assert.deepEqual(cargoDependencyRoots("unknown", graph, workspace), ["crates"]);

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
