// Declared twice across the IPC: channel, height, z-index, motion, ring.

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

// ── 1. The import progress channel
// One constant per side, never shared.
const SHELL_PROGRESS = "src-tauri/src/commands/library.rs";
const APP_PROGRESS = "crates/library-runtime/src/services/mod.rs";

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

// ── 2. Height and inset
// Mirrored assumptions the observer replaces.
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
  app?: { windows?: { trafficLightPosition?: { x?: number; y?: number } }[] };
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

// The y inset mirrors too: tao re-applies the config's `y` on every redraw.
const shellInsetY = sole(
  /^    const TRAFFIC_LIGHT_Y_INSET: f64 = ([\d.]+);/mg,
  read(TRAFFIC_LIGHT),
  `${TRAFFIC_LIGHT} (TRAFFIC_LIGHT_Y_INSET)`,
);
const confInsetY = conf.app?.windows?.[0]?.trafficLightPosition?.y;
if (shellInsetY && Number(shellInsetY) !== confInsetY) {
  fail(
    `traffic-light y inset disagrees: ${TRAFFIC_LIGHT} says ${shellInsetY}, ` +
      `${TAURI_CONF} says ${confInsetY}`,
  );
} else if (shellInsetY) {
  console.log(`traffic-light y inset: ${shellInsetY}`);
}

// ── 3. The z-index scale
// layers and tokens.css: same names, same order.
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

// Order is the contract: both tables list layers bottom-to-top.
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

// ── 4. Sidebar motion and gutter
// Hold matches slide; gutter, the lights' corner.
const SHELL_CONTROLLER = "crates/app-ui/src/components/shell/controller/mod.rs";
const SIDEBAR_ASIDE = "crates/reader-runtime/src/components/shell/sidebar/container.rs";
const SIDEBAR_OVERLAY = "crates/reader-runtime/src/components/shell/sidebar/overlay.rs";
const SIDEBAR_HEADER = "crates/reader-runtime/src/components/shell/sidebar/header.rs";

/** The one `duration-N` a quoted class list carries; prose does not count. */
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

// ── 5. The dock's ring
// One radius: SVG r, dasharray, offset.
const PROGRESS_DOCK = "crates/library-runtime/src/features/library/progress_dock.rs";
const DOCK_CSS = "styles/components/library/dock.css";

const ringRadius = sole(
  /^const RING_RADIUS: f64 = ([\d.]+);/gm,
  read(PROGRESS_DOCK),
  `${PROGRESS_DOCK} (RING_RADIUS)`,
);

if (ringRadius) {
  const radius = Number(ringRadius);

  // The check rests on 2 * pi * r: the stylesheet spells the product.
  const formula = sole(
    /^const CIRCUMFERENCE: f64 = ([^;]+);/gm,
    read(PROGRESS_DOCK),
    `${PROGRESS_DOCK} (CIRCUMFERENCE)`,
  );
  if (formula && !/2\.0\s*\*\s*std::f64::consts::PI\s*\*\s*RING_RADIUS/.test(formula)) {
    fail(
      `${PROGRESS_DOCK}: CIRCUMFERENCE is ${formula.trim()}, but this check and the ` +
        `stylesheet's stroke-dasharray both assume 2.0 * pi * RING_RADIUS`,
    );
  }

  const circumference = 2 * Math.PI * radius;

  // The markup draws the circle, so its `r` is a fourth copy of the radius.
  const svgRadii = [...read(PROGRESS_DOCK).matchAll(/\bcx='18' cy='18' r='([\d.]+)'/g)].map(
    (m) => Number(m[1]),
  );
  if (svgRadii.length === 0) {
    fail(`${PROGRESS_DOCK}: no <circle r='…'> in the ring markup to compare RING_RADIUS against`);
  }
  for (const r of svgRadii) {
    if (r !== radius) {
      fail(`${PROGRESS_DOCK}: the SVG circle's r is ${r}, but RING_RADIUS is ${ringRadius}`);
    }
  }

  // The stylesheet carries the product twice: dash length, hiding offset.
  const TOL = 0.0005;
  const dockCss = read(DOCK_CSS);
  const dash = sole(
    /^  stroke-dasharray: ([\d.]+);/gm,
    dockCss,
    `${DOCK_CSS} (stroke-dasharray)`,
  );
  const offsetFallback = sole(
    /stroke-dashoffset: var\(--ring-offset, ([\d.]+)\)/g,
    dockCss,
    `${DOCK_CSS} (stroke-dashoffset fallback)`,
  );
  for (const [what, value] of [
    ["stroke-dasharray", dash],
    ["stroke-dashoffset fallback", offsetFallback],
  ] as const) {
    if (!value) continue;
    if (Math.abs(Number(value) - circumference) > TOL) {
      fail(
        `${DOCK_CSS}: ${what} is ${value}, but 2 * pi * RING_RADIUS(${ringRadius}) ` +
          `is ${circumference.toFixed(3)} — the ring stops short of full at 100%`,
      );
    }
  }

  // Scanning parks the dash at a fixed fraction, so a radius change moves it.
  const spin = sole(
    /^  stroke-dashoffset: ([\d.]+);/gm,
    dockCss,
    `${DOCK_CSS} (spin stroke-dashoffset)`,
  );
  if (spin) {
    const shown = 1 - Number(spin) / circumference;
    if (Math.abs(shown - 0.25) > 0.01) {
      fail(
        `${DOCK_CSS}: the scan arc shows ${(shown * 100).toFixed(1)}% of the ring, ` +
          `not the quarter turn its comment describes`,
      );
    }
  }

  if (!failures.some((line) => line.startsWith(DOCK_CSS) || line.startsWith(PROGRESS_DOCK))) {
    console.log(
      `progress ring agrees: r=${ringRadius}, circumference ${circumference.toFixed(3)}`,
    );
  }
}

// ── verdict ─────────────────────────────────────────────────────────────────
if (failures.length > 0) {
  for (const line of failures) console.error(`::error::${line}`);
  process.exit(1);
}
console.log("chrome contracts agree");
