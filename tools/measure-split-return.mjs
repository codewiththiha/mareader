// Memory after a SPLIT read, measured the same way for any build of the app
// (the method of tools/measure-route-switch.mjs and
// docs/route-split-retrospective.md §5.1).
//
//   PORT=8123 DIST_DIR=dist node tests/browser/server.mjs &
//   node tools/measure-split-return.mjs [label] [baseUrl] [--intent]
//
// One scenario, end to end, in a fresh browser context: seed the library
// through `?open=`, boot the library fresh, click the book, split the
// workspace into four panes (three PDFs and a Markdown document, through
// the web build's `__mareaderOpenIn` hook — the one open command the split
// menu drives), scroll every pane, close the book with the reader's Library
// button, then sample for 70 s and once more after a forced GC. `--intent`
// keeps a pointer moving over the shelf during the idle window (what a hand
// resting on the mouse over the window does), hands off otherwise.
//
// Every sample is one JSON line: the renderer processes' PSS (Linux
// `/proc/<pid>/smaps_rollup`), the frames in the host with their slots, and
// the probe's memory fields. The last line is `RESULT {...}`. Absolute PSS
// varies by ±20 MB between runs; read the deltas within a run and the
// frame lists.
import { chromium } from "playwright";
import { readFileSync, readdirSync } from "node:fs";

const args = process.argv.slice(2).filter((a) => !a.startsWith("--"));
const label = args[0] ?? "dist";
const BASE = args[1] ?? process.env.BASE_URL ?? "http://127.0.0.1:8123";
const INTENT = process.argv.includes("--intent");
const BOOK = "/samples/Programming Pearls (2nd Edition) - Jon Bentley.pdf";
const NEEDLE = "Programming Pearls";
const SPLITS = [
  ["/samples/Deep Outline.pdf", "right"],
  ["/samples/Outlined Book.pdf", "down"],
  ["/samples/Split Notes.md", "right"],
];
const IDLE_SAMPLES_S = [0, 2, 5, 10, 20, 30, 45, 62, 70];

const browser = await chromium.launch();
const context = await browser.newContext({ viewport: { width: 1600, height: 1000 } });
const page = await context.newPage();
const cdp = await context.newCDPSession(page);
await cdp.send("HeapProfiler.enable");
const t0 = Date.now();
const consoleLines = [];
page.on("console", (m) => {
  const t = m.text();
  if (t.includes("integrity") || t.includes("willReadFrequently")) return;
  consoleLines.push(`+${Date.now() - t0} [${m.type()}] ${t.slice(0, 160)}`);
});

/** PSS/RSS of every Chromium renderer on the box (one browser runs at a time). */
function rendererMemory() {
  let pss = 0;
  let rss = 0;
  let n = 0;
  for (const p of readdirSync("/proc").filter((d) => /^\d+$/.test(d))) {
    let cmd = "";
    try {
      cmd = readFileSync(`/proc/${p}/cmdline`, "utf8");
    } catch {
      continue;
    }
    if (!cmd.includes("--type=renderer") || !/chrom|headless_shell/.test(cmd)) continue;
    try {
      const roll = readFileSync(`/proc/${p}/smaps_rollup`, "utf8");
      const get = (k) => Number((roll.match(new RegExp(`^${k}:\\s+(\\d+) kB`, "m")) ?? [0, 0])[1]) * 1024;
      pss += get("Pss");
      rss += get("Rss");
      n += 1;
    } catch {
      // a renderer that exited between the listing and the read
    }
  }
  return { pssMB: +(pss / 1048576).toFixed(1), rssMB: +(rss / 1048576).toFixed(1), renderers: n };
}

const snap = () =>
  page.evaluate(() => {
    try {
      return JSON.parse(window.__mareaderDiagnostics());
    } catch {
      return null;
    }
  });

const frames = () =>
  page.evaluate(() => {
    const list = [...document.querySelectorAll("#runtime-host iframe")];
    const kind = (f) => (f.src.includes("reader.html") ? "reader" : f.src.includes("library.html") ? "library" : "?");
    return list.map(
      (f) => `${kind(f)}:${f.getAttribute("data-mareader-slot") ?? "-"}:g${f.getAttribute("data-mareader-generation") ?? "?"}`,
    );
  });

async function sample(tag) {
  const s = await snap();
  const row = {
    tag,
    tMs: Date.now() - t0,
    ...rendererMemory(),
    frames: await frames(),
    readerFramesResident: s?.readerFramesResident ?? null,
    panes: s?.host?.panes?.length ?? null,
    heapHighWaterMB: s?.heapHighWaterBytes != null ? +(s.heapHighWaterBytes / 1048576).toFixed(1) : null,
  };
  console.log(JSON.stringify(row));
  return row;
}

async function waitFor(what, pred, timeout = 60_000) {
  const started = Date.now();
  for (;;) {
    const s = await snap();
    if (s && pred(s)) return s;
    if (Date.now() - started > timeout) {
      throw new Error(`timeout: ${what} — last ${JSON.stringify(s).slice(0, 300)}`);
    }
    await page.waitForTimeout(50);
  }
}

const activeFrame = '#runtime-host iframe.runtime-frame[data-mareader-slot="active"]';

/** Run `fn(doc, arg)` in the active frame's document. */
const inActive = (fn, arg) =>
  page.evaluate(
    ([sel, src, a]) => {
      const doc = document.querySelector(sel)?.contentDocument ?? null;
      return new Function("doc", "arg", `return (${src})(doc, arg);`)(doc, a);
    },
    [activeFrame, fn.toString(), arg],
  );

const shelfIntent = () =>
  inActive((doc) => {
    const level = doc?.getElementById("library-level") ?? doc?.body;
    level?.dispatchEvent(new PointerEvent("pointerover", { bubbles: true, pointerType: "mouse" }));
  });

const result = { label, base: BASE, intent: INTENT };

// 1. seed a library with the book (the real path: open from the URL)
await page.goto(`${BASE}/?blend=1&open=${encodeURIComponent(BOOK)}`, { waitUntil: "domcontentloaded" });
await waitFor("seed reader", (s) => s.bootState === "reader" && s.engine?.hasDocument && (s.engine?.rendersCompleted ?? 0) >= 1);
await page.waitForTimeout(1500);

// 2. a fresh library boot
await page.goto(`${BASE}/`, { waitUntil: "domcontentloaded" });
await waitFor("library boot", (s) => s.bootState === "library");
await page.waitForTimeout(2500);
result.libraryAtRest = await sample("library at rest (fresh boot, before any read)");

// 3. open the book from the shelf
await shelfIntent();
await page.waitForTimeout(1500);
// A real click first, as the lifecycle suite's `clickBook` does: the card
// opens on the pointer sequence, which a bare `el.click()` does not carry.
try {
  await page
    .frameLocator('iframe.runtime-frame[data-mareader-slot="active"]')
    .locator(`.book-title[title*="${NEEDLE}"]`)
    .first()
    .click({ timeout: 5_000 });
} catch {
  await inActive((doc, needle) => {
    const el = [...(doc?.querySelectorAll(".book-title") ?? [])].find((n) => (n.textContent ?? "").includes(needle));
    if (!el) throw new Error("book row not found");
    el.click();
  }, NEEDLE);
}
await waitFor("document rendered", (s) => s.bootState === "reader" && s.engine?.hasDocument && (s.engine?.rendersCompleted ?? 0) >= 1);
await page.waitForTimeout(1000);

// 4. split into four panes
for (const [path, target] of SPLITS) {
  const placed = await page.evaluate(
    ([sel, p, t]) => document.querySelector(sel)?.contentWindow?.__mareaderOpenIn?.(p, t) === true,
    [activeFrame, path, target],
  );
  if (!placed) throw new Error(`the host refused ${path} (${target})`);
  await page.waitForTimeout(800);
}
await waitFor(
  "four ready panes",
  (s) => s.host?.panes?.length === 4 && s.host.panes.every((p) => p.lifecycle === "ready"),
  30_000,
);
// read in every pane: a wheel over each pane's box
const boxes = await inActive((doc) =>
  [...(doc?.querySelectorAll("[data-pane-id]") ?? [])].map((el) => {
    const r = el.getBoundingClientRect();
    return { x: r.x + r.width / 2, y: r.y + r.height / 2 };
  }),
);
for (let round = 0; round < 6; round += 1) {
  for (const b of boxes) {
    await page.mouse.move(b.x, b.y);
    await page.mouse.wheel(0, 1600);
    await page.waitForTimeout(150);
  }
}
await page.waitForTimeout(2500);
result.reading = await sample("reading, four panes");

// 5. back to the library through the reader's own button
const closeAt = Date.now();
await inActive((doc) => {
  const btn = doc?.querySelector('button[title*="Close this book"]');
  if (!btn) throw new Error("close button not found");
  btn.click();
});
await waitFor("library back on screen", (s) => s.bootState === "library");
result.toLibraryOnScreenMs = Date.now() - closeAt;
await page.mouse.move(0, 0);

// 6. memory after the return
result.afterReturn = [];
const returnAt = Date.now();
let lastIntent = 0;
for (const sec of IDLE_SAMPLES_S) {
  for (;;) {
    const wait = returnAt + sec * 1000 - Date.now();
    if (wait <= 0) break;
    if (INTENT && Date.now() - lastIntent >= 1000) {
      lastIntent = Date.now();
      await shelfIntent().catch(() => {});
    }
    await page.waitForTimeout(Math.min(wait, INTENT ? 1000 : wait));
  }
  result.afterReturn.push(await sample(`+${sec}s after returning to the library`));
}

// 7. what a GC can still reclaim
await cdp.send("HeapProfiler.collectGarbage");
await page.waitForTimeout(1500);
result.afterGc = await sample("after a forced GC");

console.log(`RESULT ${JSON.stringify(result)}`);
console.log(`CONSOLE ${JSON.stringify(consoleLines.filter((l) => /mareader\]|evict|forced|panick|error/i.test(l)).slice(0, 40))}`);
await browser.close();
