// Does a closed split pane give its memory back, or does the reader grow
// with every split opened and closed?
//
//   node tools/measure-split-cycles.mjs [label] [baseUrl] [--webkit] [--cycles N]
//
// Workload: Programming Pearls open in the first pane for the whole run.
// Then N cycles of: open a split beside it (rotating two PDFs and a Markdown
// note, so both the pdf.js engine path and the reflow path are exercised),
// read in the new pane with the wheel, close it through its own × control,
// wait until the host lists one pane and the engine reports no session
// beyond the first pane's, then force a GC (Chromium, over CDP) or settle
// (WebKit has no forced GC) and sample.
//
// Each sample: content-process PSS (Linux `/proc/<pid>/smaps_rollup`, as in
// measure-split-return.mjs), the reader wasm's linear memory and its Rust
// heap high-water, the engine's live sessions and workers, and the reader
// frame's DOM element and canvas counts. The last line is `RESULT {...}`
// with a least-squares slope per cycle over the cycles after the warm-up
// (then two more samples: 30 s idle, and after allocation pressure, which
// is how WebKit — no forced GC — is made to collect),
// which is the growth question: a slope near zero means a closed pane's
// memory is reused by the next one; a steady positive slope is a leak.
import { chromium, webkit } from "playwright";
import { readFileSync, readdirSync } from "node:fs";

const args = process.argv.slice(2);
const positional = args.filter((a, i) => !a.startsWith("--") && args[i - 1] !== "--cycles");
const label = positional[0] ?? "current";
const BASE = positional[1] ?? "http://127.0.0.1:8124";
const WEBKIT = args.includes("--webkit");
const ENGINE = WEBKIT ? "webkit" : "chromium";
const cyclesAt = args.indexOf("--cycles");
const CYCLES = cyclesAt >= 0 && Number(args[cyclesAt + 1]) > 0 ? Number(args[cyclesAt + 1]) : 16;
const WARMUP = 3;
const BOOK = "/samples/Programming Pearls (2nd Edition) - Jon Bentley.pdf";
const ROTATION = ["/samples/Deep Outline.pdf", "/samples/Split Notes.md", "/samples/Outlined Book.pdf"];

const browser = await (WEBKIT ? webkit : chromium).launch();
const context = await browser.newContext({ viewport: { width: 1600, height: 1000 } });
const page = await context.newPage();
const cdp = WEBKIT ? null : await context.newCDPSession(page);
await cdp?.send("HeapProfiler.enable");
const activeFrame = '#runtime-host .runtime-frame[data-mareader-slot="active"]';

function contentMemory() {
  let pss = 0;
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
      pss += Number((roll.match(/^Pss:\s+(\d+) kB/m) ?? [0, 0])[1]) * 1024;
      n += 1;
    } catch {
      // exited between the listing and the read
    }
  }
  if (n === 0) throw new Error(`no ${ENGINE} content process found`);
  return +(pss / 1048576).toFixed(1);
}

// The reader frame's own probe answers a fresh snapshot (the Shell's merges
// the last pushed digest, a beat stale); the Shell's adds `bootState`. The
// frame's `wasmHeapBytes` is the reader instance's linear memory
// (`WebAssembly.Memory.buffer.byteLength`), which can only grow — so a flat
// line across cycles means a closed pane's Rust memory is reused.
const snap = () =>
  page.evaluate((sel) => {
    const read = (w) => {
      try {
        return JSON.parse(w?.__mareaderDiagnostics?.() ?? "null");
      } catch {
        return null;
      }
    };
    // The reader runtime mounts in the Shell's document: its digest (with
    // the pane frames folded in) sits beside the Shell's own.
    const top = read(window);
    const reader = document.querySelector(sel) ? read({ __mareaderDiagnostics: window.__mareaderReaderDiagnostics }) : null;
    return top || reader ? { ...(top ?? {}), ...(reader ?? {}), bootState: top?.bootState } : null;
  }, activeFrame);

async function waitFor(what, pred, timeout = 45_000) {
  const started = Date.now();
  for (;;) {
    const s = await snap();
    if (s && pred(s)) return s;
    if (Date.now() - started > timeout) throw new Error(`timeout: ${what} — last ${JSON.stringify(s?.host ?? s).slice(0, 400)}`);
    await page.waitForTimeout(50);
  }
}

const inActive = (fn, arg) =>
  page.evaluate(
    ([sel, src, a]) => {
      const doc = document.querySelector(sel) ? document : null;
      return new Function("doc", "arg", `return (${src})(doc, arg);`)(doc, a);
    },
    [activeFrame, fn.toString(), arg],
  );

const MB = (b) => (b == null ? null : +(b / 1048576).toFixed(2));

async function sample(tag) {
  if (cdp) await cdp.send("HeapProfiler.collectGarbage");
  await page.waitForTimeout(1500);
  const s = await snap();
  const dom = await inActive((doc) => ({
    elements: doc?.getElementsByTagName("*").length ?? null,
    canvases: doc?.querySelectorAll("canvas").length ?? null,
  }));
  const js = cdp ? await cdp.send("Runtime.getHeapUsage").catch(() => null) : null;
  const row = {
    tag,
    pssMB: contentMemory(),
    jsHeapMB: js ? MB(js.usedSize) : null,
    wasmMemoryMB: MB(s?.wasmHeapBytes),
    heapHighWaterMB: MB(s?.heapHighWaterBytes),
    panes: s?.host?.panes?.length ?? null,
    sessionsLive: s?.engine?.sessionsLive ?? null,
    workersLive: s?.engine ? s.engine.workersCreated - s.engine.workersTerminated : null,
    pageCanvasMB: MB(s?.engine?.pageCanvasBytesEst),
    readerFrames: s?.readerFramesResident ?? null,
    ...dom,
  };
  console.log(JSON.stringify(row));
  return row;
}

/** Least-squares slope of `key` over `rows` (per cycle). */
function slope(rows, key) {
  const pts = rows.map((r, i) => [i, r[key]]).filter(([, y]) => typeof y === "number");
  if (pts.length < 2) return null;
  const n = pts.length;
  const mx = pts.reduce((a, [x]) => a + x, 0) / n;
  const my = pts.reduce((a, [, y]) => a + y, 0) / n;
  const num = pts.reduce((a, [x, y]) => a + (x - mx) * (y - my), 0);
  const den = pts.reduce((a, [x]) => a + (x - mx) ** 2, 0);
  return +(num / den).toFixed(3);
}

const result = { label, engine: ENGINE, cycles: CYCLES, warmup: WARMUP };

// The first pane: Pearls, opened from the URL, kept for the whole run.
await page.goto(`${BASE}/?blend=1&open=${encodeURIComponent(BOOK)}`, { waitUntil: "domcontentloaded" });
const first = await waitFor("first pane rendered", (s) =>
  s.bootState === "reader" && s.engine?.hasDocument && (s.engine?.rendersCompleted ?? 0) >= 1 &&
  s.host?.panes?.length === 1 && s.host.panes[0].lifecycle === "ready");
const firstPane = first.host.panes[0].paneId;
await page.waitForTimeout(1500);
result.onePane = await sample("one pane, before any split");

result.afterClose = [];
result.whileOpen = [];
for (let cycle = 1; cycle <= CYCLES; cycle += 1) {
  const file = ROTATION[(cycle - 1) % ROTATION.length];
  const placed = await page.evaluate(
    ([sel, p]) => !!document.querySelector(sel) && window.__mareaderOpenIn?.(p, "right") === true,
    [activeFrame, file],
  );
  if (!placed) throw new Error(`[cycle ${cycle}] the host refused ${file}`);
  const open = await waitFor(`[cycle ${cycle}] two ready panes`, (s) =>
    s.host?.panes?.length === 2 && s.host.panes.every((p) => p.lifecycle === "ready" && p.resources?.documentSession === true));
  const added = open.host.panes.find((p) => p.paneId !== firstPane)?.paneId;
  if (added == null) throw new Error(`[cycle ${cycle}] the first pane was replaced`);

  // Read in the new pane.
  const box = await inActive((doc, id) => {
    const r = doc?.querySelector(`[data-pane-id="${id}"]`)?.getBoundingClientRect();
    return r ? { x: r.x + r.width / 2, y: r.y + r.height / 2 } : null;
  }, added);
  if (box) {
    await page.mouse.move(box.x, box.y);
    for (let i = 0; i < 5; i += 1) {
      await page.mouse.wheel(0, 1400);
      await page.waitForTimeout(160);
    }
  }
  await page.waitForTimeout(800);
  if (cycle === 1 || cycle === CYCLES) result.whileOpen.push({ cycle, file, ...(await sample(`cycle ${cycle}: ${file} open beside`)) });

  // Close it through its own control.
  await page.evaluate(([sel, id]) => {
    const btn = document.querySelector(sel)?.querySelector(`[data-pane-close="${id}"] button`);
    if (!btn) throw new Error(`pane ${id} has no close control`);
    btn.click();
  }, [activeFrame, added]);
  await waitFor(`[cycle ${cycle}] back to the first pane alone`, (s) =>
    s.host?.panes?.length === 1 && s.host.panes[0].paneId === firstPane &&
    s.engine?.sessionsLive === 1 && s.engine.workersCreated - s.engine.workersTerminated === 1);
  result.afterClose.push({ cycle, file, ...(await sample(`cycle ${cycle}: ${file} closed`)) });
}

// Is what stays after the last close garbage the engine has not collected
// yet, or memory it keeps? WebKit has no forced GC, so two ways to make it
// collect: time (JSC's timer-driven collections after allocation), then
// allocation pressure in the reader frame's realm (short-lived objects and
// buffers well past its collection thresholds, all dropped at once).
await page.waitForTimeout(30_000);
result.afterIdle30s = await sample("30 s after the last close");
await inActive(() => {
  for (let round = 0; round < 4; round += 1) {
    let objects = [];
    for (let i = 0; i < 1_000_000; i += 1) objects.push({ i, s: `x${i}` });
    objects = null;
    let buffers = [];
    for (let i = 0; i < 8; i += 1) buffers.push(new Uint8Array(8 * 1048576).fill(i));
    buffers = null;
  }
});
await page.waitForTimeout(3000);
result.afterPressure = await sample("after allocation pressure");

const steady = result.afterClose.slice(WARMUP);
const last = result.afterClose.at(-1);
const ref = result.afterClose[WARMUP - 1];
result.summary = {
  pssSlopeMBPerCycle: slope(steady, "pssMB"),
  jsHeapSlopeMBPerCycle: slope(steady, "jsHeapMB"),
  wasmMemorySlopeMBPerCycle: slope(steady, "wasmMemoryMB"),
  elementsSlopePerCycle: slope(steady, "elements"),
  pssOnePane: result.onePane.pssMB,
  pssAfterWarmup: ref.pssMB,
  pssAfterLast: last.pssMB,
  pssAfterIdle30s: result.afterIdle30s.pssMB,
  pssAfterPressure: result.afterPressure.pssMB,
  pssMin: Math.min(...steady.map((r) => r.pssMB)),
  pssMax: Math.max(...steady.map((r) => r.pssMB)),
  wasmMemoryFirstLastMB: [result.onePane.wasmMemoryMB, last.wasmMemoryMB],
  heapHighWaterLastMB: last.heapHighWaterMB,
  elementsFirstLast: [result.onePane.elements, last.elements],
  canvasesFirstLast: [result.onePane.canvases, last.canvases],
  sessionsLiveLast: last.sessionsLive,
  workersLiveLast: last.workersLive,
};
console.log(`RESULT ${JSON.stringify(result)}`);
await browser.close();
