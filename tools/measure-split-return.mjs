// PSS after a split read: one scenario per fresh browser context.
import { chromium, webkit } from "playwright";
import { readFileSync, readdirSync } from "node:fs";

const args = process.argv.slice(2).filter((a) => !a.startsWith("--"));
const label = args[0] ?? "dist";
const BASE = args[1] ?? process.env.BASE_URL ?? "http://127.0.0.1:8123";
const INTENT = process.argv.includes("--intent");
const WEBKIT = process.argv.includes("--webkit");
const ENGINE = WEBKIT ? "webkit" : "chromium";
const BOOK = "/samples/Programming Pearls (2nd Edition) - Jon Bentley.pdf";
const NEEDLE = "Programming Pearls";
const SPLITS = [
  ["/samples/Deep Outline.pdf", "right"],
  ["/samples/Outlined Book.pdf", "down"],
  ["/samples/Split Notes.md", "right"],
];
const IDLE_SAMPLES_S = [0, 2, 5, 10, 20, 30, 45, 62, 70];

const browser = await (WEBKIT ? webkit : chromium).launch();
const context = await browser.newContext({ viewport: { width: 1600, height: 1000 } });
const page = await context.newPage();
const cdp = WEBKIT ? null : await context.newCDPSession(page);
await cdp?.send("HeapProfiler.enable");
const t0 = Date.now();
const consoleLines = [];
page.on("console", (m) => {
  const t = m.text();
  if (t.includes("integrity") || t.includes("willReadFrequently")) return;
  consoleLines.push(`+${Date.now() - t0} [${m.type()}] ${t.slice(0, 160)}`);
});

/** PSS/RSS of every content process on the box. */
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
    const content = WEBKIT
      ? /(WPE|WebKit)WebProcess/.test(cmd)
      : cmd.includes("--type=renderer") && /chrom|headless_shell/.test(cmd);
    if (!content) continue;
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
    // Each runtime slot, then each pane's own frame.
    const kind = (f) =>
      f.tagName === "IFRAME"
        ? f.src.includes("reader.html") ? "reader" : f.src.includes("library.html") ? "library" : "?"
        : (f.getAttribute("data-mareader-runtime-frame") ?? "?").toLowerCase();
    const slots = [...document.querySelectorAll("#runtime-host .runtime-frame")].map(
      (f) => `${kind(f)}:${f.getAttribute("data-mareader-slot") ?? "-"}:g${f.getAttribute("data-mareader-generation") ?? "?"}`,
    );
    const documents = [document];
    for (const frame of document.querySelectorAll("iframe.runtime-frame")) {
      if (frame.contentDocument) documents.push(frame.contentDocument);
    }
    const panes = documents.flatMap((doc) => [...doc.querySelectorAll("iframe.pane-frame")]).map(
      (f) => `pane:${new URL(f.src).pathname.replace(/^\/|\.html$/g, "")}`,
    );
    return [...slots, ...panes];
  });

async function sample(tag) {
  const s = await snap();
  const row = {
    tag,
    tMs: Date.now() - t0,
    ...rendererMemory(),
    frames: await frames(),
    readerFramesResident: s?.readerFramesResident ?? null,
    libraryFramesResident: s?.libraryFramesResident ?? null,
    librarySessionsCreated: s?.librarySessionsCreated ?? null,
    libraryDisposesCompleted: s?.libraryDisposesCompleted ?? null,
    paneFramesResident: s?.paneFramesResident ?? null,
    routeReturnPolicy: s?.routeReturnPolicy ?? null,
    rasterLane: s?.rasterLane ?? null,
    atBaseline: s?.atBaseline ?? null,
    panes: s?.host?.panes?.length ?? null,
    heapHighWaterMB: s?.heapHighWaterBytes != null ? +(s.heapHighWaterBytes / 1048576).toFixed(1) : null,
  };
  if (row.renderers === 0) {
    // A page is always loaded, so nothing matched means the matcher is wrong.
    const seen = new Set();
    for (const p of readdirSync("/proc").filter((d) => /^\d+$/.test(d))) {
      try {
        seen.add(readFileSync(`/proc/${p}/comm`, "utf8").trim());
      } catch {
        // gone
      }
    }
    throw new Error(`[${tag}] no ${ENGINE} content process found; processes: ${[...seen].sort().join(" ")}`);
  }
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

// The active runtime's slot: an iframe or an element of the Shell's document.
const activeFrame = '#runtime-host .runtime-frame[data-mareader-slot="active"]';
const framed = () =>
  page.evaluate((sel) => document.querySelector(sel)?.tagName === "IFRAME", activeFrame);

/** Run `fn(doc, arg)` against the active runtime's document or slot. */
const inActive = (fn, arg) =>
  page.evaluate(
    ([sel, src, a]) => {
      const el = document.querySelector(sel);
      const doc = (el?.tagName === "IFRAME" ? el.contentDocument : el) ?? null;
      return new Function("doc", "arg", `return (${src})(doc, arg);`)(doc, a);
    },
    [activeFrame, fn.toString(), arg],
  );

const shelfIntent = () =>
  inActive((doc) => {
    const level = doc?.querySelector("#library-level") ?? doc;
    level?.dispatchEvent(new PointerEvent("pointerover", { bubbles: true, pointerType: "mouse" }));
  });

const result = { label, base: BASE, intent: INTENT, engine: ENGINE };

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
// A real click first: the card opens on the pointer sequence.
try {
  const scope = (await framed()) ? page.frameLocator(activeFrame) : page.locator(activeFrame);
  await scope
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
    ([sel, p, t]) => {
      const el = document.querySelector(sel);
      const win = el?.tagName === "IFRAME" ? el.contentWindow : window;
      return win?.__mareaderOpenIn?.(p, t) === true;
    },
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
await waitFor("Library route removed while reading", (s) => label !== "current" ||
  (s.libraryFramesResident === 0 && s.librarySessionsCreated === s.libraryDisposesCompleted && !s.bakeFrameResident));
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
  const row = await sample(`+${sec}s after returning to the library`);
  // Pinned builds keep their policy; the current build proves no-Reader.
  if (label === "current" && sec >= 2 &&
      (row.routeReturnPolicy !== "unload-both" || row.readerFramesResident !== 0 ||
       row.paneFramesResident !== 0 || row.libraryFramesResident !== 1 || row.atBaseline !== true ||
       row.librarySessionsCreated - row.libraryDisposesCompleted !== 1 ||
       row.frames.filter((f) => f.startsWith("library:")).length !== 1 ||
       row.frames.some((f) => f.startsWith("library:") && result.libraryAtRest.frames.includes(f)) ||
       row.frames.some((f) => /^(reader|pane):/.test(f)) ||
       row.rasterLane?.active !== 0 || row.rasterLane?.queued !== 0 || row.rasterLane?.owners !== 0)) {
    throw new Error(`Reader realms survived Library return: ${JSON.stringify(row)}`);
  }
  result.afterReturn.push(row);
}

// 7. what a GC can still reclaim
if (cdp) await cdp.send("HeapProfiler.collectGarbage");
await page.waitForTimeout(1500);
result.afterGc = await sample(cdp ? "after a forced GC" : "after a 1.5 s settle (WebKit has no forced GC)");

console.log(`RESULT ${JSON.stringify(result)}`);
console.log(`CONSOLE ${JSON.stringify(consoleLines.filter((l) => /mareader\]|evict|forced|panick|error/i.test(l)).slice(0, 40))}`);
await browser.close();
