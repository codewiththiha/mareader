// The development boot orchestrator: build the runtime artifacts, serve the
// shell, and PROVE that the dev server serves all three artifacts before a
// window can load them.
//
// Everything builds in RELEASE. Every route transition mounts a fresh frame
// that instantiates its own wasm module, and a debug (unoptimized) module
// makes that boot take tens of seconds — a stall CI never sees because it
// only ever runs the release profile. Dev runs the same artifacts CI and the
// packaged app prove, so a route costs what a route costs in production.
//
// Why this exists (`tauri.conf.json` build.beforeDevCommand): the old command
// was `npm run build:ts && trunk serve`, which serves whatever `dist/` happens
// to hold. `trunk serve` builds the SHELL page only — `reader.js` and
// `library.js` come from the other two Trunk invocations, merged by
// `tools/build-dist.sh`. So on a fresh clone (or after `trunk clean`), every
// dynamic import the shell makes resolved to a 404, and the app opened to an
// empty runtime host: the blank window, in dev, with nothing in the terminal.
//
// The invariant this establishes, in the order the shell needs it:
//
//   1. build all three artifacts (tools/build-dist.sh --release — the
//      canonical build, in the profile the rest of the pipeline runs),
//      SKIPPED when .dev-artifacts/release-manifest.json vouches the inputs
//      are unchanged and the artifacts exist (FORCE_REBUILD=1 to override)
//   2. assert the artifact contract (the builder runs the checker itself)
//   3. start `trunk serve`
//   4. probe the dev URL for index.html + both runtime artifacts + both wasm
//      modules, and only then report the boot as safe
//   5. keep the merged artifacts in place across shell rebuilds — Trunk owns
//      `dist/`, and a shell rebuild must not be able to drop the runtimes
//
// If a probe fails the orchestrator exits non-zero with the missing path. The
// shell must never be the thing that discovers a missing runtime: it can only
// report the failure to the user, and by then the window is already open.
//
// Run directly (`node tools/dev.mjs`) or through `npm run dev:frontend`, which
// is what tauri.conf.json's beforeDevCommand calls.

import { spawn } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const DIST = path.join(root, "dist");

/** Where the freshness manifest lives: OUTSIDE dist/, because Trunk owns
 *  dist/ and the decision has to survive its rebuilds. Gitignored. */
const MANIFEST_DIR = path.join(root, ".dev-artifacts");
const MANIFEST_PATH = path.join(MANIFEST_DIR, "release-manifest.json");

/** Everything whose change invalidates the built artifact set. Derived
 *  outputs are excluded on purpose (see GENERATED below): their sources are
 *  in these roots, and re-hashing what the build itself just rewrote would
 *  mark every fresh build stale. */
const FINGERPRINT_ROOTS = [
  "src",
  "crates",
  "public",
  "styles",
  "tools",
  "index.html",
  "reader.html",
  "library.html",
  "Trunk.toml",
  "reader.Trunk.toml",
  "library.Trunk.toml",
  "Cargo.toml",
  "Cargo.lock",
  "package.json",
  "package-lock.json",
  "tsconfig.json",
  "tsconfig.tools.json",
];

/** Hook outputs inside the roots above — derived, never fingerprinted. */
const GENERATED_NAMES = new Set(["pdfEngine.js", "readerEngine.js", "bake.worker.js"]);
const ENGINE_DIR = path.join(root, "public", "engine");

/** The floor a fresh manifest vouches for; proveServed still verifies the
 *  full promised set over HTTP before any window is allowed to load. */
const FRESHNESS_SET = [
  "dist/index.html",
  "dist/mareader.js",
  "dist/mareader_bg.wasm",
  "dist/library.html",
  "dist/library.js",
  "dist/library_bg.wasm",
  "dist/reader.html",
  "dist/reader.js",
  "dist/reader_bg.wasm",
];

/** The files the shell imports at boot, in boot order. Probed over HTTP so the
 *  check is about what the DEV SERVER serves, not what the disk holds. */
const PROBED = [
  "/index.html",
  "/library.js",
  "/library_bg.wasm",
  "/reader.js",
  "/reader_bg.wasm",
];

/** What the merged artifacts are, and where the other two Trunk builds leave
 *  them. Re-copied after shell rebuilds: Trunk owns `dist/` while serving. */
const MERGED = [
  // The page first: Trunk names the built page after the target it built
  // (reader.html) or normalizes it to index.html, so both are candidates.
  ["dist-reader/reader.html", "reader.html"],
  ["dist-reader/index.html", "reader.html"],
  ["dist-reader/reader.js", "reader.js"],
  ["dist-reader/reader_bg.wasm", "reader_bg.wasm"],
  ["dist-library/library.html", "library.html"],
  ["dist-library/index.html", "library.html"],
  ["dist-library/library.js", "library.js"],
  ["dist-library/library_bg.wasm", "library_bg.wasm"],
];

/** Source trees whose changes require a rebuild of the runtime artifacts. The
 *  runtimes share most of the crates, so any of them can change an artifact;
 *  `styles/` and `public/` land in `dist/` through the shell build. */
const WATCHED_ROOTS = ["crates", "styles", "public"];
const WATCHED_FILES = [
  "index.html",
  "reader.html",
  "library.html",
  "Trunk.toml",
  "reader.Trunk.toml",
  "library.Trunk.toml",
];

/** Directory entries in Trunk.toml's `[watch] ignore` list. Trunk 0.21.x
 *  canonicalizes every entry at startup and a missing path is a hard error,
 *  so the directories must exist before `trunk serve` spawns — the freshness
 *  gate can skip the build that would normally have created them (a gate
 *  restart after `cargo clean`, for instance). File entries are all build
 *  outputs, which the gate's own artifact check already covers. */
const IGNORE_DIRS = [
  "src-tauri",
  "scripts",
  "styles",
  "target",
  "dist-reader",
  "dist-library",
  "node_modules",
  ".dev-artifacts",
];

const POLL_MS = 1000;
const SERVE_START_TIMEOUT_MS = 180_000;
const PROBE_TIMEOUT_MS = 5_000;
/** Quiet window before a watcher rebuild: `trunk serve` swaps its
 *  distribution at the END of a build, and a concurrent canonical build
 *  copying wasm into that `dist/` fails with ENOENT mid-swap. */
const DIST_QUIET_MS = 1_200;
const DIST_QUIET_TIMEOUT_MS = 10_000;
const REBUILD_ATTEMPTS = 3;
const REBUILD_RETRY_MS = 2_500;

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const log = (msg) => console.log(`[dev] ${msg}`);

function readText(rel) {
  return fs.readFileSync(path.join(root, rel), "utf8");
}

/** The dev URL Tauri waits on is the config's; the port it must match is
 *  Trunk's. Reading both here keeps the orchestrator from inventing a third
 *  number (`tools/check-tauri-contract.mjs` asserts they agree at rest). */
function devUrl() {
  const conf = JSON.parse(readText("src-tauri/tauri.conf.json"));
  return conf.build?.devUrl ?? "http://localhost:1420";
}

/** Run a command to completion, inheriting stdio. Returns the exit code. */
function run(command, args, options = {}) {
  return new Promise((resolve) => {
    const child = spawn(command, args, { cwd: root, stdio: "inherit", ...options });
    child.on("error", (e) => {
      console.error(`[dev] cannot run ${command}: ${e.message}`);
      resolve(127);
    });
    child.on("exit", (code, signal) => resolve(signal ? 1 : (code ?? 1)));
  });
}

/** Run the canonical build. Returns the exit code — the CALLER decides
 *  whether a failure is fatal (startup: yes; the watch loop: no, it retries
 *  and stays up so a transient dist/ race never takes the dev server down). */
async function runBuildAll() {
  log("building all three artifacts (tools/build-dist.sh --release)");
  return run("sh", ["tools/build-dist.sh", "--release"]);
}

async function buildAllOrExit() {
  const code = await runBuildAll();
  if (code !== 0) {
    console.error(`[dev] the canonical build failed (exit ${code}) — not starting the shell`);
    process.exit(code);
  }
}

/** The (path, mtime, size) fingerprint of every build input: cheap, and
 *  moved by exactly the events that invalidate artifacts — edits and git
 *  checkouts — while staying stable across restarts of this script. */
function sourceFingerprint() {
  const hash = crypto.createHash("sha256");
  hash.update("profile:release;builder:build-dist.sh");
  const visit = (abs) => {
    let entries;
    try {
      entries = fs.readdirSync(abs, { withFileTypes: true });
    } catch {
      return;
    }
    entries.sort((a, b) => (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
    for (const entry of entries) {
      if (entry.name === "target" || entry.name === "node_modules" || entry.name === ".git") {
        continue;
      }
      if (GENERATED_NAMES.has(entry.name)) continue;
      if (abs === ENGINE_DIR && entry.name.endsWith(".js")) continue;
      const child = path.join(abs, entry.name);
      if (entry.isDirectory()) {
        visit(child);
        continue;
      }
      try {
        const stat = fs.statSync(child);
        hash.update(child.slice(root.length));
        hash.update(` ${stat.mtimeMs} ${stat.size} `);
      } catch {
        /* vanished mid-walk: the next start fingerprints the new state */
      }
    }
  };
  for (const rel of FINGERPRINT_ROOTS) {
    hash.update(rel);
    let stat;
    try {
      stat = fs.statSync(path.join(root, rel));
    } catch {
      continue;
    }
    if (stat.isDirectory()) {
      visit(path.join(root, rel));
    } else {
      hash.update(` ${stat.mtimeMs} ${stat.size} `);
    }
  }
  return hash.digest("hex");
}

function artifactsPresent() {
  return FRESHNESS_SET.every((rel) => {
    try {
      return fs.statSync(path.join(root, rel)).size > 0;
    } catch {
      return false;
    }
  });
}

function readManifest() {
  try {
    return JSON.parse(fs.readFileSync(MANIFEST_PATH, "utf8"));
  } catch {
    return null;
  }
}

function writeManifest(fingerprint) {
  fs.mkdirSync(MANIFEST_DIR, { recursive: true });
  fs.writeFileSync(
    MANIFEST_PATH,
    `${JSON.stringify({ profile: "release", fingerprint }, null, 2)}\n`,
  );
}

/** Re-copy the merged runtime artifacts if a shell rebuild removed them.
 *  Trunk owns `dist/` while it serves; the runtimes are merged in from the
 *  other two builds and are not Trunk's to keep. */
function ensureMergedArtifacts() {
  const restored = [];
  for (const [from, to] of MERGED) {
    const src = path.join(root, from);
    const dst = path.join(DIST, to);
    if (fs.existsSync(dst) || !fs.existsSync(src)) continue;
    fs.copyFileSync(src, dst);
    restored.push(to);
  }
  if (restored.length > 0) {
    log(`restored merged runtime artifacts after a shell rebuild: ${restored.join(", ")}`);
  }
  return restored.length > 0;
}

async function fetchStatus(urlPath) {
  const controller = new AbortController();
  const timer = setTimeout(() => controller.abort(), PROBE_TIMEOUT_MS);
  try {
    const res = await fetch(new URL(urlPath, devUrl()), { signal: controller.signal });
    // Drain the body so the connection is released; the status is the fact.
    await res.arrayBuffer().catch(() => {});
    return res.status;
  } catch {
    return 0;
  } finally {
    clearTimeout(timer);
  }
}

/** Probe every artifact the shell loads. Returns the list of failures as
 *  `path -> status` (0 = the server did not answer). */
async function probeArtifacts() {
  const failures = [];
  for (const p of PROBED) {
    const status = await fetchStatus(p);
    if (status !== 200) failures.push([p, status]);
  }
  return failures;
}

async function waitForServe() {
  const deadline = Date.now() + SERVE_START_TIMEOUT_MS;
  for (;;) {
    const status = await fetchStatus("/index.html");
    if (status === 200) return true;
    if (Date.now() > deadline) return false;
    await sleep(500);
  }
}

/** The gate: the dev server must serve index.html AND both runtime artifacts
 *  AND both wasm modules before any window is allowed to load the shell. */
async function proveServed(label) {
  for (let attempt = 1; attempt <= 3; attempt += 1) {
    const failures = await probeArtifacts();
    if (failures.length === 0) {
      log(`${label}: dev server serves index.html + library.js + reader.js + both wasm modules`);
      return true;
    }
    if (attempt === 1) ensureMergedArtifacts();
    await sleep(500);
  }
  const failures = await probeArtifacts();
  console.error(`[dev] ${label}: the dev server does not serve every runtime artifact:`);
  for (const [p, status] of failures) {
    console.error(
      `  - ${p}: ${status === 0 ? "no response" : `HTTP ${status}`}` +
        (status === 404 ? "  ← the shell's dynamic import would fail here" : ""),
    );
  }
  console.error(
    "[dev] the shell is NOT safe to boot: it imports these at start. " +
      "Re-run `npm run build:dist`, then `npm run dev:frontend`.",
  );
  return false;
}

/** A cheap, deterministic change detector: newest mtime across the watched
 *  trees. fs.watch is platform-dependent (recursive on some, not others) and
 *  this runs once a second over a small tree. Hook outputs under `public/`
 *  are EXCLUDED for the same reason the fingerprint excludes them: every
 *  Trunk build rewrites them, so counting them would fire a canonical rebuild
 *  off Trunk's own churn instead of a real source edit. */
function newestMtime() {
  let newest = 0;
  const visit = (abs) => {
    let entries;
    try {
      entries = fs.readdirSync(abs, { withFileTypes: true });
    } catch {
      return;
    }
    for (const entry of entries) {
      if (entry.name === "target" || entry.name === "node_modules" || entry.name === ".git") continue;
      if (GENERATED_NAMES.has(entry.name)) continue;
      if (abs === ENGINE_DIR && entry.name.endsWith(".js")) continue;
      const child = path.join(abs, entry.name);
      if (entry.isDirectory()) {
        visit(child);
      } else {
        try {
          const mtime = fs.statSync(child).mtimeMs;
          if (mtime > newest) newest = mtime;
        } catch {
          /* a file that vanished mid-walk is not a change */
        }
      }
    }
  };
  for (const rel of WATCHED_ROOTS) visit(path.join(root, rel));
  for (const rel of WATCHED_FILES) {
    try {
      const mtime = fs.statSync(path.join(root, rel)).mtimeMs;
      if (mtime > newest) newest = mtime;
    } catch {
      /* absent file: nothing to watch */
    }
  }
  return newest;
}

/** Wait until `trunk serve`'s own outputs have sat still for DIST_QUIET_MS.
 *  A missing shell output counts as NOT quiet — that is the dist/ swap
 *  window, exactly when a concurrent build must not start. */
async function waitForDistQuiet() {
  const deadline = Date.now() + DIST_QUIET_TIMEOUT_MS;
  for (;;) {
    let newest = null;
    for (const rel of ["dist/index.html", "dist/mareader.js", "dist/mareader_bg.wasm"]) {
      try {
        const mtime = fs.statSync(path.join(root, rel)).mtimeMs;
        newest = newest === null ? mtime : Math.max(newest, mtime);
      } catch {
        newest = null;
        break;
      }
    }
    if (newest !== null && Date.now() - newest >= DIST_QUIET_MS) return;
    if (Date.now() >= deadline) return;
    await sleep(250);
  }
}

/** Canonical builds spawned while `trunk serve` is applying its own
 *  distribution race it: wasm-opt copies into a `dist/` that just ceased to
 *  exist, the build fails, and a fatal handler used to take the whole dev
 *  session with it. Wait for quiet, retry the race window, and if it still
 *  fails keep the server up — the next source change tries again. */
async function rebuildRuntimeArtifacts() {
  await waitForDistQuiet();
  let code = 1;
  for (let attempt = 1; attempt <= REBUILD_ATTEMPTS; attempt += 1) {
    code = await runBuildAll();
    if (code === 0) return true;
    console.error(`[dev] rebuild failed (exit ${code}), attempt ${attempt}/${REBUILD_ATTEMPTS}`);
    if (attempt < REBUILD_ATTEMPTS) await sleep(REBUILD_RETRY_MS);
  }
  console.error(
    "[dev] the canonical build is still failing — keeping the dev server up " +
      "on the existing artifacts; the next source change retries",
  );
  return false;
}

function ensureIgnoreDirs() {
  for (const rel of IGNORE_DIRS) {
    fs.mkdirSync(path.join(root, rel), { recursive: true });
  }
}

async function main() {
  // The freshness gate: a warm restart pays NO three-target build. The
  // manifest vouches the inputs are unchanged (and the artifacts exist);
  // FORCE_REBUILD=1 overrides when a build itself is suspect.
  const force = process.env.FORCE_REBUILD === "1";
  const fingerprint = sourceFingerprint();
  const manifest = readManifest();
  const fresh =
    !force &&
    artifactsPresent() &&
    manifest !== null &&
    manifest.profile === "release" &&
    manifest.fingerprint === fingerprint;
  if (fresh) {
    log(
      "release artifacts are fresh — skipping the three-target build " +
        "(FORCE_REBUILD=1 to rebuild)",
    );
  } else {
    await buildAllOrExit();
    writeManifest(fingerprint);
  }

  // Trunk hard-errors on a watch-ignore entry that does not exist yet; the
  // build above normally creates these, but a gate-skipped start may not
  // have run one.
  ensureIgnoreDirs();

  log("starting trunk serve (the shell dev server, release profile)");
  // --enable-cooldown: discard filesystem events that land during a build.
  // Trunk's default is off, and with it off every write cargo makes inside
  // `target/` queues another build — an endless serve loop.
  const serve = spawn("trunk", ["serve", "--release", "--enable-cooldown"], {
    cwd: root,
    stdio: "inherit",
  });
  let serveExited = false;
  serve.on("error", (e) => {
    serveExited = true;
    console.error(`[dev] cannot run trunk: ${e.message}`);
  });
  serve.on("exit", (code) => {
    serveExited = true;
    log(`trunk serve exited (${code ?? "signal"})`);
  });

  const shutdown = () => {
    if (!serveExited) serve.kill("SIGTERM");
    process.exit(0);
  };
  process.on("SIGINT", shutdown);
  process.on("SIGTERM", shutdown);

  if (!(await waitForServe())) {
    console.error(`[dev] trunk serve never answered on ${devUrl()}/index.html`);
    shutdown();
  }
  const proven = await proveServed("boot");
  if (!proven) shutdown();

  log(`safe to open ${devUrl()} — the shell will find its runtimes`);
  log("watching crates/, styles/, public/ for runtime changes");

  let watermark = newestMtime();
  for (;;) {
    await sleep(POLL_MS);
    if (serveExited) process.exit(0);
    ensureMergedArtifacts();
    const now = newestMtime();
    if (now === watermark) continue;
    watermark = now;
    // A watched source changed: the runtime artifacts are stale until the
    // canonical build runs again. Trunk rebuilds the shell on its own.
    log("source change detected — rebuilding the runtime artifacts");
    if (!(await rebuildRuntimeArtifacts())) continue;
    writeManifest(sourceFingerprint());
    if (!(await proveServed("rebuild"))) shutdown();
  }
}

main().catch((e) => {
  console.error(`[dev] ${e.stack ?? e.message}`);
  process.exit(1);
});
