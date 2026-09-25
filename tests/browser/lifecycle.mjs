// The browser-level Phase 0 lifecycle baseline: the REAL reader in a REAL
// browser — the built wasm app, the real pdf.js worker, real renders, the
// real look-ahead, real prefetch — driven through the workload matrix
// `docs/memory-baseline.md` defines: normal lifecycle, large book, fast
// scroll (with the raster bound), zoom, look-ahead, close DURING active
// work (render / prefetch / search — raced, not assumed), rapid reopen.
//
// The engine smoke suite (web lane) proves the engine state machine's
// counters on stubs; THIS proves the whole application drains: reader
// runtime, panes, virtualizers, look-ahead samples, prefetch, and the
// engine's session and worker behind the real pdf.js.
//
// The measurement table this prints (markers below) is the recorded
// baseline: paste it into docs/memory-baseline.md for the environment that
// ran it.
import { chromium } from "playwright";

const BASE = process.env.BASE_URL ?? "http://127.0.0.1:8123";
const PEARLS = "/samples/Programming Pearls (2nd Edition) - Jon Bentley.pdf";
const DEEP_OUTLINE = "/samples/Deep Outline.pdf";
const pearlsUrl = `${BASE}/?blend=1&open=${encodeURIComponent(PEARLS)}`;
const outlineUrl = `${BASE}?blend=1&open=${encodeURIComponent(DEEP_OUTLINE)}`;

// The bounded-surface policy the workloads assert against, with its source —
// the test enforces the implementation's live numbers, not invented ones:
//   window ceiling    reader_core::view::RENDER_BUDGET = screenfuls(0.5, 3):
//                     at most 3 pages mounted per strip. Read LIVE from every
//                     snapshot as `renderBudgetMaxItems`.
//   zombie retention  src/zoom/config.rs MAX_ZOMBIES = 12, grace 120 ms —
//                     items evicted mid-fling stay mounted briefly; a
//                     transient allowance that must expire by settle time.
//   page lane         public/engine/renderer.ts PAGE_RENDER_LIMIT = 2 slots;
//                     renders execute inside a slot, so activeRenders and
//                     pageActive are hard-bounded by it.
//   thumbnail lane    public/engine/thumbnails.ts THUMB_RENDER_LIMIT = 3.
// Peak vs settled bounds are DIFFERENT and both are policy-derived:
//   hosts at peak   = window ceiling + zombie cap — a host stays registered
//                     while its item rides out the retention grace, so a
//                     swapped window plus the grace population is the legal
//                     maximum (15); once settled, exactly the ceiling (3).
//   retention peak  = the zombie cap is PER STRIP and this workload can
//                     legitimately hold zombies on up to three strips (page,
//                     horizontal, thumbnails) across a document swap, so 36
//                     at peak and ZERO at settle.
// documentPages is a different number entirely (the fixture's real page
// count) and is gated separately.
const MAX_ZOMBIES = 12;
const PAGE_LANE_SLOTS = 2;
const THUMB_LANE_SLOTS = 3;
// Both workhorse fixtures ship 40 pages (verified against the committed
// PDFs). A distant jump must cross >= 12x the window budget, and the 16
// thumb warmup needs a book of this size to be meaningful.
const MIN_FIXTURE_PAGES = 40;

const pageErrors = [];
const consoleLog = [];
const badResponses = [];
const failedRequests = [];
let currentStage = "boot";

const browser = await chromium.launch();
const context = await browser.newContext({ viewport: { width: 1400, height: 900 } });
const page = await context.newPage();
page.on("pageerror", (e) => {
  // Full stack: a wasm trap's frames name the glue wrapper it came
  // through, which is the difference between "somewhere in the binary"
  // and a fixable entry point. The stage tag orders it against the run.
  pageErrors.push(`[stage: ${currentStage}] ${e.message}\n${e.stack ?? "(no stack)"}`);
  consoleLog.push(`[err-capture] stage=${currentStage} ${e.message}`);
});
const errorLog = [];
// Trap-hunt ring: every console line, wide, dumped only on failure so the
// statements around a wasm trap survive the run's noise.
const huntLog = [];
page.on("console", (msg) => {
  const line = `[${msg.type()}] ${msg.text()}`;
  consoleLog.push(line);
  huntLog.push(`[${currentStage}] ${line}`);
  if (huntLog.length > 6000) huntLog.shift();
  if (consoleLog.length > 400) consoleLog.shift();
  // A release wasm panic routes through console_error_panic_hook — the one
  // line that names the file and function. Keep it out of the sliding
  // window's reach.
  if (msg.type() === "error") {
    errorLog.push(`[stage: ${currentStage}] ${line}`);
    if (errorLog.length > 50) errorLog.shift();
  }
});
page.on("response", (res) => {
  if (res.status() >= 400) badResponses.push(`${res.status()} ${res.url()}`);
});
page.on("requestfailed", (req) => {
  // A close that races in-flight work aborts the document's open range
  // fetch (net::ERR_ABORTED) — that is the race WORKING, not a failure.
  failedRequests.push(`${req.failure()?.errorText ?? "?"} ${req.url()}`);
});

/** One diagnostics snapshot (the dev probe the app installs at boot), plus
 *  the browser-side memory categories. They are kept separate ON PURPOSE —
 *  none of them is "the browser's total RAM", and each answers a different
 *  question:
 *    wasmHeapBytes   — the wasm LINEAR memory (Rust-side allocations).
 *    liveCanvasBytes — backing stores of live <canvas> elements: raster
 *                      surfaces the wasm heap cannot see. This is the
 *                      category the original fast-scroll complaint was
 *                      about, not a footprint total.
 *    jsHeapBytes     — the browser-reported JS heap (Chromium's
 *                      performance.memory), best-effort: null wherever the
 *                      environment does not provide it. */
async function snap() {
  return page.evaluate(() => {
    const raw = window.__mareaderDiagnostics?.();
    if (!raw) return null;
    const s = JSON.parse(raw);
    let liveCanvasBytes = 0;
    for (const c of document.querySelectorAll("canvas")) {
      liveCanvasBytes += c.width * c.height * 4;
    }
    s.liveCanvasBytes = liveCanvasBytes;
    s.jsHeapBytes = performance.memory?.usedJSHeapSize ?? null;
    return s;
  });
}

// --- Peak sampling ---------------------------------------------------------
// The settled snapshot at the end of a workload cannot see a burst that came
// and went while it was moving; every workload therefore samples DENSELY
// while it runs and asserts the MAXIMA against the policy constants above.
const PEAK_KEYS = [
  "enginePages", "activeRenders", "pageActive", "pageQueue",
  "thumbActive", "thumbQueue", "retainedVirtualItems", "liveWindowItems",
  "lookaheadSamplesActive", "liveCanvasBytes", "wasmHeapBytes", "jsHeapBytes",
  "pageCanvasBytesEst", "thumbnailRasterBytesEst", "rawRetentionBytesEst",
  "pooledIntermediateBytesEst",
];

function newPeaks() {
  return Object.fromEntries(PEAK_KEYS.map((k) => [k, 0]));
}

function samplePeaks(peaks, s) {
  if (!s) return;
  const e = s.engine ?? {};
  const values = {
    enginePages: e.pages ?? 0,
    activeRenders: e.activeRenders ?? 0,
    pageActive: e.pageActive ?? 0,
    pageQueue: e.pageQueue ?? 0,
    thumbActive: e.thumbActive ?? 0,
    thumbQueue: e.thumbQueue ?? 0,
    retainedVirtualItems: s.retainedVirtualItems ?? 0,
    liveWindowItems: s.liveWindowItems ?? 0,
    lookaheadSamplesActive: s.lookaheadSamplesActive ?? 0,
    liveCanvasBytes: s.liveCanvasBytes ?? 0,
    wasmHeapBytes: s.wasmHeapBytes ?? 0,
    jsHeapBytes: s.jsHeapBytes ?? 0,
    pageCanvasBytesEst: s.engine?.pageCanvasBytesEst ?? 0,
    thumbnailRasterBytesEst: s.engine?.thumbnailRasterBytesEst ?? 0,
    rawRetentionBytesEst: s.engine?.rawRetentionBytesEst ?? 0,
    pooledIntermediateBytesEst: s.engine?.pooledIntermediateBytesEst ?? 0,
  };
  for (const k of PEAK_KEYS) peaks[k] = Math.max(peaks[k], values[k]);
}

/** The bounded-surface policy, asserted on the PEAKS a workload observed —
 *  so "mount everything the jump flew over, release it, look innocent at
 *  settle" cannot pass: the burst itself is what fails here. */
function assertSurfacePolicy(label, peaks, windowCeiling) {
  if (peaks.activeRenders > PAGE_LANE_SLOTS) {
    throw new Error(`[${label}] peak ${peaks.activeRenders} active renders exceeds the ${PAGE_LANE_SLOTS}-slot page lane`);
  }
  if (peaks.pageActive > PAGE_LANE_SLOTS) {
    throw new Error(`[${label}] peak ${peaks.pageActive} running lane slots exceeds the ${PAGE_LANE_SLOTS}-slot page lane`);
  }
  if (peaks.thumbActive > THUMB_LANE_SLOTS) {
    throw new Error(`[${label}] peak ${peaks.thumbActive} thumbnail renders exceeds the ${THUMB_LANE_SLOTS}-slot thumbnail lane`);
  }
  const hostPeakBound = windowCeiling + MAX_ZOMBIES;
  if (peaks.enginePages > hostPeakBound) {
    throw new Error(`[${label}] peak ${peaks.enginePages} page hosts exceeds window ${windowCeiling} + zombie cap ${MAX_ZOMBIES} — skipped pages were being rasterised`);
  }
  const retentionPeakBound = MAX_ZOMBIES * 3;
  if (peaks.retainedVirtualItems > retentionPeakBound) {
    throw new Error(`[${label}] peak ${peaks.retainedVirtualItems} retained virtual items exceeds the per-strip zombie cap ${MAX_ZOMBIES} x 3 strips`);
  }
}

function dumpDiagnosis(lastSnapshot) {
  console.error("--- trap hunt ring (full) ---");
  console.error(huntLog.join("\n"));
  console.error("--- last snapshot ---");
  console.error(JSON.stringify(lastSnapshot, null, 2));
  console.error("--- page errors ---");
  console.error(pageErrors.join("\n") || "(none)");
  console.error("--- responses >= 400 ---");
  console.error(badResponses.join("\n") || "(none)");
  console.error("--- failed requests ---");
  console.error(failedRequests.join("\n") || "(none)");
  console.error("--- error-level console (all kept) ---");
  console.error(errorLog.join("\n") || "(none)");
  console.error("--- console (last 60) ---");
  console.error(consoleLog.slice(-60).join("\n") || "(none)");
}

async function waitFor(label, predicate, timeoutMs = 120_000) {
  const started = Date.now();
  for (;;) {
    const s = await snap();
    if (s && predicate(s)) return s;
    if (Date.now() - started > timeoutMs) {
      dumpDiagnosis(s);
      // Piped stdout drains asynchronously; give the dump a moment before
      // the throw or the process exit truncates it.
      await page.waitForTimeout(1_000);
      throw new Error(`timed out waiting for ${label}`);
    }
    await page.waitForTimeout(250);
  }
}

async function openBook(url) {
  await page.goto(url, { waitUntil: "domcontentloaded" });
  // Reader live AND the document actually open AND the first page rendered
  // AND the runtime itself reporting Ready (it publishes its own lifecycle).
  return waitFor("the reader to open and first-render", (s) =>
    s.readerRuntimeLive === true &&
    s.runtime?.state === "ready" &&
    (s.runtime?.generation ?? 0) >= 1 &&
    s.engine?.hasDocument === true &&
    s.engine.sessionsOpened >= 1 &&
    s.engine.rendersCompleted >= 1 &&
    s.engine.activeRenders === 0);
}

/** Wait for the warmup's prefetch fire to pass through the thumbnail lane.
 *  HARD on this fixture: a 40+ page book always warms 16 thumbs, so a run
 *  where the warmup never starts is a failure, not a skip. */
async function waitForWarmup() {
  const started = await waitFor("thumbnail warmup to start", (s) =>
    s.engine.prefetchesStarted >= 1, 25_000);
  if (started.engine.prefetchesStarted < 1) {
    throw new Error("warmup never started on a fixture that guarantees it");
  }
  return waitFor("the warmup prefetches to drain", (s) =>
    s.engine.activePrefetches === 0, 40_000);
}

async function clickCloseNow() {
  // The toolbar sits under the window drag-region overlay (window chrome
  // that only means anything inside Tauri), so a hit-tested click is
  // swallowed in a plain browser. Dispatch on the button itself: same
  // handler, same close path the packaged app runs.
  await page.evaluate(() => {
    const btn = document.querySelector('button[title*="Close this book"]');
    if (!btn) throw new Error("close button not found");
    btn.click();
  });
}

/** The disposal baseline after a close, with every pairing asserted.
 *  Every cycle here boots fresh, so the open claims epoch 1 and the close
 *  claims epoch 2 — exactly one advance per cycle. */
async function closeAndWaitBaseline(label, settledWork, expectedEpoch = 2) {
  if (!settledWork) await clickCloseNow();
  const s = await waitFor(`the disposal baseline (${label})`, (x) =>
    x.atBaseline === true && x.runtime?.state === "disposed", 45_000);
  assertDrained(s, label, expectedEpoch);
  // The runtime itself reports disposal completion (Phase 1 §12): state
  // Disposed with a generation stamp — disposal is owned, not inferred.
  if (s.runtime?.state !== "disposed" || (s.runtime?.generation ?? 0) < 1) {
    throw new Error(`[${label}] runtime did not report disposal (state ${s.runtime?.state}, generation ${s.runtime?.generation})`);
  }
  return s;
}

function assertDrained(s, label, expectedEpoch = 2) {
  if (!s.atBaseline) throw new Error(`[${label}] baseline not reached`);
  if (s.readerRuntimeLive !== false) throw new Error(`[${label}] reader runtime still live`);
  if (s.engine.hasDocument !== false) throw new Error(`[${label}] engine still holds a document`);
  // FAIL CLOSED on accounting: `paneLive` derives from saturating
  // subtraction, so a double dispose would read as a quiet zero. The
  // cumulative pairs must balance exactly and the live counts must equal
  // created-minus-disposed — "actually zero" and "accounting broke" are
  // different answers and only the first may pass.
  if (s.accountingConsistent === false) {
    throw new Error(`[${label}] create/dispose accounting inconsistent (a dispose exceeded its create)`);
  }
  if (s.panesCreated !== s.panesDisposed) {
    throw new Error(`[${label}] panes created ${s.panesCreated} != disposed ${s.panesDisposed}`);
  }
  if (s.virtualizersCreated !== s.virtualizersDisposed) {
    throw new Error(`[${label}] virtualizers created ${s.virtualizersCreated} != disposed ${s.virtualizersDisposed}`);
  }
  if (s.paneLive !== s.panesCreated - s.panesDisposed) {
    throw new Error(`[${label}] live panes ${s.paneLive} != created - disposed (${s.panesCreated - s.panesDisposed})`);
  }
  if (s.virtualizerLive !== s.virtualizersCreated - s.virtualizersDisposed) {
    throw new Error(`[${label}] live virtualizers ${s.virtualizerLive} != created - disposed (${s.virtualizersCreated - s.virtualizersDisposed})`);
  }
  if (s.disposalEpoch !== expectedEpoch) {
    throw new Error(`[${label}] disposal epoch ${s.disposalEpoch}, expected ${expectedEpoch} (one claim per open and per close)`);
  }
  // The engine's per-document raster categories must be EMPTY in bytes, not
  // just in counts — the byte estimate is what a released-but-unshrunk
  // surface would hide. (The recycler is module-bounded, not per-document;
  // the same-page workload gates its per-cycle drift instead.)
  if (s.engine.pageCanvasBytesEst !== 0) {
    throw new Error(`[${label}] page render surfaces still hold ${s.engine.pageCanvasBytesEst} bytes`);
  }
  if (s.engine.thumbnailRasterBytesEst !== 0) {
    throw new Error(`[${label}] thumbnail rasters still hold ${s.engine.thumbnailRasterBytesEst} bytes`);
  }
  if (s.engine.rawRetentionBytesEst !== 0) {
    throw new Error(`[${label}] retained raws still hold ${s.engine.rawRetentionBytesEst} bytes`);
  }
  if (s.engine.sessionsOpened !== s.engine.sessionsDestroyed) {
    throw new Error(`[${label}] session counters unbalanced`);
  }
  if (s.engine.workersCreated !== s.engine.workersTerminated) {
    throw new Error(`[${label}] worker counters unbalanced (teardown not awaited?)`);
  }
  if (s.engine.rendersStarted !==
      s.engine.rendersCompleted + s.engine.rendersCancelled + s.engine.rendersFailed) {
    throw new Error(`[${label}] render counters unbalanced after close`);
  }
  if (s.engine.prefetchesStarted !==
      s.engine.prefetchesCompleted + s.engine.prefetchesDropped) {
    throw new Error(`[${label}] prefetch counters unbalanced after close`);
  }
  if (s.lookaheadSamplesActive !== 0) {
    throw new Error(`[${label}] look-ahead samples survived the close`);
  }
  // The lanes must be EMPTY, not merely quiet: queued closures retain
  // canvases, scales and caller resolvers across the dispose.
  if (s.engine.pageQueue !== 0 || s.engine.pageActive !== 0) {
    throw new Error(`[${label}] page lane not drained (queue ${s.engine.pageQueue}, active ${s.engine.pageActive})`);
  }
  if (s.engine.thumbQueue !== 0 || s.engine.thumbActive !== 0) {
    throw new Error(`[${label}] thumbnail lane not drained (queue ${s.engine.thumbQueue}, active ${s.engine.thumbActive})`);
  }
  if (s.engine.searchActive !== 0) {
    throw new Error(`[${label}] search build survived the close`);
  }
}

/** Observe-and-close in ONE js turn: the close lands microseconds after the
 *  activity check, so a caught render/prefetch cannot settle in between.
 *  This is what makes "close during active work" a manufactured race
 *  instead of a hope. */
async function raceCloseDuringRender() {
  return page.evaluate(() => {
    const raw = window.__mareaderDiagnostics?.();
    if (!raw) return false;
    const s = JSON.parse(raw);
    if (s.engine.activeRenders > 0) {
      const btn = document.querySelector('button[title*="Close this book"]');
      if (btn) { btn.click(); return true; }
    }
    return false;
  });
}

async function raceCloseDuringPrefetch() {
  return page.evaluate(() => {
    const raw = window.__mareaderDiagnostics?.();
    if (!raw) return false;
    const s = JSON.parse(raw);
    if (s.engine.activePrefetches > 0) {
      const btn = document.querySelector('button[title*="Close this book"]');
      if (btn) { btn.click(); return true; }
    }
    return false;
  });
}

const stages = {};
const summary = {
  fixturePages: {},
  fastJumpRenderDeltas: [],
  scrollPeaks: null,
  zoomPeaks: null,
  fastJumpPeaks: null,
  largeScrollPeaks: null,
  closeDuringRenderRaced: false,
  closeDuringPrefetchDrops: 0,
  closeDuringSearchRaced: false,
  rapidReopenHeaps: [],
  rapidReopenSlopeBytesPerCycle: null,
  rapidReopenDriftBytes: null,
  normalCycles: 0,
  largeCycles: 0,
  rapidCycles: 0,
  samePageOpenHeaps: [],
  samePagePooledBytes: [],
  samePageSlopeBytesPerCycle: null,
  samePageDriftBytes: null,
  samePageCycles: 0,
};

// --- Stage 1: boot + open a real book -------------------------------------
currentStage = "stage1-open";
const opened = await openBook(pearlsUrl);
stages.afterOpen = opened;
if (opened.disposalEpoch < 1) throw new Error("open did not claim the document state");
// The workload gate: documentPages is the fixture's REAL page count (the
// document proxy's numPages), not engine.pages (registered page hosts). A
// book too small to jump across would make the fast-scroll policy trivially
// untestable.
if (((opened.engine ?? {}).documentPages ?? 0) < MIN_FIXTURE_PAGES) {
  throw new Error(`fixture has ${opened.engine?.documentPages} pages, need >= ${MIN_FIXTURE_PAGES} for the jump workload`);
}
summary.fixturePages.pearls = opened.engine.documentPages;
console.log("opened; epoch:", opened.disposalEpoch,
  "| documentPages:", opened.engine.documentPages, "| hosts:", opened.engine.pages,
  "| heap:", opened.wasmHeapBytes);

// --- Stage 2: the warmup prefetch (hard: this fixture guarantees it) ------
currentStage = "stage2-warmup";
const warmed = await waitForWarmup();
stages.afterWarmup = warmed;
if (warmed.engine.prefetchesCompleted !== warmed.engine.prefetchesStarted) {
  throw new Error("warmup prefetches did not all complete");
}
console.log("warmup drained:", warmed.engine.prefetchesCompleted, "prefetches");

// --- Stage 3: pressure — fast navigation, look-ahead, zoom ----------------
currentStage = "stage3-scroll-zoom";
// The scroll surface must own the input: one click into the page area puts
// the viewer in front of the keyboard and the wheel, exactly where a real
// reading session starts from.
await page.mouse.click(700, 450);
await page.waitForTimeout(120);
const scrollStart = await snap();
const WINDOW_CEILING = scrollStart.renderBudgetMaxItems ?? 3;
let sawLookahead = false;
let maxRenders = scrollStart.engine.rendersStarted;
const scrollPeaks = newPeaks();
for (let i = 0; i < 24; i += 1) {
  await page.keyboard.press("PageDown");
  await page.mouse.move(700, 450);
  await page.mouse.wheel(0, 2200);
  // Fast flicks with a reader's micro-pauses: the virtualizer mounts page
  // hosts during the flicks and the render lane drains on the pauses (a
  // continuous synthetic storm never yields, so nothing would ever raster).
  // A look-ahead sample is live only for a blink, so the pause is watched
  // densely rather than sampled once.
  const settle = i % 5 === 4 ? 700 : 130;
  const deadline = Date.now() + settle;
  let s = null;
  while (Date.now() < deadline) {
    await page.waitForTimeout(40);
    const probe = await snap();
    if (!probe) continue;
    s = probe;
    samplePeaks(scrollPeaks, probe);
    if (probe.lookaheadSamplesActive > 0) sawLookahead = true;
  }
  if (!s) continue;
  maxRenders = Math.max(maxRenders, s.engine.rendersStarted);
  stages.duringScroll = s;
}
assertSurfacePolicy("scroll workload", scrollPeaks, WINDOW_CEILING);
summary.scrollPeaks = scrollPeaks;
if (maxRenders <= scrollStart.engine.rendersStarted) {
  throw new Error("no page raster work happened during the scroll workload");
}
if (!sawLookahead) {
  throw new Error("the look-ahead was never observed active during scroll (blend was on)");
}
// Zoom pressure through the app's real zoom pipeline, ending back at the
// original zoom (workload D: rapid zoom, return near original). Sampled
// through every step: a zoom re-raster is exactly where canvas backing
// storage and the render lane spike.
const zoomPeaks = newPeaks();
for (const key of ["+", "-"]) {
  for (let i = 0; i < 3; i += 1) {
    await page.keyboard.press(key);
    const deadline = Date.now() + 320;
    while (Date.now() < deadline) {
      await page.waitForTimeout(40);
      samplePeaks(zoomPeaks, await snap());
    }
  }
}
stages.afterZoom = await snap();
samplePeaks(zoomPeaks, stages.afterZoom);
assertSurfacePolicy("zoom workload", zoomPeaks, WINDOW_CEILING);
summary.zoomPeaks = zoomPeaks;
const zoomRenders = stages.afterZoom.engine.rendersStarted;
if (zoomRenders <= maxRenders) {
  throw new Error("zoom produced no re-renders");
}

// --- Stage 4: fast-scroll raster bound (the original complaint, measured) -
currentStage = "stage4-fast-jump";
// A distant jump must rasterise the DESTINATION window, not every page it
// flew over: observed jumps cost ~2 renders on this build. The bound pins
// the policy; the later virtualizer phase has to keep or improve it.
await page.mouse.click(700, 450);
const jumpTargets = ["end", "top", "quarter"];
const jumpPeaks = newPeaks();
const DOC_PAGES = summary.fixturePages.pearls;
for (const where of jumpTargets) {
  const beforeSnap = await snap();
  const before = beforeSnap.engine.rendersStarted;
  const fromPage = beforeSnap.readerPage;
  // A new measurement generation: every raster the engine starts from now
  // carries this id, so the trace assertion reads exactly this jump.
  const gen = await page.evaluate(() => window.PDFReader.beginRenderGeneration());
  await page.evaluate((w) => {
    const list = document.querySelector("#page-list");
    list.scrollTop = w === "end" ? list.scrollHeight
      : w === "top" ? 0
      : list.scrollHeight / 4;
  }, where);
  // The burst IS the measurement: sample the first 600 ms densely (that is
  // where a broken skip policy would rasterise the ~37 pages this jump
  // crosses), then give the settle policy its full window before the
  // settled assertions.
  const peakDeadline = Date.now() + 600;
  while (Date.now() < peakDeadline) {
    await page.waitForTimeout(30);
    samplePeaks(jumpPeaks, await snap());
  }
  await page.waitForTimeout(1_000);
  const after = await snap();
  samplePeaks(jumpPeaks, after);
  const delta = after.engine.rendersStarted - before;
  summary.fastJumpRenderDeltas.push(delta);
  stages.duringFastJump = after;
  if (delta < 1) throw new Error(`fast jump to ${where} rendered nothing`);
  // The jump's cost is the DESTINATION window, not the ~37 pages it flew
  // over: ceiling + one swap of overlap.
  if (delta > WINDOW_CEILING * 2) {
    throw new Error(`fast jump to ${where} rasterised ${delta} pages — the skip policy is gone (bound ${WINDOW_CEILING * 2})`);
  }
  // Settled: hosts back to the window ceiling, retention expired.
  if (after.engine.pages > WINDOW_CEILING) {
    throw new Error(`fast jump left ${after.engine.pages} page hosts mounted (window budget ${WINDOW_CEILING})`);
  }
  if (after.retainedVirtualItems !== 0) {
    throw new Error(`fast jump to ${where} left ${after.retainedVirtualItems} retained virtual items after settle`);
  }
  // PAGE-IDENTITY PROOF: every page the engine actually rasterized during
  // this jump generation must sit inside the destination window the policy
  // allows. The allowed range comes from the same policy constants the host
  // bound uses — mounted window ceiling on each side plus the zombie
  // retention cap — not from an invented page count. Render COUNTS cannot
  // catch a skipped page being rasterized (a small burst looks identical);
  // the trace names the pages.
  const trace = await page.evaluate((g) =>
    window.PDFReader.renderTrace().filter((e) => e.gen === g), gen);
  if (trace.length === 0) {
    throw new Error(`fast jump to ${where} produced no traced raster (trace dead?)`);
  }
  const dest = after.readerPage || fromPage;
  const half = WINDOW_CEILING + MAX_ZOMBIES;
  const lo = Math.max(1, dest - half);
  const hi = Math.min(DOC_PAGES, dest + half);
  const strays = [...new Set(trace.filter((e) => e.page < lo || e.page > hi).map((e) => e.page))];
  if (strays.length > 0) {
    throw new Error(`fast jump ${fromPage} -> ${dest} (${where}): unexpected rasterized skipped page: ${strays.join(", ")} (allowed ${lo}..${hi}); render trace: [${trace.map((e) => e.page).join(", ")}]`);
  }
  console.log(`fast jump ${fromPage} -> ${dest} (${where}): traced pages [${[...new Set(trace.map((e) => e.page))].join(", ")}] all inside ${lo}..${hi}`);
}
assertSurfacePolicy("fast jump", jumpPeaks, WINDOW_CEILING);
summary.fastJumpPeaks = jumpPeaks;
console.log("fast-jump render deltas:", summary.fastJumpRenderDeltas.join(", "));
console.log("fast-jump peaks:", JSON.stringify(jumpPeaks));

// --- Stage 5: close DURING an active page render (raced, then proven) -----
currentStage = "stage5-render-race";
// Zoom commits keep the render lane fed; the atomic racer closes the book
// in the same js turn that observes an in-flight render. The interrupted
// work must show up as rendersCancelled/rendersDropped — a close that
// raced nothing proves nothing.
let renderRaceWon = false;
let renderRaceSnapshot = null;
for (let attempt = 1; attempt <= 3 && !renderRaceWon; attempt += 1) {
  if (attempt > 1) {
    console.log(`render race attempt ${attempt}: fresh open`);
    await openBook(pearlsUrl);
    await page.mouse.click(700, 450);
  }
  const deadline = Date.now() + 9_000;
  while (Date.now() < deadline && !renderRaceWon) {
    await page.keyboard.press("+");
    for (let k = 0; k < 5; k += 1) {
      if (await raceCloseDuringRender()) { renderRaceWon = true; break; }
      await page.waitForTimeout(25);
    }
  }
  if (renderRaceWon) {
    renderRaceSnapshot = await waitFor("the disposal baseline (close during render)",
      (x) => x.atBaseline === true, 45_000);
    const e = renderRaceSnapshot.engine;
    if (e.rendersCancelled + e.rendersDropped < 1) {
      throw new Error("close-during-render won the race but no render was cancelled or dropped");
    }
    assertDrained(renderRaceSnapshot, "close during render");
  } else {
    console.log(`render race attempt ${attempt}: never caught an active render; settling and retrying`);
    await closeAndWaitBaseline(`render-race attempt ${attempt} missed; settled close`);
  }
}
if (!renderRaceWon) throw new Error("could not race a close into an active render in 3 attempts");
summary.closeDuringRenderRaced = true;
stages.afterCloseDuringRender = renderRaceSnapshot;
console.log("close-during-render race WON; cancelled+dropped:",
  renderRaceSnapshot.engine.rendersCancelled, "+", renderRaceSnapshot.engine.rendersDropped);

// --- Stage 6: close DURING an active thumbnail prefetch -------------------
currentStage = "stage6-prefetch-race";
// The warmup fires ~1.5s after ready; dense polling catches the first
// prefetch early, and the atomic close lands inside its render window.
let prefetchRaceWon = false;
let prefetchSnapshot = null;
for (let attempt = 1; attempt <= 3 && !prefetchRaceWon; attempt += 1) {
  await openBook(pearlsUrl);
  const deadline = Date.now() + 14_000;
  while (Date.now() < deadline && !prefetchRaceWon) {
    if (await raceCloseDuringPrefetch()) { prefetchRaceWon = true; break; }
    await page.waitForTimeout(20);
  }
  if (prefetchRaceWon) {
    prefetchSnapshot = await waitFor("the disposal baseline (close during prefetch)",
      (x) => x.atBaseline === true, 45_000);
    if (prefetchSnapshot.engine.prefetchesDropped < 1) {
      throw new Error("close-during-prefetch won the race but no prefetch was dropped");
    }
    assertDrained(prefetchSnapshot, "close during prefetch");
  } else {
    console.log(`prefetch race attempt ${attempt}: never caught an active prefetch; settling and retrying`);
    await closeAndWaitBaseline(`prefetch-race attempt ${attempt} missed; settled close`);
  }
}
if (!prefetchRaceWon) throw new Error("could not race a close into an active prefetch in 3 attempts");
summary.closeDuringPrefetchDrops = prefetchSnapshot.engine.prefetchesDropped;
stages.afterCloseDuringPrefetch = prefetchSnapshot;
console.log("close-during-prefetch race WON; prefetches dropped:",
  prefetchSnapshot.engine.prefetchesDropped);

// --- Stage 7: close DURING a search index build (raced, then proven) ------
currentStage = "stage7-search-race";
// The build is search's long async half — a worker round trip per page,
// ~3 pages per turn — and the engine gauges it (searchActive). Same-turn
// observe-and-close like the other races. Each attempt uses a book whose
// index was never built: an adopted index skips extraction, so a retry on
// an already-searched book can never catch the gauge up.
const searchBooks = [pearlsUrl, outlineUrl, `${BASE}?blend=1&open=${encodeURIComponent("/samples/Good Title Book.pdf")}`];
let searchRaceWon = false;
let searchSnapshot = null;
for (let attempt = 1; attempt <= 3 && !searchRaceWon; attempt += 1) {
  await openBook(searchBooks[attempt - 1]);
  await page.mouse.click(700, 450);
  await page.keyboard.press("Control+f");
  const searchBox = page.locator('input[placeholder^="Search in document"]');
  await searchBox.focus();
  await page.keyboard.type("the");
  await page.keyboard.press("Enter");
  const deadline = Date.now() + 12_000;
  while (Date.now() < deadline && !searchRaceWon) {
    searchRaceWon = await page.evaluate(() => {
      const raw = window.__mareaderDiagnostics?.();
      if (!raw) return false;
      const s = JSON.parse(raw);
      if (s.engine.searchActive > 0) {
        const btn = document.querySelector('button[title*="Close this book"]');
        if (btn) { btn.click(); return true; }
      }
      return false;
    });
    if (!searchRaceWon) await page.waitForTimeout(25);
  }
  if (searchRaceWon) {
    searchSnapshot = await waitFor("the disposal baseline (close during search)",
      (x) => x.atBaseline === true, 45_000);
    if (searchSnapshot.engine.searchActive !== 0) {
      throw new Error("close-during-search won the race but the build gauge survived the close");
    }
    assertDrained(searchSnapshot, "close during search");
  } else if (attempt < 3) {
    console.log(`search race attempt ${attempt}: the build finished before the close could land; trying a fresh book`);
    await closeAndWaitBaseline(`search-race attempt ${attempt} missed; settled close`);
  }
}
if (!searchRaceWon) throw new Error("could not race a close into a search build in 3 attempts");
summary.closeDuringSearchRaced = true;
stages.afterCloseDuringSearch = searchSnapshot;
console.log("close-during-search race WON; build gauge drained to 0");

// --- Stage 8: workload A — normal lifecycle x10 ----------------------------
currentStage = "stage8-normal-x10";
for (let cycle = 1; cycle <= 10; cycle += 1) {
  await openBook(pearlsUrl);
  await page.mouse.click(700, 450);
  for (let p = 0; p < 3; p += 1) {
    await page.keyboard.press("PageDown");
    await page.waitForTimeout(150);
  }
  await closeAndWaitBaseline(`normal cycle ${cycle}`);
  summary.normalCycles = cycle;
}
console.log("normal lifecycle x10 drained");

// --- Stage 9: workload B — large PDF lifecycle x5 --------------------------
currentStage = "stage9-large-x5";
const largePeaks = newPeaks();
for (let cycle = 1; cycle <= 5; cycle += 1) {
  const openedLarge = await openBook(outlineUrl);
  if (cycle === 1) {
    if (((openedLarge.engine ?? {}).documentPages ?? 0) < MIN_FIXTURE_PAGES) {
      throw new Error(`large fixture has ${openedLarge.engine?.documentPages} pages, need >= ${MIN_FIXTURE_PAGES}`);
    }
    summary.fixturePages.deepOutline = openedLarge.engine.documentPages;
    console.log("large fixture documentPages:", openedLarge.engine.documentPages);
  }
  await page.mouse.click(700, 450);
  for (let w = 0; w < 4; w += 1) {
    await page.mouse.wheel(0, 2_500);
    const deadline = Date.now() + 400;
    while (Date.now() < deadline) {
      await page.waitForTimeout(40);
      samplePeaks(largePeaks, await snap());
    }
  }
  await page.waitForTimeout(900); // grace period: zombie items must drain
  const settled = await snap();
  samplePeaks(largePeaks, settled);
  if (settled.retainedVirtualItems !== 0) {
    throw new Error(`large cycle ${cycle}: ${settled.retainedVirtualItems} virtual items retained after scroll settled`);
  }
  await closeAndWaitBaseline(`large cycle ${cycle}`);
  summary.largeCycles = cycle;
}
assertSurfacePolicy("large scroll", largePeaks, WINDOW_CEILING);
summary.largeScrollPeaks = largePeaks;
console.log("large PDF lifecycle x5 drained; peaks:", JSON.stringify(largePeaks));

// --- Stage 10: workload G — rapid reopen x10 -------------------------------
currentStage = "stage10-rapid-reopen";
// The wasm heap only ratchets up (the platform never shrinks it); judge
// LEAK versus LATCH by the per-cycle step, so the steps are recorded and
// the level gets a generous ceiling rather than a flatness demand.
for (let cycle = 1; cycle <= 10; cycle += 1) {
  const openedCycle = await openBook(pearlsUrl);
  summary.rapidReopenHeaps.push(openedCycle.wasmHeapBytes);
  await closeAndWaitBaseline(`rapid reopen ${cycle}`);
  summary.rapidCycles = cycle;
}
// The wasm heap never shrinks, so the invariant is NOT "back to first
// reading" — it is: resources disappear every cycle (asserted by the
// per-cycle baselines above) AND no NEW ownership accumulates per cycle.
// Over the ten recorded samples that separates a one-time allocator
// ratchet (one step, then flat) from a per-cycle leak (a sustained climb):
// a least-squares slope over the cycles plus a total-drift ceiling, both
// tolerant of environment noise but both requiring the flat-after-ratchet
// shape.
const heaps = summary.rapidReopenHeaps;
const n = heaps.length;
const meanY = heaps.reduce((a, b) => a + b, 0) / n;
const meanX = (n - 1) / 2;
let cov = 0;
let varX = 0;
heaps.forEach((y, x) => {
  cov += (x - meanX) * (y - meanY);
  varX += (x - meanX) ** 2;
});
const slope = cov / varX;
const drift = Math.max(...heaps) - Math.min(...heaps);
summary.rapidReopenSlopeBytesPerCycle = Math.round(slope);
summary.rapidReopenDriftBytes = drift;
if (slope > 262_144) {
  throw new Error(`rapid reopen climbs ~${Math.round(slope)} bytes/cycle on average — a per-cycle leak, not a one-time ratchet`);
}
if (drift > 4_194_304) {
  throw new Error(`rapid reopen drifted ${drift} bytes across ${n} cycles (4 MB ceiling) — sustained accumulation`);
}
console.log("rapid reopen heaps:", heaps.join(", "));
console.log(`rapid reopen trend: slope ${Math.round(slope)} B/cycle, drift ${drift} B`);
stages.afterRapidReopen = await snap();

// --- Stage 11: workload H — SAME-PAGE lifecycle x10 (no reload) ------------
currentStage = "stage11-same-page-x10";
// Stages 8-10 proved the RELOAD matrix: every cycle began with page.goto,
// which discards the whole JS/wasm world. The resources that live at
// APPLICATION scope — the wasm module and its heap ratchet, the engine's
// raster recycler, the library's caches, the disposal epoch — never felt a
// cycle. This workload rides the real application path instead: click the
// book open from the library (the row the reader recorded on open), work
// the pages, click the toolbar close, and repeat in the SAME live page.
// Every close must still return every reader-owned resource to baseline,
// and the epoch must advance exactly one claim per open and one per close
// (2k-1 after open k, 2k after close k) — "every cycle starts from epoch 1"
// is a reload artifact this stage exists to stop assuming.
async function openFromLibrary(cycle) {
  const card = page.locator('.book-title[title*="Programming Pearls"]').first();
  try {
    await card.click({ timeout: 5_000 });
  } catch {
    // The grid's gesture layer can swallow a synthetic hit; dispatching on
    // the row is the same app open path either way.
    await page.evaluate(() => {
      const t = [...document.querySelectorAll(".book-title")]
        .find((el) => (el.textContent ?? "").includes("Programming Pearls"));
      if (!t) throw new Error("book row not found in the library");
      t.click();
    });
  }
  const o = await waitFor(`same-page open ${cycle}`, (x) =>
    x.readerRuntimeLive === true &&
    x.engine?.hasDocument === true &&
    x.engine.activeRenders === 0, 45_000);
  return o;
}
// The base is whatever the reload matrix left: the epoch is claimed per
// open and per close in the LIVE page, so the same-page cycles assert the
// DELTA — exactly one claim each — never an absolute "back to 1".
const epochBase = (await snap()).disposalEpoch;
// The runtime generation base: every same-page open must be a NEW runtime
// generation (a disposed runtime is never revived in place), so generation
// advances exactly once per cycle on top of wherever the matrix left it.
const generationBase = (await snap()).runtime?.generation ?? 1;
console.log("same-page stage: epoch base", epochBase, "| runtime generation base", generationBase);
for (let cycle = 1; cycle <= 10; cycle += 1) {
  const o = await openFromLibrary(cycle);
  if (o.disposalEpoch !== epochBase + 2 * cycle - 1) {
    throw new Error(`same-page open ${cycle}: epoch ${o.disposalEpoch}, expected ${epochBase + 2 * cycle - 1} (base ${epochBase} + one open claim)`);
  }
  if (o.runtime?.generation !== generationBase + cycle) {
    throw new Error(`same-page open ${cycle}: runtime generation ${o.runtime?.generation}, expected a NEW runtime (${generationBase + cycle}) — the disposed runtime was revived`);
  }
  if (((o.engine ?? {}).documentPages ?? 0) < MIN_FIXTURE_PAGES) {
    throw new Error(`same-page cycle ${cycle}: fixture has ${o.engine.documentPages} pages`);
  }
  summary.samePageOpenHeaps.push(o.wasmHeapBytes);
  await page.mouse.click(700, 450);
  for (let p = 0; p < 2; p += 1) {
    await page.keyboard.press("PageDown");
    await page.waitForTimeout(150);
  }
  await clickCloseNow();
  const c = await waitFor(`the disposal baseline (same-page cycle ${cycle})`,
    (x) => x.atBaseline === true, 45_000);
  assertDrained(c, `same-page cycle ${cycle}`, epochBase + 2 * cycle);
  if (c.runtime?.state !== "disposed" || c.runtime?.generation !== generationBase + cycle) {
    throw new Error(`same-page cycle ${cycle}: runtime ${c.runtime?.state} gen ${c.runtime?.generation}, expected disposed at generation ${generationBase + cycle}`);
  }
  // The raster recycler is module-bounded, not document-owned: it may hold
  // its placeholders across cycles, but a close must not make it GROW.
  summary.samePagePooledBytes.push(c.engine.pooledIntermediateBytesEst ?? 0);
  summary.samePageCycles = cycle;
}
stages.afterSamePage = await snap();
{
  const heaps = summary.samePageOpenHeaps;
  const n = heaps.length;
  const meanY = heaps.reduce((a, b) => a + b, 0) / n;
  const meanX = (n - 1) / 2;
  let cov = 0;
  let varX = 0;
  heaps.forEach((y, x) => {
    cov += (x - meanX) * (y - meanY);
    varX += (x - meanX) ** 2;
  });
  const slope = cov / varX;
  const drift = Math.max(...heaps) - Math.min(...heaps);
  summary.samePageSlopeBytesPerCycle = Math.round(slope);
  summary.samePageDriftBytes = drift;
  if (slope > 262_144) {
    throw new Error(`same-page reopen climbs ~${Math.round(slope)} bytes/cycle — app-scope accumulation, not a one-time ratchet`);
  }
  if (drift > 4_194_304) {
    throw new Error(`same-page reopen drifted ${drift} bytes across ${n} cycles (4 MB ceiling)`);
  }
  const pooledDrift = Math.max(...summary.samePagePooledBytes)
    - Math.min(...summary.samePagePooledBytes);
  if (pooledDrift > 1_048_576) {
    throw new Error(`the engine raster recycler drifted ${pooledDrift} bytes across ${n} same-page closes — per-cycle growth`);
  }
  console.log("same-page lifecycle x10 drained (no reload); heaps:", heaps.join(", "));
  console.log(`same-page trend: slope ${Math.round(slope)} B/cycle, drift ${drift} B | recycler drift ${pooledDrift} B`);
}

// --- Guards ----------------------------------------------------------------
if (pageErrors.length > 0) {
  dumpDiagnosis(await snap().catch(() => null));
  throw new Error(`page errors during the lifecycle:\n${pageErrors.join("\n")}`);
}
if (badResponses.length > 0) {
  console.warn("non-2xx responses:", badResponses);
}
if (failedRequests.length > 0) {
  console.warn("aborted in-flight requests (expected from raced closes):", failedRequests.length);
}

await browser.close();

// --- The recorded measurement table ---------------------------------------
console.log("\n=== PHASE0 BROWSER BASELINE (chromium, release wasm build) ===");
console.log("policy: window ceiling", WINDOW_CEILING,
  "| zombie cap", MAX_ZOMBIES,
  "| page lane", PAGE_LANE_SLOTS,
  "| thumb lane", THUMB_LANE_SLOTS,
  "| min fixture pages", MIN_FIXTURE_PAGES);
for (const [stage, s] of Object.entries(stages)) {
  console.log(`--- ${stage} ---`);
  console.log(JSON.stringify({
    disposalEpoch: s.disposalEpoch,
    readerPage: s.readerPage,
    readerRuntimeLive: s.readerRuntimeLive,
    runtime: s.runtime,
    paneLive: s.paneLive,
    virtualizerLive: s.virtualizerLive,
    virtualizerListeners: s.virtualizerListeners,
    virtualizerObservers: s.virtualizerObservers,
    virtualizerTimers: s.virtualizerTimers,
    liveWindowItems: s.liveWindowItems,
    retainedVirtualItems: s.retainedVirtualItems,
    lookaheadSamplesActive: s.lookaheadSamplesActive,
    engine: s.engine,
    wasmHeapBytes: s.wasmHeapBytes,
    heapHighWaterBytes: s.heapHighWaterBytes,
    liveCanvasBytes: s.liveCanvasBytes,
    jsHeapBytes: s.jsHeapBytes,
    atBaseline: s.atBaseline,
  }));
}
console.log("--- summary ---");
console.log(JSON.stringify(summary));
// One machine-readable line for CI/tooling; the table above stays the
// human-readable record.
console.log("PHASE0_BASELINE_JSON " + JSON.stringify(summary));
console.log("=== END PHASE0 BROWSER BASELINE ===");
console.log("\nBROWSER LIFECYCLE BASELINE PASSED");
