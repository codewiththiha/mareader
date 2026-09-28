// Route-switch latency and memory-after-return, measured the same way for
// any build of the app — the numbers in docs/route-split-retrospective.md.
//
//   node tests/browser/server.mjs &                 # serves dist/ on :8123
//   node tools/measure-route-switch.mjs [label] [baseUrl] [--no-hover]
//
// One scenario, end to end, in a fresh browser context: seed one read book
// through `?open=`, boot the library fresh, give the shelf 1.5 s of pointer
// intent (skipped with --no-hover), click the book, flick through ~40
// screens, close, keep the hands off for 70 s while sampling, then force a
// GC and sample once more. Every sample is one JSON line: the renderer
// processes' PSS (Linux `/proc/<pid>/smaps_rollup` — this tool is Linux
// only), the frames in the host with their slots, and the probe's memory
// fields. The last line is `RESULT {...}` with the four latencies.
//
// Absolute PSS varies by ±20 MB between runs (V8 and the code cache are not
// deterministic); read the deltas within a run and the frame lists.
// `BOOK`/`NEEDLE` pick the fixture (a `dist/samples/` path and a substring
// of its shelf title); the defaults are the suite's Programming Pearls.
import { chromium } from "playwright";
import { readFileSync, readdirSync } from "node:fs";

const args = process.argv.slice(2).filter((a) => !a.startsWith("--"));
const label = args[0] ?? "dist";
const BASE = args[1] ?? process.env.BASE_URL ?? "http://127.0.0.1:8123";
const HOVER = !process.argv.includes("--no-hover");
const BOOK = process.env.BOOK ?? "/samples/Programming Pearls (2nd Edition) - Jon Bentley.pdf";
const NEEDLE = process.env.NEEDLE ?? "Programming Pearls";
const IDLE_SAMPLES_S = [0, 2, 5, 10, 20, 30, 45, 62, 70];

const browser = await chromium.launch();
const context = await browser.newContext();
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
    return {
      reader: list.filter((f) => kind(f) === "reader").length,
      library: list.filter((f) => kind(f) === "library").length,
      bake: document.querySelectorAll("iframe.bake-frame").length,
      slots: list.map(
        (f) => `${kind(f)}:${f.getAttribute("data-mareader-slot") ?? "-"}:g${f.getAttribute("data-mareader-generation") ?? "?"}`,
      ),
    };
  });

async function sample(tag) {
  const s = await snap();
  const f = await frames();
  const row = {
    tag,
    tMs: Date.now() - t0,
    ...rendererMemory(),
    frames: f,
    bootState: s?.bootState,
    atBaseline: s?.atBaseline,
    warm: s?.warmRuntime ?? null,
    readerFramesResident: s?.readerFramesResident ?? null,
    wasmHeapMB: s?.wasmHeapBytes != null ? +(s.wasmHeapBytes / 1048576).toFixed(1) : null,
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
    await page.waitForTimeout(25);
  }
}

/** The library frame's document: the active slot when slots exist (v2+), else the library page. */
const inLibraryDocument = (fn, arg) =>
  page.evaluate(
    ([src, a]) => {
      const list = [...document.querySelectorAll("#runtime-host iframe")];
      const active =
        list.find((f) => f.getAttribute("data-mareader-slot") === "active") ?? list.find((f) => f.src.includes("library.html"));
      const doc = active?.contentDocument ?? null;
      return new Function("doc", "arg", `return (${src})(doc, arg);`)(doc, a);
    },
    [fn.toString(), arg],
  );

const result = { label, base: BASE, book: BOOK, hover: HOVER };

// 1. seed a library with one read book (the real path: open from the URL)
await page.goto(`${BASE}/?blend=1&open=${encodeURIComponent(BOOK)}`, { waitUntil: "domcontentloaded" });
await waitFor("seed reader", (s) => s.bootState === "reader" && s.engine?.hasDocument && (s.engine?.rendersCompleted ?? 0) >= 1);
await page.waitForTimeout(1500);

// 2. a fresh library boot, as a user opening the app
const navStart = Date.now();
await page.goto(`${BASE}/`, { waitUntil: "domcontentloaded" });
await waitFor("library boot", (s) => s.bootState === "library");
result.libraryBootMs = Date.now() - navStart;
await page.waitForTimeout(2500);
result.libraryAtRest = await sample("library at rest (fresh boot, before any read)");

// 3. library -> reader
if (HOVER) {
  await inLibraryDocument((doc) => {
    const level = doc?.getElementById("library-level") ?? doc?.body;
    level?.dispatchEvent(new PointerEvent("pointerover", { bubbles: true, pointerType: "mouse" }));
  });
  await page.waitForTimeout(1500);
  result.afterIntent = await sample("after 1.5 s of shelf intent");
}
const clickAt = Date.now();
{
  const frame = page.frames().find((f) => f.url().includes("library.html"));
  try {
    await frame.locator(`.book-title[title*="${NEEDLE}"]`).first().click({ timeout: 5_000 });
  } catch (e) {
    // The grid's gesture layer can swallow a synthetic hit; dispatching on
    // the row is the same open path.
    console.log(`real click failed, dispatching: ${e.message.split("\n")[0]}`);
    await inLibraryDocument((doc, needle) => {
      const el = [...(doc?.querySelectorAll(".book-title") ?? [])].find((n) => (n.textContent ?? "").includes(needle));
      if (!el) throw new Error("book row not found");
      el.click();
    }, NEEDLE);
  }
}
await waitFor("reader on screen", (s) => s.bootState === "reader");
result.toReaderOnScreenMs = Date.now() - clickAt;
await waitFor(
  "document rendered",
  (s) => s.bootState === "reader" && s.engine?.hasDocument && (s.engine?.rendersCompleted ?? 0) >= 1,
);
result.toDocumentRenderedMs = Date.now() - clickAt;

// read: flick through the book the way the lifecycle suite does, so the
// reader accumulates what a real read accumulates
await page.mouse.move(700, 450);
for (let i = 0; i < 40; i += 1) {
  await page.keyboard.press("PageDown");
  await page.mouse.wheel(0, 2200);
  await page.waitForTimeout(i % 5 === 4 ? 400 : 120);
}
await page.evaluate(() => {
  const list = [...document.querySelectorAll("#runtime-host iframe")]
    .find((f) => f.src.includes("reader.html"))
    ?.contentDocument?.querySelector("#page-list");
  if (list) list.scrollTop = list.scrollHeight / 2;
});
await page.waitForTimeout(2500);
result.reading = await sample("reading (after flicking through ~40 screens)");

// 4. reader -> library
const closeAt = Date.now();
await page.evaluate(() => {
  const list = [...document.querySelectorAll("#runtime-host iframe")];
  const active =
    list.find((f) => f.getAttribute("data-mareader-slot") === "active") ?? list.find((f) => f.src.includes("reader.html"));
  const btn = active?.contentDocument?.querySelector('button[title*="Close this book"]');
  if (!btn) throw new Error("close button not found");
  btn.click();
});
await waitFor(
  "library back on screen",
  (s) => s.bootState === "library" && (s.activeRuntime === undefined || s.activeRuntime === "library"),
);
result.toLibraryOnScreenMs = Date.now() - closeAt;
await page.mouse.move(0, 0);

// 5. memory after the return, hands off
result.afterReturn = [];
const returnAt = Date.now();
for (const sec of IDLE_SAMPLES_S) {
  const wait = returnAt + sec * 1000 - Date.now();
  if (wait > 0) await page.waitForTimeout(wait);
  result.afterReturn.push(await sample(`+${sec}s after returning to the library`));
}

// 6. what a GC can still reclaim
await cdp.send("HeapProfiler.collectGarbage");
await page.waitForTimeout(1500);
result.afterGc = await sample("after a forced GC");

console.log(`RESULT ${JSON.stringify(result)}`);
console.log(`CONSOLE ${JSON.stringify(consoleLines.filter((l) => /mareader\]|warm|evict|forced|panick|error/i.test(l)).slice(0, 40))}`);
await browser.close();
