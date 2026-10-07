// Dev boot: build the artifacts, serve the shell, prove they are all served.

import { spawn } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import net from "node:net";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { RUNTIME_INPUTS, RUNTIME_FILES, mergeRuntimeArtifacts } from "./runtime-artifacts.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const DIST = path.join(root, "dist");

/** Where the freshness manifest lives: outside dist/, which Trunk owns. */
const MANIFEST_DIR = path.join(root, ".dev-artifacts");
const MANIFEST_PATH = path.join(MANIFEST_DIR, "artifact-manifest.json");

/** Inputs that invalidate a complete production distribution. */
const RELEASE_FINGERPRINT_ROOTS = [
  "src",
  "crates",
  "public",
  "styles",
  "tools",
  "index.html",
  "Trunk.toml",
  ...RUNTIME_INPUTS,
  "Cargo.toml",
  "Cargo.lock",
  "package.json",
  "package-lock.json",
  "tsconfig.json",
  "tsconfig.tools.json",
];

/** Trunk owns shell, CSS and public assets. */
const DEV_FINGERPRINT_ROOTS = [
  "crates",
  "tools",
  "Trunk.toml",
  ...RUNTIME_INPUTS,
  "Cargo.toml",
  "Cargo.lock",
  "package.json",
  "package-lock.json",
  "tsconfig.json",
  "tsconfig.tools.json",
];

/** Hook outputs inside the roots above — derived, never fingerprinted. */
const GENERATED_NAMES = new Set(["pdfEngine.js", "readerEngine.js", "rasterLane.js", "readerHost.js", "bake.worker.js", "coverBake.js"]);
const ENGINE_DIR = path.join(root, "public", "engine");

/** Production outputs; dev only reuses the route artifacts. */
const RELEASE_FRESHNESS_SET = [
  "dist/index.html",
  "dist/mareader.js",
  "dist/mareader_bg.wasm",
  "dist/tauri-relay.js",
  "dist/rasterLane.js",
  "dist/readerHost.js",
  ...RUNTIME_FILES.map((file) => `dist/${file}`),
];
const DEV_FRESHNESS_SET = RUNTIME_FILES.map((file) => `dist/${file}`);

/** The files the shell and its pane frames load, probed over HTTP. */
const PROBED = [
  "/index.html",
  "/tauri-relay.js",
  "/rasterLane.js",
  "/bake.html",
  "/coverBake.js",
  "/readerHost.js",
  ...RUNTIME_FILES.map((file) => `/${file}`),
];

/** Rust changes need all runtime artifacts; Trunk watches web assets. */
const WATCHED_ROOTS = ["crates"];
const WATCHED_FILES = ["Trunk.toml", "Cargo.toml", "Cargo.lock", ...RUNTIME_INPUTS];

/** Trunk's watch-ignore directories must exist before `trunk serve` spawns. */
const IGNORE_DIRS = [
  "src-tauri",
  "scripts",
  "target",
  "dist-library",
  "dist-reader",
  "dist-pdf",
  "dist-reflow",
  "node_modules",
  ".dev-artifacts",
];

const POLL_MS = 1000;
const SERVE_START_TIMEOUT_MS = 180_000;
const PROBE_TIMEOUT_MS = 5_000;
/** Quiet window before a rebuild: `trunk serve` swaps dist/ at build end. */
const DIST_QUIET_MS = 1_200;
const DIST_QUIET_TIMEOUT_MS = 10_000;
const REBUILD_ATTEMPTS = 3;
const REBUILD_RETRY_MS = 2_500;
const PORT_RELEASE_TIMEOUT_MS = 3_500;

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const log = (msg) => console.log(`[dev] ${msg}`);

function readText(rel) {
  return fs.readFileSync(path.join(root, rel), "utf8");
}

/** The dev URL is the config's, the port Trunk's: read both. */
function devUrl() {
  const conf = JSON.parse(readText("src-tauri/tauri.conf.json"));
  if (!conf.build?.devUrl) throw new Error("tauri.conf.json has no build.devUrl");
  return conf.build.devUrl;
}

function devPort() {
  const url = new URL(devUrl());
  return Number(url.port || (url.protocol === "https:" ? 443 : 80));
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

/** Capture stdout; rejects on spawn failure or a non-zero exit. */
function capture(command, args) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { cwd: root });
    let out = "";
    child.stdout?.on("data", (d) => (out += d));
    child.stderr?.resume();
    child.on("error", reject);
    child.on("exit", (code) => (code === 0 ? resolve(out) : reject(new Error(`${command} exit ${code}`))));
  });
}

/** Is anything accepting connections on the dev port right now? */
function portIsFree(port) {
  return new Promise((resolve) => {
    const sock = net.connect({ port, host: "127.0.0.1" });
    const done = (free) => {
      sock.removeAllListeners();
      sock.destroy();
      resolve(free);
    };
    sock.setTimeout(750, () => done(true));
    sock.once("connect", () => done(false));
    sock.once("error", () => done(true));
  });
}

/** Clear a trunk orphan off the port; other pids are left alone. */
async function releaseStaleTrunk(port) {
  let pids = [];
  try {
    const out = await capture("lsof", ["-nP", `-iTCP:${port}`, "-sTCP:LISTEN", "-t"]);
    pids = out.split("\n").map((s) => s.trim()).filter(Boolean);
  } catch {
    return false;
  }
  let signalled = false;
  for (const pid of pids) {
    let comm = "";
    try {
      comm = (await capture("ps", ["-p", pid, "-o", "comm="])).trim();
    } catch {
      continue;
    }
    if (!/trunk/i.test(path.basename(comm))) continue;
    try {
      process.kill(Number(pid), "SIGKILL");
      signalled = true;
      log(`released an orphaned trunk serve (pid ${pid}) from an earlier session`);
    } catch {
      /* already gone */
    }
  }
  return signalled;
}

/** Clear the port first: a stale server would make every probe lie. */
async function ensureDevPortFree() {
  const port = devPort();
  if (await portIsFree(port)) return true;
  log(`port ${port} is busy — checking for a trunk serve orphaned by an earlier session`);
  await releaseStaleTrunk(port);
  const deadline = Date.now() + PORT_RELEASE_TIMEOUT_MS;
  while (Date.now() < deadline) {
    if (await portIsFree(port)) {
      log(`port ${port} is free`);
      return true;
    }
    await sleep(250);
  }
  console.error(
    `[dev] port ${port} is still in use by something that is not trunk — ` +
      `the dev server cannot start. Find it with ` +
      `\`lsof -nP -iTCP:${port} -sTCP:LISTEN\`, stop that process, and restart.`,
  );
  return false;
}

/** Capture output; failures print only after every attempt fails. */
function runCaptured(command, args) {
  return new Promise((resolve) => {
    const child = spawn(command, args, { cwd: root });
    let out = "";
    child.stdout?.on("data", (d) => (out += d));
    child.stderr?.on("data", (d) => (out += d));
    child.on("error", (e) => resolve({ code: 127, out: `${out}\n[dev] ${command}: ${e.message}` }));
    child.on("exit", (code, signal) => resolve({ code: signal ? 1 : (code ?? 1), out }));
  });
}

function profileArgs(profile) {
  if (profile === "release") return ["--release"];
  if (profile === "dev") return [];
  throw new Error(`unsupported frontend profile: ${profile}`);
}

function developmentProfile() {
  const profile = process.env.MAREADER_DEV_PROFILE ?? "dev";
  if (profile !== "dev" && profile !== "release") {
    throw new Error("MAREADER_DEV_PROFILE must be `dev` or `release`");
  }
  return profile;
}

/** Run the canonical build; the caller decides whether a failure is fatal. */
async function runBuildAll(profile) {
  log(`building all five artifacts (${profile} profile)`);
  return run("sh", ["tools/build-dist.sh", ...profileArgs(profile)]);
}

async function buildAllOrExit(profile) {
  const code = await runBuildAll(profile);
  if (code !== 0) {
    console.error(`[dev] the canonical build failed (exit ${code}) — not starting the shell`);
    process.exit(code);
  }
}

/** The (path, mtime, size) fingerprint of each build profile's inputs. */
function sourceFingerprint(profile) {
  const roots = profile === "release" ? RELEASE_FINGERPRINT_ROOTS : DEV_FINGERPRINT_ROOTS;
  const hash = crypto.createHash("sha256");
  hash.update(`profile:${profile};builder:build-dist.sh`);
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
  for (const rel of roots) {
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

function artifactsPresent(profile) {
  const files = profile === "release" ? RELEASE_FRESHNESS_SET : DEV_FRESHNESS_SET;
  return files.every((rel) => {
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

function writeManifest(profile, fingerprint) {
  fs.mkdirSync(MANIFEST_DIR, { recursive: true });
  fs.writeFileSync(
    MANIFEST_PATH,
    `${JSON.stringify({ profile, fingerprint }, null, 2)}\n`,
  );
}

/** Re-copy the merged runtime artifacts if a shell rebuild removed them. */
function ensureMergedArtifacts() {
  const restored = mergeRuntimeArtifacts(DIST, { required: false, onlyMissing: true });
  if (restored.length) log(`restored merged runtime artifacts: ${restored.join(", ")}`);
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

/** Probe every artifact the shell loads; failures as `path -> status`. */
async function probeArtifacts() {
  const failures = [];
  for (const p of PROBED) {
    const status = await fetchStatus(p);
    if (status !== 200) failures.push([p, status]);
  }
  return failures;
}

/** Wait for the dev server, failing fast if the serve process exits. */
async function waitForServe(dead) {
  const deadline = Date.now() + SERVE_START_TIMEOUT_MS;
  for (;;) {
    if (dead()) return false;
    const status = await fetchStatus("/index.html");
    if (status === 200) return true;
    if (Date.now() > deadline) return false;
    await sleep(500);
  }
}

/** The gate: the dev server must serve index.html and all pane artifacts. */
async function proveServed(label) {
  for (let attempt = 1; attempt <= 3; attempt += 1) {
    const failures = await probeArtifacts();
    if (failures.length === 0) {
      log(`${label}: dev server serves index.html + the pane pages, scripts and wasm modules`);
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

/** Newest mtime across runtime sources; Trunk owns shell, CSS and public assets. */
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

/** Newest mtime across `dist/.stage` and `target/wasm-opt`, or null when
 *  neither exists. These are the two trees a Trunk build writes LATE —
 *  staging fills as pipelines finish, wasm-opt runs just before the swap —
 *  so they are the signals that a build is genuinely in flight. (`prepare_*
 *  deletes `dist/.stage` at build start and the apply removes it after, so
 *  an absent tree means idle, not mid-swap.) */
function lateBuildActivity() {
  let newest = null;
  for (const rel of ["dist/.stage", "target/wasm-opt"]) {
    const visit = (abs) => {
      let entries;
      try {
        entries = fs.readdirSync(abs, { withFileTypes: true });
      } catch {
        return;
      }
      for (const entry of entries) {
        const child = path.join(abs, entry.name);
        try {
          const mtime = fs.statSync(child).mtimeMs;
          if (newest === null || mtime > newest) newest = mtime;
        } catch {
          /* vanished mid-walk */
        }
        if (entry.isDirectory()) visit(child);
      }
    };
    visit(path.join(root, rel));
  }
  return newest;
}

/** Wait until `trunk serve`'s outputs sit still and no build is running. */
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
    const activity = lateBuildActivity();
    if (activity !== null) newest = newest === null ? activity : Math.max(newest, activity);
    if (newest !== null && Date.now() - newest >= DIST_QUIET_MS) return;
    if (Date.now() >= deadline) return;
    await sleep(250);
  }
}

/** Builds racing `trunk serve` are retried; failures surface at the end. */
async function rebuildRuntimeArtifacts(profile) {
  await waitForDistQuiet();
  let last = { code: 1, out: "" };
  for (let attempt = 1; attempt <= REBUILD_ATTEMPTS; attempt += 1) {
    last = await runCaptured("sh", ["tools/build-dist.sh", ...profileArgs(profile)]);
    if (last.code === 0) {
      if (attempt > 1) log(`rebuild succeeded on attempt ${attempt}`);
      return true;
    }
    console.error(`[dev] rebuild attempt ${attempt}/${REBUILD_ATTEMPTS} exited ${last.code} — retrying`);
    if (attempt < REBUILD_ATTEMPTS) {
      await sleep(REBUILD_RETRY_MS);
      await waitForDistQuiet();
    }
  }
  console.error(
    `[dev] the canonical build failed after ${REBUILD_ATTEMPTS} attempts — keeping ` +
      "the dev server up on the existing artifacts; the next source change retries.\n" +
      last.out,
  );
  return false;
}

function ensureIgnoreDirs() {
  for (const rel of IGNORE_DIRS) {
    fs.mkdirSync(path.join(root, rel), { recursive: true });
  }
}

/** The freshness decision: build only when the manifest does not vouch. */
function freshnessDecision(profile) {
  const force = process.env.FORCE_REBUILD === "1";
  const fingerprint = sourceFingerprint(profile);
  if (force) return { fresh: false, fingerprint, why: "FORCE_REBUILD=1" };
  if (!artifactsPresent(profile))
    return { fresh: false, fingerprint, why: "artifacts are missing" };
  const manifest = readManifest();
  if (manifest === null) return { fresh: false, fingerprint, why: "no manifest yet" };
  if (manifest.profile !== profile || manifest.fingerprint !== fingerprint)
    return { fresh: false, fingerprint, why: "inputs changed since the last build" };
  return { fresh: true, fingerprint, why: "inputs unchanged" };
}

/** `--build-only`: the same gate with no server, run by beforeBuildCommand. */
async function buildOnly() {
  const profile = "release";
  const decision = freshnessDecision(profile);
  if (decision.fresh) {
    log(`build-only: ${decision.why} — skipping the five-target build`);
    return;
  }
  log(`build-only: ${decision.why} — running the five-target build`);
  await buildAllOrExit(profile);
  writeManifest(profile, decision.fingerprint);
  log("build-only: artifacts built and manifest written");
}

async function main() {
  const profile = developmentProfile();

  // The port must be ours before the first build touches dist/, not the spawn.
  if (!(await ensureDevPortFree())) process.exit(1);

  // The port is ours: every build from here is dist/'s one writer.
  process.env.MAREADER_DEV_BUILD = "1";

  // The freshness gate: a warm restart pays no build when the manifest vouches.
  const decision = freshnessDecision(profile);
  if (decision.fresh) {
    log(
      `freshness gate: ${decision.why} — skipping the five-target build ` +
        "(FORCE_REBUILD=1 to rebuild)",
    );
  } else {
    log(`freshness gate: ${decision.why} — building all five artifacts`);
    await buildAllOrExit(profile);
    writeManifest(profile, decision.fingerprint);
  }

  // Trunk hard-errors on a missing watch-ignore entry; create them first.
  ensureIgnoreDirs();

  // One orchestrator, one trunk, one server; `stop` is the only exit.
  let serve = null;
  let serveExited = false;
  const stop = (code) => {
    if (serve && !serveExited) {
      try {
        serve.kill("SIGTERM");
      } catch {
        /* already gone */
      }
    }
    process.exit(code);
  };
  process.on("SIGINT", () => stop(0));
  process.on("SIGTERM", () => stop(0));
  // A closed terminal hangs up: same cleanup, so trunk cannot outlive it.
  process.on("SIGHUP", () => stop(0));
  process.on("exit", () => {
    if (serve && !serveExited) {
      try {
        serve.kill("SIGTERM");
      } catch {
        /* already gone */
      }
    }
  });

  if (!(await ensureDevPortFree())) process.exit(1);

  log(`starting trunk serve (the shell dev server, ${profile} profile)`);
  // --enable-cooldown drops in-build events; without it, endless rebuilds.
  const serveArgs = ["serve", ...profileArgs(profile), "--enable-cooldown"];
  serve = spawn("trunk", serveArgs, {
    cwd: root,
    stdio: "inherit",
  });
  serve.on("error", (e) => {
    serveExited = true;
    console.error(`[dev] cannot run trunk: ${e.message}`);
  });
  serve.on("exit", (code, signal) => {
    serveExited = true;
    log(`trunk serve exited (${signal ?? code})`);
  });

  if (!(await waitForServe(() => serveExited))) {
    if (serveExited) {
      console.error(
        "[dev] trunk serve exited before the dev server came up — a window " +
          "opened now would have nothing to load. A trunk orphaned by an " +
          "earlier session is the usual cause; see the trunk output above.",
      );
      stop(1);
    }
    console.error(`[dev] trunk serve never answered on ${devUrl()}/index.html`);
    stop(1);
  }
  if (!(await proveServed("boot"))) stop(1);

  log(`safe to open ${devUrl()} — the shell will find its runtimes`);
  log("watching runtime crate and entry-point inputs for changes");

  let watermark = newestMtime();
  for (;;) {
    await sleep(POLL_MS);
    if (serveExited) {
      console.error(
        "[dev] trunk serve died mid-session — the dev server is gone. " +
          "Restart tauri dev; if the port was stolen, this orchestrator " +
          "clears a stale trunk on the next start.",
      );
      process.exit(1);
    }
    ensureMergedArtifacts();
    const now = newestMtime();
    if (now === watermark) continue;
    watermark = now;
    // A watched source changed: the runtime artifacts are stale until rebuilt.
    log("source change detected — rebuilding the runtime artifacts");
    if (!(await rebuildRuntimeArtifacts(profile))) continue;
    writeManifest(profile, sourceFingerprint(profile));
    if (!(await proveServed("rebuild"))) stop(1);
  }
}

if (process.argv.includes("--build-only")) {
  // beforeBuildCommand mode: gate + build, no server, no watch.
  buildOnly().catch((e) => {
    console.error(`[dev] ${e.stack ?? e.message}`);
    process.exit(1);
  });
} else {
  main().catch((e) => {
    console.error(`[dev] ${e.stack ?? e.message}`);
    process.exit(1);
  });
}
