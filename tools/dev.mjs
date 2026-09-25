// The development boot orchestrator: build the runtime artifacts, serve the
// shell, and PROVE that the dev server serves all three artifacts before a
// window can load them.
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
//   1. build all three artifacts (tools/build-dist.sh — the canonical build)
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
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const DIST = path.join(root, "dist");

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
  ["dist-reader/reader.html", "reader.html"],
  ["dist-reader/reader.js", "reader.js"],
  ["dist-reader/reader_bg.wasm", "reader_bg.wasm"],
  ["dist-library/library.html", "library.html"],
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

const POLL_MS = 1000;
const SERVE_START_TIMEOUT_MS = 180_000;
const PROBE_TIMEOUT_MS = 5_000;

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

async function buildAll() {
  log("building all three artifacts (tools/build-dist.sh)");
  const code = await run("sh", ["tools/build-dist.sh"]);
  if (code !== 0) {
    console.error(`[dev] the canonical build failed (exit ${code}) — not starting the shell`);
    process.exit(code);
  }
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
 *  this runs once a second over a small tree. */
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

async function main() {
  await buildAll();

  log("starting trunk serve (the shell dev server)");
  const serve = spawn("trunk", ["serve"], { cwd: root, stdio: "inherit" });
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
    await buildAll();
    if (!(await proveServed("rebuild"))) shutdown();
  }
}

main().catch((e) => {
  console.error(`[dev] ${e.stack ?? e.message}`);
  process.exit(1);
});
