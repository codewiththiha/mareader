// Chrome-contract check — the window frame's numbers are declared on both
// sides of the IPC boundary, in two languages that cannot see each other.
// Each pair used to carry only a comment asking the next reader to keep it in
// sync; a comment is a request, and this is the enforcement.
//
//   1. the import progress channel (shell constant ↔ frontend constant)
//   2. the title-bar height (wasm constant ↔ native fallback) and the
//      traffic-light inset (native constant ↔ tauri.conf.json)
//   3. the z-index scale (app_chrome::layers ↔ styles/tokens.css)
//   4. the sidebar's motion durations and its traffic-light gutter
//      (shell controller constants ↔ the rail's Tailwind classes)
//
// None of these can fail a build when it drifts. A renamed channel means the
// progress ring never moves; a title-bar height that disagrees means the
// macOS traffic lights sit off-centre until the first ResizeObserver fires;
// a missing `--z-*` means a floating surface renders at z-index 0 and hides
// behind the page. Same family as check-versions and check-formats.
// TypeScript source; Trunk's pre-build hook compiles it to
// `scripts/check-chrome-contracts.js`.

import { read } from "./repo.js";

const failures: string[] = [];

function fail(message: string): void {
  failures.push(message);
}

/** The one match a pattern must produce in a file, or a named complaint. */
function sole(pattern: RegExp, text: string, label: string): string {
  const found = [...text.matchAll(pattern)].map((m) => m[1] ?? "");
  if (found.length === 0) {
    fail(`${label}: nothing matched ${pattern} — the declaration moved or was renamed`);
    return "";
  }
  if (found.length > 1) {
    fail(`${label}: ${found.length} matches (${found.join(", ")}) — narrow the pattern`);
    return found[0]!;
  }
  return found[0]!;
}

function agree(label: string, values: [file: string, value: string][]): void {
  const distinct = new Set(values.map(([, v]) => v));
  if (distinct.size === 1) {
    console.log(`${label}: ${values[0]![1]}`);
    return;
  }
  fail(`${label} disagrees across its declarations:`);
  for (const [file, value] of values) fail(`    ${file}: ${value}`);
}

// ── 1. The import progress channel ──────────────────────────────────────────
// The shell emits on it, the frontend listens on it, and the two constants sit
// in different languages with no shared definition. library-core's wire module
// documents the channel in prose, which is not a declaration and is not read
// here.
const SHELL_PROGRESS = "src-tauri/src/commands/library.rs";
const APP_PROGRESS = "src/services/library/mod.rs";

const shellChannel = sole(
  /^const PROGRESS_EVENT: &str = "([^"]+)";/mg,
  read(SHELL_PROGRESS),
  `${SHELL_PROGRESS} (PROGRESS_EVENT)`,
);
const appChannel = sole(
  /^pub\(crate\) const PROGRESS_CHANNEL: &str = "([^"]+)";/mg,
  read(APP_PROGRESS),
  `${APP_PROGRESS} (PROGRESS_CHANNEL)`,
);
if (shellChannel && appChannel) {
  agree("import progress channel", [
    [SHELL_PROGRESS, shellChannel],
    [APP_PROGRESS, appChannel],
  ]);
}

// ── 2. The title bar's height and the traffic-light inset ───────────────────
// `TITLE_BAR_H` is the pre-observation assumption on both sides: the wasm
// constant the ResizeObserver replaces, and the native centring fallback the
// shell uses before the first invoke carries a measured height. The x inset
// exists twice for the same reason — the native layout and the pre-mount
// `trafficLightPosition` Tauri applies before Rust takes over.
const TITLEBAR = "crates/app-chrome/src/titlebar/mod.rs";
const TRAFFIC_LIGHT = "src-tauri/src/macos/traffic_light.rs";
const TAURI_CONF = "src-tauri/tauri.conf.json";

const titleBar = sole(
  /^pub const TITLE_BAR_H: f64 = ([\d.]+);/mg,
  read(TITLEBAR),
  `${TITLEBAR} (TITLE_BAR_H)`,
);
const shellHeader = sole(
  /^    const DEFAULT_HEADER_HEIGHT: f64 = ([\d.]+);/mg,
  read(TRAFFIC_LIGHT),
  `${TRAFFIC_LIGHT} (DEFAULT_HEADER_HEIGHT)`,
);
if (titleBar && shellHeader) {
  agree("title-bar height", [
    [TITLEBAR, titleBar],
    [TRAFFIC_LIGHT, shellHeader],
  ]);
}

const shellInset = sole(
  /^    const TRAFFIC_LIGHT_X_INSET: f64 = ([\d.]+);/mg,
  read(TRAFFIC_LIGHT),
  `${TRAFFIC_LIGHT} (TRAFFIC_LIGHT_X_INSET)`,
);
const conf = JSON.parse(read(TAURI_CONF)) as {
  app?: { windows?: { trafficLightPosition?: { x?: number } }[] };
};
const confInset = conf.app?.windows?.[0]?.trafficLightPosition?.x;
if (shellInset && confInset === undefined) {
  fail(`${TAURI_CONF}: no app.windows[0].trafficLightPosition.x to compare the inset against`);
} else if (shellInset && Number(shellInset) !== confInset) {
  fail(
    `traffic-light x inset disagrees: ${TRAFFIC_LIGHT} says ${shellInset}, ` +
      `${TAURI_CONF} says ${confInset}`,
  );
} else if (shellInset) {
  console.log(`traffic-light x inset: ${shellInset}`);
}

// ── 3. The z-index scale ────────────────────────────────────────────────────
// `app_chrome::layers` holds the class-name constants components embed, and
// `styles/tokens.css` holds the numbers behind them. A constant naming a
// `--z-*` the stylesheet does not define is the silent half: Tailwind emits
// `z-[var(--z-thing)]` happily and the surface renders at z-index 0.
//
// The token name is matched as `[A-Za-z0-9-]+` on purpose — the stylesheet's
// own comment above the scale writes `z-[var(--z-…)]`, and an ellipsis is not
// a custom-property name.
const LAYERS = "crates/app-chrome/src/layers.rs";
const TOKENS = "styles/tokens.css";

const rustTokens = [
  ...read(LAYERS).matchAll(/^pub const \w+: &str = "z-\[var\((--[A-Za-z0-9-]+)\)\]";$/gm),
].map((m) => m[1]!);
const cssLayers = [...read(TOKENS).matchAll(/^\s*(--z-[A-Za-z0-9-]+):\s*(-?\d+);/gm)].map(
  (m) => ({ token: m[1]!, z: Number(m[2]) }),
);
const cssTokens = cssLayers.map((l) => l.token);

if (rustTokens.length === 0) fail(`${LAYERS}: no z-[var(--z-*)] constants found`);
if (cssTokens.length === 0) fail(`${TOKENS}: no --z-* custom properties found`);

for (const token of rustTokens) {
  if (!cssTokens.includes(token)) {
    fail(`${LAYERS} names ${token}, which ${TOKENS} does not define — the surface renders at z-index 0`);
  }
}
for (const token of cssTokens) {
  if (!rustTokens.includes(token)) {
    fail(`${TOKENS} defines ${token}, which no constant in ${LAYERS} embeds — nothing can reach that layer`);
  }
}

// The order IS the contract: both tables list the layers bottom-to-top, so a
// token inserted out of place is a surface that paints under one it should
// cover, and nothing else in the pipeline can see it. The stylesheet's
// numbers must ascend, and the constants must follow the stylesheet's order.
for (let i = 1; i < cssLayers.length; i++) {
  const prev = cssLayers[i - 1]!;
  const cur = cssLayers[i]!;
  if (cur.z <= prev.z) {
    fail(`${TOKENS}: ${cur.token} (${cur.z}) does not stack above ${prev.token} (${prev.z})`);
  }
}
const sameOrder =
  rustTokens.length === cssTokens.length &&
  rustTokens.every((token, i) => token === cssTokens[i]);
if (rustTokens.length > 0 && cssTokens.length > 0 && !sameOrder) {
  fail(
    `${LAYERS} and ${TOKENS} list the layers in different orders:\n` +
      `    ${LAYERS}: ${rustTokens.join(", ")}\n` +
      `    ${TOKENS}: ${cssTokens.join(", ")}`,
  );
}

if (rustTokens.length > 0 && cssTokens.length > 0 && sameOrder) {
  console.log(`z-index scale agrees on ${cssTokens.length} layers, in stacking order`);
}

// ── 4. The sidebar's motion and gutter ─────────────────────────────────────
// Both are a Rust number and a Tailwind class that cannot see each other, and
// both were guarded by a comment on the class asking the next reader to keep
// it in step.
//
//   motion — the close machine holds the panel and its live canvases for
//            exactly as long as the rail takes to get out of the way, so the
//            hold and the transition have to land on the same frame. A hold
//            that outlasts the slide releases the bar's inset a timer late;
//            one that ends early drops the canvases mid-slide.
//   gutter — the traffic-light corner is reserved twice: by the bar's row
//            padding, and by the rail header's own while the rail owns that
//            corner. Different widths put the lights off-centre in one of
//            them, which is the title-bar-height failure one section up.
const SHELL_CONTROLLER = "src/components/shell/controller/mod.rs";
const SIDEBAR_ASIDE = "src/components/shell/sidebar/container.rs";
const SIDEBAR_OVERLAY = "src/components/shell/sidebar/overlay.rs";
const SIDEBAR_HEADER = "src/components/shell/sidebar/header.rs";

/** The one `duration-N` a quoted class list carries. Unquoted mentions in
 * doc comments are prose and must not be counted as the declaration. */
function tailwindDuration(file: string, label: string): string {
  return sole(/"[^"\n]*\bduration-(\d+)\b[^"\n]*"/g, read(file), `${file} (${label})`);
}

/** The one `pl-[Npx]` a quoted class list carries. */
function tailwindPaddingPx(file: string, label: string): string {
  return sole(/"[^"\n]*\bpl-\[(\d+)px\][^"\n]*"/g, read(file), `${file} (${label})`);
}

function agreeNumber(label: string, unit: string, values: [file: string, value: string][]): void {
  const nums = values.map(([, v]) => Number(v));
  if (nums.every((n) => n === nums[0])) {
    console.log(`${label}: ${nums[0]}${unit}`);
    return;
  }
  fail(`${label} disagrees across its declarations:`);
  for (const [file, value] of values) fail(`    ${file}: ${value}`);
}

const slideMs = sole(
  /^pub\(crate\) const SIDEBAR_SLIDE_MS: u64 = (\d+);/gm,
  read(SHELL_CONTROLLER),
  `${SHELL_CONTROLLER} (SIDEBAR_SLIDE_MS)`,
);
const fadeMs = sole(
  /^pub\(crate\) const SIDEBAR_FADE_MS: u64 = (\d+);/gm,
  read(SHELL_CONTROLLER),
  `${SHELL_CONTROLLER} (SIDEBAR_FADE_MS)`,
);
const gutterPx = sole(
  /^const TRAFFIC_LIGHTS_GUTTER_PX: f64 = ([\d.]+);/gm,
  read(SHELL_CONTROLLER),
  `${SHELL_CONTROLLER} (TRAFFIC_LIGHTS_GUTTER_PX)`,
);

const asideMs = tailwindDuration(SIDEBAR_ASIDE, "docked aside width tween");
const overlayMs = tailwindDuration(SIDEBAR_OVERLAY, "floating wrapper fade");
const headerPx = tailwindPaddingPx(SIDEBAR_HEADER, "rail header chrome row");

if (slideMs && asideMs) {
  agreeNumber("docked rail slide", "ms", [
    [SHELL_CONTROLLER, slideMs],
    [SIDEBAR_ASIDE, asideMs],
  ]);
}
if (fadeMs && overlayMs) {
  agreeNumber("floating rail fade", "ms", [
    [SHELL_CONTROLLER, fadeMs],
    [SIDEBAR_OVERLAY, overlayMs],
  ]);
}
if (gutterPx && headerPx) {
  agreeNumber("traffic-light gutter", "px", [
    [SHELL_CONTROLLER, gutterPx],
    [SIDEBAR_HEADER, headerPx],
  ]);
}

// ── verdict ─────────────────────────────────────────────────────────────────
if (failures.length > 0) {
  for (const line of failures) console.error(`::error::${line}`);
  process.exit(1);
}
console.log("chrome contracts agree");
