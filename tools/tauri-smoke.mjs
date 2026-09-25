// The native boot smoke (§14): launch the packaged app for real and prove it
// put a RUNTIME on screen — not just a window.
//
// Why this is a separate gate from every other one: the browser suite drives
// the same `dist/` through a static server, and CI builds it with the
// canonical build script. The failure that shipped a blank window lived in
// neither of those — `tauri.conf.json`'s build command produced a shell page
// with no runtime artifacts, so the packaged app could open a native window
// whose runtime host stayed empty and whose terminal said nothing. A compiler
// cannot see that, and neither can a check that only reads files.
//
// So this driver asks the running application three questions and fails on any
// answer other than the expected one:
//
//   1. behavioural — did the shell bring the Library runtime up? The shell
//      reports each boot transition to the native host (`boot_report`,
//      src/app/manager.rs -> src-tauri), which prints one `[mareader] boot:`
//      line. A blank window cannot print `boot: library`, because that line is
//      only emitted once the runtime's start export returned.
//   2. visual — did the window actually draw? A screenshot of the X display
//      must contain real content (colour count + luminance spread), which a
//      blank frame cannot fake.
//   3. handoff — open a document the way the OS does (a second launch, whose
//      argv the single-instance plugin forwards to the running window) and
//      require the READER runtime to come up. That is a real runtime
//      transition inside the packaged app, not a fixture.
//
// Environment: Linux with Xvfb (the runner provides DISPLAY), `xwd` for the
// screenshot and `convert` (ImageMagick) to summarise it. Both are installed
// by .github/workflows/deep-ci.yml's tauri-smoke job.
//
// Usage: node tools/tauri-smoke.mjs [--binary <path>] [--sample <pdf>]

import { spawn, spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

const args = process.argv.slice(2);
const flag = (name, fallback) => {
  const index = args.indexOf(`--${name}`);
  return index >= 0 && args[index + 1] ? args[index + 1] : fallback;
};

const BINARY = flag("binary", process.env.MAREADER_BIN ?? findBinary());
const SAMPLE = flag(
  "sample",
  path.join(root, "public/samples/Programming Pearls (2nd Edition) - Jon Bentley.pdf"),
);

/** The packaged binary, where `cargo build -p mareader-shell --release` puts
 *  it: the workspace root target dir (the crate is a workspace member), or
 *  src-tauri/target when it was built from inside that directory. */
function findBinary() {
  const candidates = [
    path.join(root, "target/release/mareader-shell"),
    path.join(root, "src-tauri/target/release/mareader-shell"),
  ];
  return candidates.find((candidate) => fs.existsSync(candidate)) ?? candidates[0];
}

const T_LIBRARY_MS = 180_000;
const T_READER_MS = 180_000;
const SETTLE_MS = 2_500;
const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

const failures = [];
const log = (message) => console.log(`[tauri-smoke] ${message}`);
const fail = (message) => {
  failures.push(message);
  console.error(`[tauri-smoke] FAIL ${message}`);
};

if (!BINARY || !fs.existsSync(BINARY)) {
  fail(`no packaged binary at ${BINARY ?? "(none found)"} — run \`cargo build --release -p mareader-shell\` first`);
  process.exit(1);
}
if (!fs.existsSync(SAMPLE)) {
  fail(`no sample document at ${SAMPLE}`);
  process.exit(1);
}
if (!process.env.DISPLAY) {
  fail("no DISPLAY: the smoke needs an X server (the workflow runs it under xvfb-run)");
  process.exit(1);
}

/** Launch the app, collecting its stdout/stderr. */
function launch(extraArgs = []) {
  const child = spawn(BINARY, extraArgs, {
    cwd: root,
    env: {
      ...process.env,
      // WebKitGTK on a software-rendered X server: the compositor needs to be
      // off, or the window never paints a frame we can screenshot.
      WEBKIT_DISABLE_COMPOSITING_MODE: "1",
      LIBGL_ALWAYS_SOFTWARE: "1",
      GDK_BACKEND: "x11",
    },
    stdio: ["ignore", "pipe", "pipe"],
  });
  const output = [];
  const collect = (chunk) => output.push(chunk.toString());
  child.stdout.on("data", collect);
  child.stderr.on("data", collect);
  return {
    child,
    output,
    text: () => output.join(""),
    /** Wait for a `[mareader] boot: <state>` line matching `pattern`. */
    async waitForBoot(pattern, timeoutMs, label) {
      const started = Date.now();
      for (;;) {
        const match = this.text()
          .split("\n")
          .filter((line) => line.includes("[mareader] boot:"))
          .find((line) => pattern.test(line));
        if (match) return match.trim();
        if (this.child.exitCode !== null) {
          fail(`${label}: the app exited (code ${this.child.exitCode}) before reporting a boot state`);
          return null;
        }
        if (Date.now() - started > timeoutMs) {
          const seen = this.text().split("\n").slice(-40).join("\n");
          fail(`${label}: no boot report matching ${pattern} within ${timeoutMs / 1000}s\n--- app output ---\n${seen}`);
          return null;
        }
        await sleep(500);
      }
    },
    /** Every boot line the app reported so far (for the report/artifacts). */
    bootLines() {
      return this.text()
        .split("\n")
        .filter((line) => line.includes("[mareader] boot:"))
        .map((line) => line.trim());
    },
  };
}

/** The window's pixels: colour count and luminance spread over a downscaled
 *  screenshot. A blank (uniform) window yields one colour and a spread of 0. */
function screenshotStats() {
  const shot = spawnSync("xwd", ["-root", "-silent"], { maxBuffer: 64 * 1024 * 1024 });
  if (shot.status !== 0 || !shot.stdout || shot.stdout.length === 0) {
    return { error: `xwd failed (status ${shot.status}): ${shot.stderr?.toString().trim()}` };
  }
  const stats = spawnSync(
    "convert",
    [
      "xwd:-",
      "-resize",
      "160x120!",
      "-colors",
      "32",
      "-format",
      "%k %[fx:100*standard_deviation]",
      "info:",
    ],
    { input: shot.stdout, maxBuffer: 16 * 1024 * 1024 },
  );
  if (stats.status !== 0) {
    return { error: `convert failed (status ${stats.status}): ${stats.stderr?.toString().trim()}` };
  }
  const [colors, deviation] = stats.stdout.toString().trim().split(/\s+/);
  return { colors: Number(colors), deviation: Number(deviation) };
}

async function kill(child) {
  if (!child || child.exitCode !== null) return;
  child.kill("SIGTERM");
  for (let waited = 0; waited < 20 && child.exitCode === null; waited += 1) await sleep(100);
  if (child.exitCode === null) child.kill("SIGKILL");
}

const report = { binary: BINARY, sample: SAMPLE, boot: {}, pixels: null };

try {
  // ---- 1. Library at launch: the §14 minimum ----------------------------
  log(`launching ${BINARY} (no document) — expecting the Library runtime`);
  const app = launch();
  const libraryLine = await app.waitForBoot(/boot: library$/, T_LIBRARY_MS, "library boot");
  if (libraryLine) {
    log(`library boot reported: ${libraryLine}`);
    // Let the first real frame land before the screenshot: the report is
    // emitted when the runtime's start export returned, which is up to a
    // frame before the window shows it.
    await sleep(SETTLE_MS);
    const stats = screenshotStats();
    report.pixels = stats;
    if (stats.error) {
      fail(`could not read the window's pixels: ${stats.error}`);
    } else {
      log(`window pixels: ${stats.colors} colours, luminance deviation ${stats.deviation?.toFixed(2)}`);
      // A blank window is a single colour (and, at most, the title bar and
      // shadow of a decorated frame): this is the §14 "merely opens a native
      // blank window" gate, measured rather than assumed.
      if (!(stats.colors >= 4) || !(stats.deviation >= 2)) {
        fail(
          `the window looks blank (${stats.colors} colours, deviation ${stats.deviation}) — ` +
            "a booted Library must paint a grid with cards and text",
        );
      }
    }
  }
  report.boot.library = libraryLine;

  // ---- 2. The OS handoff opens the Reader (§14, stronger) ---------------
  if (libraryLine) {
    log(`opening ${path.basename(SAMPLE)} the way the OS does (second launch, forwarded argv)`);
    const opener = launch([SAMPLE]);
    const readerLine = await app.waitForBoot(/boot: reader$/, T_READER_MS, "reader boot");
    await kill(opener.child);
    report.boot.reader = readerLine;
    if (readerLine) {
      log(`reader boot reported: ${readerLine}`);
      await sleep(SETTLE_MS);
      const stats = screenshotStats();
      report.pixelsAfterOpen = stats;
      if (stats.error) {
        fail(`could not read the window's pixels after the open: ${stats.error}`);
      } else if (!(stats.colors >= 4)) {
        fail(`the window looks blank after opening a document (${stats.colors} colours)`);
      }
    }
  }

  // A second instance that did NOT forward the file would leave the app at
  // the library, so the reader line above is also the single-instance path's
  // assertion.
  report.boot.lines = app.bootLines();
  await kill(app.child);
} catch (e) {
  fail(`the smoke threw: ${e.stack ?? e.message}`);
}

if (failures.length > 0) {
  console.error(`\nTAURI SMOKE FAILED (${failures.length} problem(s)):`);
  for (const failure of failures) console.error(`  - ${failure}`);
  console.error("\nBOOT REPORT " + JSON.stringify(report));
  process.exit(1);
}

console.log("\nTAURI SMOKE PASSED: native window booted the Library runtime, drew content, and handed a document to the Reader");
console.log("BOOT REPORT " + JSON.stringify(report));
