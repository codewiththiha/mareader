// The Tauri ⇄ CI build-contract check.
//
// The blank-window regression had one shape: CI built all three artifacts
// (tools/build-dist.sh) while Tauri still ran `trunk build --release`, so the
// packaged app carried a shell page whose dynamic imports resolved to nothing.
// Two builds for one responsibility is the bug; this check makes the
// divergence a red build instead of a review promise.
//
// It asserts, deterministically and without needing a built dist:
//   1. tauri.conf.json packages `../dist` — the directory the canonical
//      builder writes.
//   2. both Tauri commands (beforeBuildCommand, beforeDevCommand) invoke the
//      canonical builder / dev orchestrator through the npm script names that
//      package.json defines, and those scripts point at the same files.
//   3. beforeDevCommand cannot start a shell whose runtime artifacts are not
//      guaranteed to exist (it must run the orchestrator, not `trunk serve`
//      directly).
//   4. devUrl's port is the port Trunk actually serves (Trunk.toml).
//   5. CI runs that same builder — so "CI → build-dist.sh, Tauri → trunk build"
//      cannot come back unnoticed.
//
// JavaScript, not TypeScript: it must run in the Web/contracts lane before
// node_modules exists (see tools/check-runtime-artifacts.mjs for the same
// reasoning).

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

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
const buildDirect = beforeBuild.includes(CANONICAL_BUILDER);
if (!buildIndirect && !buildDirect) {
  fail(
    `tauri.conf.json build.beforeBuildCommand is ${JSON.stringify(beforeBuild)} — ` +
      `it must run the canonical multi-artifact build (npm run build:dist, i.e. ` +
      `${CANONICAL_BUILDER}). Tauri must not build the frontend by another path.`,
  );
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

// 3. devUrl's port is Trunk's port. Trunk.toml owns the number; a divergence
// means Tauri waits on a URL nothing serves.
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

// 5. Neither Tauri command may assemble the frontend by hand. A `cp` in the
// config is a second, unreviewed build path — the shape the merge belongs to
// (tools/build-dist.sh) rather than to the config.
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

// 6. The native smoke is what makes "the window opened and showed nothing" a
// red run; it must exist and be wired into a workflow.
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

// 7. The lane that runs the smoke must not skip the paths the boot depends
// on: a change to the shell, the Tauri config or the build script has to
// reach it.
const requiredPaths = ["src/**", "src-tauri/**", "index.html", CANONICAL_BUILDER];
for (const required of requiredPaths) {
  if (!deepText.includes(`"${required}"`)) {
    fail(
      `.github/workflows/deep-ci.yml's push paths do not include ${required} — a ` +
        `boot-affecting change could skip the lane that tests it`,
    );
  }
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
