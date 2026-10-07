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
import { verifyPaneRuntimes } from "./pane-runtimes.mjs";
import { verifyCleanupRuntime } from "./cleanup-runtime.mjs";
import { verifyWindowState } from "./window-state.mjs";

const BASE = process.env.BASE_URL ?? "http://127.0.0.1:8123";
const PEARLS = "/samples/Programming Pearls (2nd Edition) - Jon Bentley.pdf";
const DEEP_OUTLINE = "/samples/Deep Outline.pdf";
const pearlsUrl = `${BASE}/?blend=1&open=${encodeURIComponent(PEARLS)}`;
const outlineUrl = `${BASE}?blend=1&open=${encodeURIComponent(DEEP_OUTLINE)}`;

// The bounded-surface policy the workloads assert against, with its source —
// the test enforces the implementation's live numbers, not invented ones:
//   window ceiling    reader_runtime::features::virtualizers::RENDER_BUDGET =
//                     screenfuls(0.5, 3): at most 3 pages mounted per strip. Read
//                     LIVE from every snapshot as `renderBudgetMaxItems`.
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

process.on("unhandledRejection", async (err) => {
  console.error("=== UNHANDLED REJECTION ===");
  console.error(err?.stack ?? String(err));
  try { dumpDiagnosis(lastSnap); } catch (_) {}
  process.exit(1);
});
process.on("uncaughtException", async (err) => {
  console.error("=== UNCAUGHT EXCEPTION ===");
  console.error(err?.stack ?? String(err));
  try { dumpDiagnosis(lastSnap); } catch (_) {}
  process.exit(1);
});

const pageErrors = [];
const consoleLog = [];
const badResponses = [];
const failedRequests = [];
let currentStage = "boot";

const browser = await chromium.launch();
const context = await browser.newContext({ viewport: { width: 1400, height: 900 } });
const page = await context.newPage();
// TEST-ONLY query adapter over REAL route and document iframes. Older
// single-pane assertions address one surface; queries compose the actual
// host document and visible child documents without inventing div/iframe
// facades in production. Frame identities, resources and teardown remain real.
await context.addInitScript(() => {
  if (window !== window.top) return;
  const panes = (slot) =>
    [...slot.querySelectorAll("iframe.pane-frame")]
      .filter((f) => !f.hasAttribute("data-frame-hidden") && f.contentDocument);
  // `[data-pane-id="N"] <rest>` crosses the pane boundary: the entry is the
  // Shell's, what it holds is the pane frame's.
  const scoped = (slot, sel) => {
    const m = /^(\[data-pane-id="[^"]+"\])\s+(.+)$/.exec(sel);
    const entry = m && slot.querySelector(m[1]);
    return entry ? { scope: entry, rest: m[2] } : null;
  };
  const one = (slot, sel) => {
    const s = scoped(slot, sel);
    if (s) return one(s.scope, s.rest);
    return slot.querySelector(sel) ??
      panes(slot).map((f) => f.contentDocument.querySelector(sel)).find(Boolean) ??
      null;
  };
  const all = (slot, sel) => {
    const s = scoped(slot, sel);
    if (s) return all(s.scope, s.rest);
    return [
      ...slot.querySelectorAll(sel),
      ...panes(slot).flatMap((f) => [...f.contentDocument.querySelectorAll(sel)]),
    ];
  };
  const fromPoint = (doc, x, y) => {
    const el = doc.elementFromPoint(x, y);
    if (el?.tagName === "IFRAME" && el.classList.contains("pane-frame") && el.contentDocument) {
      const r = el.getBoundingClientRect();
      return el.contentDocument.elementFromPoint(x - r.left, y - r.top);
    }
    return el;
  };
  const view = (slot) =>
    new Proxy(slot, {
      get(target, key) {
        if (key === "querySelector") return (sel) => one(slot, sel);
        if (key === "querySelectorAll") return (sel) => all(slot, sel);
        if (key === "getElementById") return (id) => one(slot, `#${CSS.escape(id)}`);
        if (key === "elementFromPoint") return (x, y) => fromPoint(slot, x, y);
        const value = Reflect.get(target, key);
        return typeof value === "function" ? value.bind(target) : value;
      },
    });
  const realm = (slot, nativeWindow) =>
    new Proxy(nativeWindow, {
      get(target, key) {
        if (!(key in target)) {
          const pane = panes(slot).find((f) => key in f.contentWindow);
          if (pane) {
            const value = pane.contentWindow[key];
            return typeof value === "function" ? value.bind(pane.contentWindow) : value;
          }
        }
        const value = Reflect.get(target, key);
        return typeof value === "function" ? value.bind(target) : value;
      },
    });
  const isRuntime = (el) => el.classList?.contains("runtime-frame");
  const nativeDocument = Object.getOwnPropertyDescriptor(HTMLIFrameElement.prototype, "contentDocument").get;
  const nativeWindow = Object.getOwnPropertyDescriptor(HTMLIFrameElement.prototype, "contentWindow").get;
  Object.defineProperty(HTMLIFrameElement.prototype, "contentDocument", {
    configurable: true,
    get() {
      const doc = nativeDocument.call(this);
      return isRuntime(this) && doc ? view(doc) : doc;
    },
  });
  Object.defineProperty(HTMLIFrameElement.prototype, "contentWindow", {
    configurable: true,
    get() {
      const win = nativeWindow.call(this);
      const doc = nativeDocument.call(this);
      return isRuntime(this) && win && doc ? realm(doc, win) : win;
    },
  });
});
page.on("pageerror", (e) => {
  // Full stack: a wasm trap's frames name the glue wrapper it came
  // through, which is the difference between "somewhere in the binary"
  // and a fixable entry point. The stage tag orders it against the run.
  pageErrors.push(`[stage: ${currentStage}] ${e.message}\n${e.stack ?? "(no stack)"}`);
  consoleLog.push(`[err-capture] stage=${currentStage} ${e.message}`);
});
// The class this suite exists to catch: a frame, timer or listener firing
// after its reactive owner was disposed (or any trap that escapes the wasm).
const PANIC_CLASS =
  /already been disposed|has been disposed|panicked at|RuntimeError: unreachable|RuntimeError: memory access|recursive use of an object/i;

const errorLog = [];
// Monotonic, unlike the capped ring above it: stages assert "no panic SINCE
// my marker", and a sliding window cannot answer that.
let panicCount = 0;
// Console errors that are NOT the disposal/panic class (a blanket "any error
// fails the stage" gate would red a run over a benign library that logs as
// `error`): counted and reported, never asserted.
let otherErrorCount = 0;
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
    if (PANIC_CLASS.test(line)) panicCount += 1;
    else otherErrorCount += 1;
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
let lastSnap = null;
async function snap() {
  const value = await page.evaluate(() => {
    const raw = window.__mareaderDiagnostics?.();
    if (!raw) return null;
    const s = JSON.parse(raw);
    let liveCanvasBytes = 0;
    // Runtime DOM lives inside the host's frame now: canvases count across
    // both documents (§21's sampling is byte-faithful, not frame-blind).
    const docs = [document];
    // Every frame, not only the visible one: a non-active runtime's canvases
    // are real backing stores even though nothing is showing them, and a
    // byte-faithful memory sample cannot be frame-blind.
    for (const frame of document.querySelectorAll("#runtime-host .runtime-frame")) {
      const route = frame.contentDocument?.defaultView.document;
      if (!route) continue;
      docs.push(route);
      // Actual incoming/retiring document canvases count too. The visible
      // single-pane query adapter must not hide their backing stores.
      for (const pane of route.querySelectorAll("iframe.pane-frame")) {
        if (pane.contentDocument) docs.push(pane.contentDocument);
      }
    }
    for (const d of docs) {
      for (const c of d.querySelectorAll("canvas")) {
        liveCanvasBytes += c.width * c.height * 4;
      }
    }
    s.liveCanvasBytes = liveCanvasBytes;
    s.jsHeapBytes = performance.memory?.usedJSHeapSize ?? null;
    return s;
  });
  if (value) lastSnap = value;
  return value;
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

/** No stage may pass while the page logged a disposal panic. Counter-based, so
 *  a stage asserts "none SINCE my marker" — and a panic is caught by nothing
 *  else: the counters can settle after a trap that killed the owner of a
 *  queued callback, and that trap is the thing being prevented. */
function assertNoNewPanics(label, sinceCount) {
  if (panicCount > sinceCount) {
    throw new Error(`[${label}] ${panicCount - sinceCount} disposal panic(s) during the stage:\n${errorLog.join("\n")}`);
  }
}

async function openBook(url) {
  await page.goto(url, { waitUntil: "domcontentloaded" });
  // Reader live AND the document actually open AND the first page rendered
  // AND the runtime itself reporting Ready (it publishes its own lifecycle).
  const s = await waitFor("the reader to open and first-render", (s) =>
    s.readerRuntimeLive === true &&
    s.runtime?.state === "ready" &&
    (s.runtime?.generation ?? 0) >= 1 &&
    s.engine?.hasDocument === true &&
    s.engine.sessionsOpened >= 1 &&
    s.engine.rendersCompleted >= 1 &&
    s.engine.activeRenders === 0 &&
    s.host?.panes?.[0]?.lifecycle === "ready");
  assertHostWorkspace(s, "open");
  await assertPaneBox(s, "open");
  // Session ownership: the one pane owns exactly one engine session — its
  // PdfSession's — and nothing else in the realm holds a document.
  if (s.engine.sessionsLive !== 1) {
    throw new Error(`[open] ${s.engine.sessionsLive} live engine sessions, expected the pane's one`);
  }
  return s;
}

/** The host's bounds are what the pane is actually laid out in: its entry in
 *  the workspace slot renders at the box the host reports for it (the
 *  entry is positioned from the manager's per-pane bounds, not by filling
 *  the slot on its own). */
async function assertPaneBox(s, label, pane = s.host.panes[0]) {
  const box = await page.evaluate((id) => {
    const frame = document.querySelector("#runtime-host .runtime-frame[data-mareader-slot=\"active\"]");
    const doc = frame?.contentDocument ?? document;
    const el = doc.querySelector(`[data-pane-id="${id}"]`);
    if (!el) return null;
    const r = el.getBoundingClientRect();
    return { width: r.width, height: r.height };
  }, pane.paneId);
  if (!box) throw new Error(`[${label}] pane ${pane.paneId} has no entry in the workspace slot`);
  const off = Math.max(Math.abs(box.width - pane.bounds.width), Math.abs(box.height - pane.bounds.height));
  if (off > 2) {
    throw new Error(`[${label}] pane ${pane.paneId} renders at ${box.width}x${box.height}, the host handed it ${pane.bounds.width}x${pane.bounds.height}`);
  }
}

/** The production path, observed: the reader runtime's host reports a live
 *  workspace of exactly one READY pane, that pane is the one active pane,
 *  it holds the document session and its virtualizers, and its identity is
 *  a pane id — never the document's. */
function assertHostWorkspace(s, label) {
  const host = s.host;
  if (!host) throw new Error(`[${label}] the snapshot carries no host block`);
  if (host.lifecycle !== "live") throw new Error(`[${label}] host is ${host.lifecycle}, expected live`);
  if (host.panes.length !== 1) throw new Error(`[${label}] host has ${host.panes.length} panes, expected 1`);
  const pane = host.panes[0];
  if (typeof pane.paneId !== "number") throw new Error(`[${label}] pane id ${JSON.stringify(pane.paneId)} is not a pane id`);
  if (typeof pane.documentId !== "string" || !/^(book|path):/.test(pane.documentId)) {
    throw new Error(`[${label}] pane ${pane.paneId} names document ${JSON.stringify(pane.documentId)}`);
  }
  if (host.activePane !== pane.paneId || pane.focused !== true) {
    throw new Error(`[${label}] active pane ${host.activePane} != the one pane ${pane.paneId} (focused ${pane.focused})`);
  }
  const focused = host.panes.filter((p) => p.focused).length;
  if (focused !== 1) throw new Error(`[${label}] ${focused} panes claim focus`);
  if (pane.format !== "pdf") throw new Error(`[${label}] pane format ${pane.format}, expected pdf`);
  if (pane.resources.documentSession !== true) throw new Error(`[${label}] the pane does not hold its document session`);
  if (pane.resources.virtualizers < 2) {
    throw new Error(`[${label}] the pane owns ${pane.resources.virtualizers} virtualizers, expected its two strips`);
  }
  if (!(pane.bounds.width > 0 && pane.bounds.height > 0)) {
    throw new Error(`[${label}] the host handed the pane no bounds (${JSON.stringify(pane.bounds)})`);
  }
}

/** The workspace teardown, observed: the host is disposed, no pane is left
 *  (every pane's dispose finished), nobody is active. */
function assertHostDisposed(s, label) {
  const host = s.host;
  if (!host) throw new Error(`[${label}] the snapshot carries no host block`);
  if (host.lifecycle !== "disposed" || host.panes.length !== 0 || host.activePane !== null) {
    throw new Error(`[${label}] host not torn down: ${JSON.stringify(host)}`);
  }
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
    const btn = document.querySelector("#runtime-host .runtime-frame[data-mareader-slot=\"active\"]")?.contentDocument?.querySelector('button[title*="Close this book"]');
    if (!btn) throw new Error("close button not found");
    btn.click();
  });
}

/** The disposal baseline after a close, with every pairing asserted.
 *  Every cycle here boots fresh, so the open claims epoch 1 and the close
 *  claims epoch 2 — exactly one advance per cycle. */
async function closeAndWaitBaseline(label, settledWork, expectedEpoch = 2) {
  const panicsBefore = panicCount;
  if (!settledWork) await clickCloseNow();
  const s = await waitFor(`the disposal baseline (${label})`, (x) =>
    x.atBaseline === true && x.runtime?.state === "disposed", 45_000);
  assertDrained(s, label, expectedEpoch);
  assertHostDisposed(s, label);
  // The runtime itself reports disposal completion (Phase 1 §12): state
  // Disposed with a generation stamp — disposal is owned, not inferred.
  if (s.runtime?.state !== "disposed" || (s.runtime?.generation ?? 0) < 1) {
    throw new Error(`[${label}] runtime did not report disposal (state ${s.runtime?.state}, generation ${s.runtime?.generation})`);
  }
  assertNoNewPanics(label, panicsBefore);
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
  // Every pane's PdfSession was disposed with its pane: no engine session is
  // left registered in the realm.
  if (s.engine.sessionsLive !== 0) {
    throw new Error(`[${label}] ${s.engine.sessionsLive} engine sessions still live after the dispose`);
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
  if (!s.rasterLane || s.rasterLane.active !== 0 || s.rasterLane.queued !== 0 || s.rasterLane.owners !== 0) {
    throw new Error(`[${label}] host raster leases did not drain: ${JSON.stringify(s.rasterLane)}`);
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
// The race helper reads the artifact's OWN diagnostics probe (its window,
// fresh snapshots) — the Shell-side global merges the runtime's last pushed
// digest one digest beat behind, so a render it reports "active" may have
// finished. The close click below then lands while the render the close is
// meant to interrupt is actually in flight.
async function raceCloseDuringRender() {
  return page.evaluate(() => {
    const frame = document.querySelector("#runtime-host .runtime-frame[data-mareader-slot=\"active\"]");
    const raw = frame?.contentWindow?.__mareaderDiagnostics?.() ?? window.__mareaderDiagnostics?.();
    if (!raw) return false;
    const s = JSON.parse(raw);
    if (s.engine.activeRenders > 0) {
      const btn = frame?.contentDocument?.querySelector('button[title*="Close this book"]');
      if (btn) { btn.click(); return true; }
    }
    return false;
  });
}

async function raceCloseDuringPrefetch() {
  return page.evaluate(() => {
    const frame = document.querySelector("#runtime-host .runtime-frame[data-mareader-slot=\"active\"]");
    const raw = frame?.contentWindow?.__mareaderDiagnostics?.() ?? window.__mareaderDiagnostics?.();
    if (!raw) return false;
    const s = JSON.parse(raw);
    if (s.engine.activePrefetches > 0) {
      const btn = frame?.contentDocument?.querySelector('button[title*="Close this book"]');
      if (btn) { btn.click(); return true; }
    }
    return false;
  });
}

const stages = {};
const summary = {
  consoleErrorsUnrelated: 0,
  bootContract: {
    libraryBoot: null,
    readerBoot: null,
    transition: null,
    missingPane: null,
    missingShell: null,
  },
  fixturePages: {},
  fastJumpRenderDeltas: [],
  scrollPeaks: null,
  zoomPeaks: null,
  fastJumpPeaks: null,
  largeScrollPeaks: null,
  callbackCycles: 0,
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

// --- Stage 0: the production boot contract ---------------------------------
// Everything below this point proves the app works once it is UP. This stage
// proves it GETS there, on the same build Tauri packages (`frontendDist:
// ../dist`, served here by tests/browser/server.mjs), and that a boot which
// cannot finish says so instead of leaving an empty window. It is the browser
// half of §8/§9/§10/§11; the native half is tools/tauri-smoke.mjs plus
// .github/workflows/deep-ci.yml's tauri-smoke job.
currentStage = "stage0-boot-contract";

/** The runtime artifacts — the Shell's own, and the pane frames' — with the
 *  status the browser actually got (§9 — "resolves 200 when LOADED", not "the file is
 *  on disk"). Tauri serves these through its custom protocol and the dev
 *  server serves them from dist/; a missing one is the incident. */
const artifactStatuses = new Map();
page.on("response", (res) => {
  const { pathname } = new URL(res.url());
  if (["/mareader.js", "/mareader_bg.wasm", "/library.js", "/library_bg.wasm", "/reader.js", "/reader_bg.wasm", "/pdf.js", "/pdf_bg.wasm", "/reflow.js", "/reflow_bg.wasm"].includes(pathname)) {
    artifactStatuses.set(pathname, res.status());
  }
});

function assertArtifactLoaded(path, label) {
  const status = artifactStatuses.get(path);
  if (status !== 200) {
    throw new Error(`[${label}] ${path} resolved ${status ?? "never requested"}; every runtime load must reach a real artifact`);
  }
}

/** §5: the placeholder is part of the BUILT page and the shell takes it away
 *  once the host paints. Recorded from the very first document, with the
 *  timer starting at DOMContentLoaded. */
async function armShellBootWatcher() {
  await page.addInitScript(() => {
    window.__shellBoot = { copy: null, removedAt: null, background: null, titleWidth: null, mark: null };
    const record = () => {
      const boot = document.getElementById("shell-boot");
      if (boot && window.__shellBoot.copy === null) {
        window.__shellBoot.copy = (boot.textContent ?? "").replace(/\s+/g, " ").trim();
        window.__shellBoot.background = getComputedStyle(boot).backgroundColor;
        window.__shellBoot.titleWidth = boot.querySelector(".shell-boot__title")?.getBoundingClientRect().width ?? null;
        // The mark is the wait's other half: present, and ANIMATING — a
        // placeholder that only exists for assistive tech is how a slow launch
        // read as a hung one, and a mark stuck still reads the same way.
        const dot = boot.querySelector(".shell-boot__loader > .loader-dot");
        window.__shellBoot.mark = dot
          ? { count: boot.querySelectorAll(".shell-boot__loader > .loader-dot").length, animation: getComputedStyle(dot).animationName }
          : null;
      }
      if (!boot && window.__shellBoot.copy !== null && window.__shellBoot.removedAt === null) {
        window.__shellBoot.removedAt = Math.round(performance.now());
      }
    };
    document.addEventListener("DOMContentLoaded", () => {
      record();
      const timer = window.setInterval(record, 5);
      window.setTimeout(() => window.clearInterval(timer), 120_000);
    });
  });
}

/** Dense sampling of the invariants a screenshot cannot see (§11): the host
 *  is never empty, and two runtimes are never live in it at once. 10 ms is
 *  fast enough to catch a paint gap that lasts a frame and cheap enough to
 *  run across a whole transition. */
async function startHostSampler() {
  await page.evaluate(() => {
    const sample = () => {
      const host = document.getElementById("runtime-host");
      let diag = null;
      try {
        diag = JSON.parse(window.__mareaderDiagnostics?.() ?? "null");
      } catch {
        diag = null;
      }
      // Every frame carries one of the Shell's three slots. Anything else
      // throws here instead of being bucketed: the retired route "warm" slot
      // is the value this used to read, and a slot this lane does not know is
      // a bug, not a frame to sort.
      const slotOf = (f) => {
        const slot = f.getAttribute("data-mareader-slot");
        if (slot !== "active" && slot !== "incoming" && slot !== "retiring") {
          throw new Error(
            `runtime frame ${f.getAttribute("data-mareader-generation")} carries ` +
              `data-mareader-slot=${JSON.stringify(slot)}, which is not a frame slot`,
          );
        }
        return slot;
      };
      // Two frames are legal now (the on-screen one and the one booted
      // behind it), so the sampler sorts them by slot instead of assuming a
      // single runtime. Every DOM fact below is read from the ACTIVE frame:
      // an incoming or retiring runtime's grid and page host are on screen
      // nowhere.
      const frames = host ? [...host.querySelectorAll(".runtime-frame")] : [];
      let activeDoc = null;
      let actives = 0;
      let incoming = 0;
      let retiring = 0;
      let leaked = 0;
      for (const f of frames) {
        const slot = slotOf(f);
        if (slot === "active") {
          activeDoc = f.contentDocument?.defaultView.document;
          actives += 1;
        } else if (slot === "incoming") incoming += 1;
        else retiring += 1;
        // A frame that is not the active one must be invisible: a runtime
        // that rendered on screen would be two apps at once.
        if (slot !== "active" && getComputedStyle(f).visibility !== "hidden") leaked += 1;
      }
      return {
        t: Math.round(performance.now()),
        host: host !== null,
        nodes: host ? host.childNodes.length : 0,
        empty: host !== null && host.children.length === 0,
        bootNodes: host ? host.querySelectorAll("[data-mareader-boot]").length : 0,
        active: host ? host.getAttribute("data-mareader-active") : null,
        library: activeDoc ? activeDoc.querySelectorAll(".lib-grid").length : 0,
        reader: activeDoc ? activeDoc.querySelectorAll(".reader-bg").length : 0,
        frames: frames.length,
        actives,
        incoming,
        retiring,
        leaked,
        placeholder: document.getElementById("shell-boot") !== null,
        bootState: diag?.bootState ?? null,
        activeRuntime: diag?.activeRuntime ?? null,
        libraryFramesResident: diag?.libraryFramesResident ?? null,
        readerFramesResident: diag?.readerFramesResident ?? null,
        librarySessions: diag?.librarySessionsCreated ?? null,
        libraryDisposes: diag?.libraryDisposesCompleted ?? null,
        readerSessions: diag?.readerSessionsCreated ?? null,
        readerDisposes: diag?.readerDisposesCompleted ?? null,
      };
    };
    window.__hostSamples = [];
    window.__hostSampler = window.setInterval(() => {
      window.__hostSamples.push(sample());
      if (window.__hostSamples.length > 8000) window.__hostSamples.shift();
    }, 10);
  });
}

async function stopHostSampler() {
  return page.evaluate(() => {
    if (window.__hostSampler) window.clearInterval(window.__hostSampler);
    const samples = window.__hostSamples ?? [];
    window.__hostSampleContext = [];
    const violations = { empty: [], twoLive: [], mixed: [], unmarked: [], tooManyFrames: [], leakedHidden: [], twoActive: [] };
    // The samples around a violation are the diagnostic that matters — a bare
    // host is only meaningful next to what came before and after it.
    for (const s of samples) {
      // Before the shell's view mounts there is no host to be empty: the
      // page's own placeholder is the whole window, and §5 owns that state.
      if (!s.host) continue;
      // §10: one runtime at a time, and the host's own marker must agree
      // with what is mounted.
      // One runtime's DOM in the frame that is on screen — an incoming or
      // retiring runtime rendering its own shelf or page host beside it is
      // not "two live runtimes"; `leakedHidden` below is what catches the
      // case where that second frame is not hidden.
      if (s.library + s.reader > 1) violations.twoLive.push(s);
      if (s.active === "library" && s.reader > 0) violations.mixed.push(s);
      if (s.active === "reader" && s.library > 0) violations.mixed.push(s);
      // The host may hold two frames (the one on screen and the incoming or
      // retiring one behind it) and briefly two more (a retirement in flight
      // and the one before it that a back-to-back handoff outran), but never
      // a fifth — an unbounded frame count is the leak this bound exists to
      // catch. And never two frames claiming to be the one on screen.
      if (s.frames > 4) violations.tooManyFrames.push(s);
      if (s.actives > 1) violations.twoActive.push(s);
      if (s.leaked > 0) violations.leakedHidden.push(s);
      // §11: never a blank window. An empty host is legal only while the
      // page placeholder covers it — that is the loading state before the
      // first paint, and it is visible.
      const covered = s.bootNodes > 0 || s.placeholder;
      if (s.empty && !covered) violations.empty.push(s);
      // Nothing identifiable anywhere: not a boot state, not a marked
      // runtime, not a runtime's DOM, not the placeholder.
      if (s.bootNodes === 0 && s.active === null && s.library + s.reader === 0 && !covered) {
        violations.unmarked.push(s);
      }
    }
    for (const kind of [
      "empty", "twoLive", "mixed", "unmarked", "tooManyFrames", "leakedHidden", "twoActive",
    ]) {
      if (violations[kind].length === 0) continue;
      const first = violations[kind][0];
      const at = samples.indexOf(first);
      window.__hostSampleContext = samples.slice(Math.max(0, at - 6), at + 7);
      break;
    }
    return { samples, violations, context: window.__hostSampleContext ?? [] };
  });
}

function firstViolation(violations, context = []) {
  for (const kind of [
    "empty", "twoLive", "mixed", "unmarked", "tooManyFrames", "leakedHidden", "twoActive",
  ]) {
    if (violations[kind].length > 0) {
      const first = violations[kind][0];
      const around = context.map(
        (s) =>
          `t=${s.t} nodes=${s.nodes} boot=${s.bootNodes} active=${s.active} lib=${s.library} reader=${s.reader} frames=${s.frames} actives=${s.actives} inc=${s.incoming} ret=${s.retiring} leaked=${s.leaked} placeholder=${s.placeholder}`,
      );
      // NOT named `context`: the module has one of those (the Playwright
      // browser context) and a same-scope binding would shadow the parameter.
      const aroundText = around.length > 0 ? "\n  around: " + around.join("\n          ") : "";
      const counts = Object.entries(violations)
        .filter(([, list]) => list.length > 0)
        .map(([name, list]) => `${name}=${list.length}`)
        .join(" ");
      return `${kind}: ${JSON.stringify(first)} (${counts})${aroundText}`;
    }
  }
  return null;
}

/** The library's own DOM marker, not an HTTP 200: the grid is only there
 *  when the Library runtime really mounted and rendered (§8). */
async function libraryDomState() {
  return page.evaluate(() => {
    const host = document.getElementById("runtime-host");
    // Every frame carries one of the Shell's three slots; anything else
    // fails here rather than reading as a frame that is simply absent. The
    // retired route "warm" slot is the value this used to read.
    const slotOf = (f) => {
      const slot = f.getAttribute("data-mareader-slot");
      if (slot !== "active" && slot !== "incoming" && slot !== "retiring") {
        throw new Error(
          `runtime frame ${f.getAttribute("data-mareader-generation")} carries ` +
            `data-mareader-slot=${JSON.stringify(slot)}, which is not a frame slot`,
        );
      }
      return slot;
    };
    const frames = [...(host?.querySelectorAll(".runtime-frame") ?? [])];
    const slots = frames.map(slotOf);
    const pick = (slot) => frames.find((_, i) => slots[i] === slot)?.contentDocument?.defaultView.document;
    // The runtime roots themselves, in the slots: a pane frame renders a
    // document surface of its own, which is not a second runtime.
    const runtimeDoc = pick("active") ?? null;
    return {
      path: location.pathname,
      active: host?.getAttribute("data-mareader-active") ?? null,
      library: runtimeDoc?.querySelectorAll(".lib-grid").length ?? 0,
      reader: runtimeDoc?.querySelectorAll(".reader-bg").length ?? 0,
      bootNodes: host?.querySelectorAll("[data-mareader-boot]").length ?? 0,
      placeholder: document.getElementById("shell-boot") !== null,
      hosts: document.querySelectorAll("#runtime-host").length,
      frames: frames.length,
      // One frame on screen; up to one booting behind it; the rest retiring.
      actives: slots.filter((s) => s === "active").length,
      retiring: slots.filter((s) => s === "retiring").length,
    };
  });
}

/** The frame identities, straight off the DOM. The generation is the proof
 *  that every handoff booted a fresh realm — a reused boot would show up as
 *  an active generation repeating. A slot outside the Shell's three throws
 *  here instead of counting as a frame this lane knows. */
async function frameSlots() {
  return page.evaluate(() => {
    const host = document.getElementById("runtime-host");
    const out = { active: null, incoming: 0, retiring: 0, frames: 0 };
    for (const f of host?.querySelectorAll(".runtime-frame") ?? []) {
      out.frames += 1;
      const generation = Number(f.getAttribute("data-mareader-generation") ?? 0);
      const slot = f.getAttribute("data-mareader-slot");
      if (slot === "active") out.active = generation;
      else if (slot === "incoming") out.incoming += 1;
      else if (slot === "retiring") out.retiring += 1;
      else {
        throw new Error(
          `runtime frame ${generation} carries data-mareader-slot=${JSON.stringify(slot)}, ` +
            "which is not a frame slot",
        );
      }
    }
    return out;
  });
}

/** After retirement, only the active route may own a live session. */
function assertSessionBalance(s, label) {
  for (const kind of ["reader", "library"]) {
    const live = s.activeRuntime === kind ? 1 : 0;
    const created = s[`${kind}SessionsCreated`];
    const disposed = s[`${kind}DisposesCompleted`];
    if (created - disposed !== live) {
      throw new Error(
        `[${label}] ${kind}: ${created} created - ${disposed} disposed != ${live} live ` +
          `(active ${s.activeRuntime})`,
      );
    }
  }
}

/** The outgoing runtime is retired behind the reveal, so its disposal is no
 *  longer ordered before the handoff — but it must still COMPLETE. */
async function waitForRetirement(label, kind, before, timeoutMs = 45_000) {
  return waitFor(
    `${label}: the ${kind} finished disposing`,
    (x) => (x[`${kind}DisposesCompleted`] ?? 0) >= before + 1,
    timeoutMs,
  );
}

/** Poll the DOM until it reaches a settled state. The manager reports a
 *  runtime active the moment its start export returned; the runtime's own
 *  content (the library grid, the reader's page host) renders a frame later,
 *  so the assertion waits for the DOM rather than assuming one tick. The
 *  sampler's invariants cover what must NOT happen in between. */
async function waitForDom(label, predicate, timeoutMs = 30_000) {
  const started = Date.now();
  let state = await libraryDomState();
  for (;;) {
    if (predicate(state)) return state;
    if (Date.now() - started > timeoutMs) {
      throw new Error(`[${label}] the DOM never reached the settled state: ${JSON.stringify(state)}`);
    }
    await page.waitForTimeout(100);
    state = await libraryDomState();
  }
}

async function clickBook(title, label, timeout = 45_000) {
  try {
    await page.frameLocator('.runtime-frame[data-mareader-slot="active"]').locator(`.book-title[title*="${title}"]`).first().click({ timeout: 5_000 });
  } catch {
    // The grid's tap is pointerup-owned; HTMLElement.click only swallows a
    // completed hold. The row's Enter handler is the real alternate open.
    await page.evaluate((needle) => {
      const doc = document.querySelector("#runtime-host .runtime-frame[data-mareader-slot=\"active\"]")?.contentDocument;
      const el = [...(doc?.querySelectorAll(".book-title") ?? [])]
        .find((n) => (n.textContent ?? "").includes(needle));
      if (!el) throw new Error("book row not found in the library");
      el.closest('[role="button"]').focus();
    }, title);
    await page.keyboard.press("Enter");
  }
  return waitFor(`${label}: the reader runtime to become active`, (x) =>
    x.bootState === "reader" &&
    x.readerRuntimeLive === true &&
    x.engine?.hasDocument === true &&
    x.engine.activeRenders === 0, timeout);
}

/** Exercise the pointer path that formerly prewarmed Reader. It must now
 *  leave Library alone; only an actual open may instantiate a Reader. */
function assertLibraryOnly(s, label) {
  if (s.activeRuntime !== "library" || s.routeReturnPolicy !== "unload-both" ||
      s.routePrewarmAllowed !== false || s.readerFramesResident !== 0 || s.libraryFramesResident !== 1 ||
      s.paneFramesResident !== 0 || s.atBaseline !== true ||
      s.rasterLane?.active !== 0 || s.rasterLane?.queued !== 0 || s.rasterLane?.owners !== 0 ||
      s.readerSessionsCreated !== s.readerDisposesCompleted) {
    throw new Error(`[${label}] Library retained Reader resources: ${JSON.stringify(s)}`);
  }
}

function assertReaderOnly(s, label) {
  if (s.activeRuntime !== "reader" || s.routeReturnPolicy !== "unload-both" ||
      s.routePrewarmAllowed !== false || s.libraryFramesResident !== 0 ||
      s.readerFramesResident !== 1 || s.bakeFrameResident !== false ||
      s.librarySessionsCreated !== s.libraryDisposesCompleted) {
    throw new Error(`[${label}] Reader retained Library resources: ${JSON.stringify(s)}`);
  }
  assertSessionBalance(s, label);
}

async function signalShelfIntent(label) {
  try {
    await page
      .frameLocator('.runtime-frame[data-mareader-slot="active"]')
      .locator("#library-level")
      .hover({ timeout: 5_000, position: { x: 40, y: 40 } });
  } catch {
    const dispatched = await page.evaluate(() => {
      const doc = document.querySelector("#runtime-host .runtime-frame[data-mareader-slot=\"active\"]")?.contentDocument;
      const level = doc?.getElementById("library-level");
      if (!level) return false;
      level.dispatchEvent(new PointerEvent("pointerover", { bubbles: true, pointerType: "mouse" }));
      return true;
    });
    if (!dispatched) throw new Error(`[${label}] the shelf has no #library-level to reach into`);
  }
}

// ---- 0: the library is seeded the way a user seeds it ---------------------
// A fresh browser context has an EMPTY library: there is no grid to assert and
// no book to open, so the stage that proves the boot contract has to run on a
// library that holds something. Opening a document from the URL is that path
// (the Shell disposes the library and loads the reader for `?open=`), and it
// doubles as the first assertion that the PRODUCTION boot path renders a
// runtime into the host rather than only returning HTTP 200s.
currentStage = "stage0-seed";
await page.goto(`${BASE}/?blend=1&open=${encodeURIComponent(PEARLS)}`, {
  waitUntil: "domcontentloaded",
});
const seeded = await waitFor("the reader runtime to boot from ?open=", (x) =>
  x.bootState === "reader" && x.readerRuntimeLive === true && x.engine?.hasDocument === true, 60_000);
const seededDom = await waitForDom("?open=: the reader rendered", (s) =>
  s.active === "reader" && s.reader >= 1 && s.library === 0 && !s.placeholder);
assertArtifactLoaded("/pdf.js", "?open= seed");
assertArtifactLoaded("/pdf_bg.wasm", "?open= seed");
summary.bootContract.seedOpen = {
  bootState: seeded.bootState,
  readerSessionsCreated: seeded.readerSessionsCreated,
  libraryDisposes: seeded.libraryDisposesCompleted ?? 0,
  readerDom: seededDom.reader,
};

// ---- 0a: `/` boots the Library runtime ------------------------------------
// The reader files a book's cover on its first open (its open pipeline: a
// second, small render of page 1 that lands shortly AFTER the first page is
// on screen), and a shelf that finds its cover never asks for a bake. The
// cover block below is the one proof the Shell's bake page works end to
// end, so the shelf must start without one. Let the seed's own cover land
// first — clearing the key while that write is still in flight would only
// have it reappear a moment later — then drop the persisted covers before
// the library boots. A seed whose cover never lands (a failed render leaves
// the stylised fallback) has nothing to drop, and the shelf bakes anyway.
{
  const started = Date.now();
  while (Date.now() - started < 15_000) {
    const landed = await page.evaluate(() => {
      try {
        return Object.keys(JSON.parse(localStorage.getItem("mareader.covers.v1") ?? "{}")).length > 0;
      } catch {
        return false;
      }
    });
    if (landed) break;
    await page.waitForTimeout(250);
  }
  await page.evaluate(() => localStorage.removeItem("mareader.covers.v1"));
}
await armShellBootWatcher();
await page.goto(`${BASE}/`, { waitUntil: "domcontentloaded" });
await startHostSampler();
const libraryBoot = await waitFor("the library runtime to boot at /", (x) =>
  x.bootState === "library" && x.activeRuntime === "library", 60_000);
// The settled library: its grid rendered, its loading state gone, and the
// page's own placeholder (which covered the window until then) removed.
const libraryDom = await waitForDom("/: the library rendered", (s) =>
  s.library >= 1 && s.reader === 0 && !s.placeholder);
if (libraryDom.path !== "/") {
  throw new Error(`[/] the boot left the route at ${libraryDom.path}`);
}
if (libraryDom.active !== "library") {
  throw new Error(`[/] the host does not mark the library active (data-mareader-active=${libraryDom.active})`);
}
if (libraryDom.library !== 1) {
  throw new Error(`[/] the library did not render into the host (.lib-grid x${libraryDom.library})`);
}
if (libraryDom.reader !== 0) {
  throw new Error(`[/] a reader session is mounted at the library route (.reader-bg x${libraryDom.reader})`);
}
if (libraryDom.bootNodes !== 0) {
  throw new Error(`[/] the shell's loading state outlived the boot (${libraryDom.bootNodes} boot node(s))`);
}
if (libraryDom.placeholder) {
  throw new Error("[/] the page's boot placeholder is still in the DOM after the host painted");
}
if (libraryDom.hosts !== 1) {
  throw new Error(`[/] expected exactly one runtime host, found ${libraryDom.hosts}`);
}
// One frame ON SCREEN and nothing behind it: the library route at rest is
// the library alone. A reader is booted behind the shelf on the shelf's
// intent signal (a pointer over the grid), never on the shelf's paint — a
// reader kept "just in case" is exactly the memory the library route is
// meant to give back, and nothing in this stage has reached into the shelf.
if (libraryDom.actives !== 1) {
  throw new Error(`[/] expected exactly one active runtime frame, found ${libraryDom.actives}`);
}
if (libraryDom.frames !== 1) {
  throw new Error(`[/] expected the library frame alone at rest, found ${libraryDom.frames} runtime frame(s)`);
}
{
  const atRest = await snap();
  assertLibraryOnly(atRest, "untouched shelf");
}
assertArtifactLoaded("/mareader.js", "/");
assertArtifactLoaded("/mareader_bg.wasm", "/");
assertArtifactLoaded("/library.js", "/");
assertArtifactLoaded("/library_bg.wasm", "/");
const shellBoot = await page.evaluate(() => window.__shellBoot);
if (!shellBoot?.copy?.includes("Loading MAReader")) {
  throw new Error(`[/] the shell page never carried its loading state (saw ${JSON.stringify(shellBoot?.copy)})`);
}
if (shellBoot.removedAt === null) {
  throw new Error("[/] the shell never removed the page's boot placeholder");
}
// A healthy boot SHOWS its wait: the app's own loading mark, running, and the
// stage line under it — the webview can hold an unpainted window for seconds on
// Windows, and the one thing that screen must not be is blank.
if (!(shellBoot.titleWidth !== null && shellBoot.titleWidth > 1)) {
  throw new Error(`[/] the boot placeholder's copy is not laid out on a healthy start (title width ${shellBoot.titleWidth})`);
}
// loader-hop-* normally, loader-fade under prefers-reduced-motion: the one
// animation styles/components/animations.css refuses to still.
if (!(shellBoot.mark?.count === 3 && /^loader-(hop|fade)/.test(shellBoot.mark.animation ?? ""))) {
  throw new Error(`[/] the boot placeholder does not run the app's own loading mark: ${JSON.stringify(shellBoot.mark)}`);
}
// The shelf's cover bakes: neither the Shell nor the library loads a PDF
// engine, and no reader is booted for them — a cover can only exist if the
// Shell's own bake page works end to end: shelf asks, Shell mounts
// `bake.html` (pdf.js alone, no wasm), the page bakes, the Shell hands the
// art back, the shelf files and persists it. The seeded book must get its
// cover, and the bake page must be GONE a few seconds after: it is a
// transient worker, not a resident.
{
  const started = Date.now();
  let covers = 0;
  let sawBakeFrame = false;
  for (;;) {
    const s = await snap();
    if (s?.bakeFrameResident === true) sawBakeFrame = true;
    covers = await page.evaluate(() => {
      try {
        return Object.keys(JSON.parse(localStorage.getItem("mareader.covers.v1") ?? "{}")).length;
      } catch {
        return 0;
      }
    });
    if (covers > 0) break;
    if (Date.now() - started > 45_000) {
      throw new Error("[/] the seeded book never got a cover — the Shell's bake page is broken");
    }
    await page.waitForTimeout(250);
  }
  const baked = await snap();
  if ((baked?.readerFramesResident ?? 0) !== 0) {
    throw new Error(`[/] baking a cover booted a reader (readerFramesResident ${baked.readerFramesResident})`);
  }
  // The covers were cleared before this boot, so the only way one exists now
  // is the bake page: it must have been seen resident. (It stays for the
  // idle grace after the drain, well above the poll interval above.)
  if (!sawBakeFrame && baked?.bakeFrameResident !== true) {
    throw new Error("[/] a cover arrived without the Shell's bake page ever being resident");
  }
  const bakeGone = await waitFor("/: the bake page removed after the covers drained", (x) =>
    x.bakeFrameResident === false, 30_000);
  const bakeFrames = await page.evaluate(() => document.querySelectorAll("iframe.bake-frame").length);
  if (bakeFrames !== 0) {
    throw new Error(`[/] the probe says the bake page is gone but ${bakeFrames} bake frame(s) are in the DOM`);
  }
  summary.bootContract.coverBake = {
    covers,
    ms: Date.now() - started,
    sawBakeFrame,
    coversAnswered: bakeGone.coversAnswered ?? null,
  };
}
summary.bootContract.libraryBoot = {
  path: libraryDom.path,
  placeholderCopy: shellBoot.copy,
  placeholderRemovedAtMs: shellBoot.removedAt,
  libraryDisposes: libraryBoot.libraryDisposesCompleted ?? 0,
};

// ---- 0b: actual opens instantiate a fresh Reader, never intent ------------
currentStage = "stage0-transition";
await signalShelfIntent("library → reader");
await page.waitForTimeout(800);
const intentSlots = await frameSlots();
const beforeHandoff = await snap();
assertLibraryOnly(beforeHandoff, "before real open");
if (intentSlots.frames !== 1 || intentSlots.incoming !== 0 || intentSlots.retiring !== 0) {
  throw new Error(`shelf intent allocated a runtime realm (${JSON.stringify(intentSlots)})`);
}
const libraryInstance = await page.evaluate(() => {
  const frame = document.querySelector('iframe.runtime-frame[data-mareader-slot="active"]');
  frame.contentWindow.__libraryInstanceMarker = crypto.randomUUID();
  window.__shellLifetimeMarker = crypto.randomUUID();
  return { library: frame.contentWindow.__libraryInstanceMarker, shell: window.__shellLifetimeMarker,
    generation: Number(frame.dataset.mareaderGeneration) };
});
const readerActive = await clickBook("Programming Pearls", "library → reader");
const readerDom = await waitForDom("library → reader: the reader mounted", (s) =>
  s.active === "reader" && s.reader >= 1 && s.library === 0);
const revealedSlots = await frameSlots();
if (readerDom.reader !== 1 || readerDom.library !== 0) {
  throw new Error(`[library → reader] host holds reader ${readerDom.reader} / library ${readerDom.library}`);
}
if (revealedSlots.active === intentSlots.active || revealedSlots.active === null) {
  throw new Error("a real open did not instantiate its separate Reader host");
}
assertArtifactLoaded("/reader.js", "library → reader");
assertArtifactLoaded("/reader_bg.wasm", "library → reader");
// The shelf the user left is retired BEHIND the reveal — so its disposal is
// no longer ordered before the handoff, but it must still run to completion.
const libraryRetired = await waitForRetirement("library → reader", "library",
  beforeHandoff.libraryDisposesCompleted ?? 0);
assertReaderOnly(libraryRetired, "library → reader");
assertArtifactLoaded("/pdf.js", "library → reader");
assertArtifactLoaded("/pdf_bg.wasm", "library → reader");

const beforeHandback = await snap();
const readerOnlySlots = await frameSlots();
if (readerOnlySlots.frames !== 1 || readerOnlySlots.incoming !== 0 || readerOnlySlots.retiring !== 0) {
  throw new Error(`reading retained another route realm (${JSON.stringify(readerOnlySlots)})`);
}
await clickCloseNow();
const libraryAgain = await waitFor("the library runtime after the handback", (x) =>
  x.bootState === "library" && x.activeRuntime === "library", 45_000);
const handbackDom = await waitForDom("reader → library: the library came back", (s) =>
  s.active === "library" && s.library >= 1 && s.reader === 0);
const backSlots = await frameSlots();
if (handbackDom.library !== 1 || handbackDom.reader !== 0) {
  throw new Error(`[reader → library] host holds library ${handbackDom.library} / reader ${handbackDom.reader}`);
}
if (backSlots.active === libraryInstance.generation || backSlots.active === null) {
  throw new Error("reader → library reused the old Library realm");
}
const freshLibrary = await page.evaluate(() => {
  const frame = document.querySelector('iframe.runtime-frame[data-mareader-slot="active"]');
  return { marker: frame.contentWindow.__libraryInstanceMarker ?? null, shell: window.__shellLifetimeMarker,
    generation: Number(frame.dataset.mareaderGeneration) };
});
if (freshLibrary.marker !== null || freshLibrary.shell !== libraryInstance.shell) {
  throw new Error(`fresh Library inherited the old realm or reloaded Shell: ${JSON.stringify(freshLibrary)}`);
}
summary.bootContract.libraryUnload = { oldGeneration: libraryInstance.generation,
  freshGeneration: freshLibrary.generation, oldMarkerAbsent: true, shellPreserved: true,
  libraryFramesWhileReading: libraryRetired.libraryFramesResident, bakeFrameWhileReading: libraryRetired.bakeFrameResident };
const readerRetired = await waitForRetirement("reader → library", "reader",
  beforeHandback.readerDisposesCompleted ?? 0);
assertSessionBalance(readerRetired, "reader → library");
assertLibraryOnly(readerRetired, "reader → library");

// ---- 0c: the same handoff, back to back ----------------------------------
// One pair of transitions proves the mechanism; repetition is what finds the
// hole that only racing starts open — a click that lands while a retirement
// is still settling, an incoming boot still in flight when the user asks
// for it, a queued start draining into the same host. The sampler from 0a is
// still running, so its invariants cover every instant of all of these too,
// not just the first pair.
currentStage = "stage0-rapid-transitions";
const cycles = [];
for (let cycle = 0; cycle < 4; cycle += 1) {
  const before = await waitFor(`rapid ${cycle}: no Reader realm on Library`,
    (x) => x.atBaseline === true && x.readerFramesResident === 0);
  assertLibraryOnly(before, `rapid ${cycle} before open`);
  const libraryAtRest = await frameSlots();
  if (libraryAtRest.frames !== 1 || libraryAtRest.incoming !== 0 || libraryAtRest.retiring !== 0) {
    throw new Error(`[rapid ${cycle}] Library retained a realm (${JSON.stringify(libraryAtRest)})`);
  }
  const intoReader = await clickBook("Programming Pearls", `rapid ${cycle}: library → reader`);
  const readerDomNow = await waitForDom(`rapid ${cycle}: the reader mounted`, (s) =>
    s.active === "reader" && s.reader >= 1 && s.library === 0);
  if (readerDomNow.reader !== 1 || readerDomNow.library !== 0) {
    throw new Error(`[rapid ${cycle}] host holds reader ${readerDomNow.reader} / library ${readerDomNow.library}`);
  }
  const revealed = await frameSlots();
  if (revealed.active === libraryAtRest.active || revealed.active === null ||
      cycles.some((c) => c.readerGeneration === revealed.active)) {
    throw new Error(`[rapid ${cycle}] the Reader host was not a fresh realm`);
  }
  const libraryClosedNow = await waitForRetirement(`rapid ${cycle}: Library removed`, "library", before.libraryDisposesCompleted);
  assertReaderOnly(libraryClosedNow, `rapid ${cycle} reading`);
  await clickCloseNow();
  const libraryDomNow = await waitForDom(`rapid ${cycle}: the library came back`, (s) =>
    s.active === "library" && s.library >= 1 && s.reader === 0);
  if (libraryDomNow.library !== 1 || libraryDomNow.reader !== 0) {
    throw new Error(`[rapid ${cycle}] host holds library ${libraryDomNow.library} / reader ${libraryDomNow.reader}`);
  }
  // The retirement is behind the reveal now, so wait for it to land rather
  // than expecting it to have happened already — then hold the accounting
  // to the exact number of runtimes that exist.
  const retired = await waitForRetirement(`rapid ${cycle}`, "reader",
    intoReader.readerDisposesCompleted ?? 0);
  assertSessionBalance(retired, `rapid ${cycle}`);
  assertLibraryOnly(retired, `rapid ${cycle} returned`);
  const returned = await frameSlots();
  if (returned.active === libraryAtRest.active || returned.active === null ||
      cycles.some((c) => c.libraryGeneration === returned.active)) {
    throw new Error(`[rapid ${cycle}] Library return reused an old realm`);
  }
  const after = retired;
  if (after.readerRuntimeLive !== false) {
    throw new Error(`[rapid ${cycle}] the reader runtime is still live after the close`);
  }
  cycles.push({
    cycle,
    readerGeneration: revealed.active,
    libraryGeneration: returned.active,
    libraryFramesWhileReading: libraryClosedNow.libraryFramesResident,
    readerFramesAfterReturn: after.readerFramesResident,
    librarySessions: after.librarySessionsCreated,
    libraryDisposes: after.libraryDisposesCompleted,
    readerSessions: after.readerSessionsCreated,
    readerDisposes: after.readerDisposesCompleted,
  });
}
if (new Set(cycles.map((c) => c.readerGeneration)).size !== cycles.length ||
    new Set(cycles.map((c) => c.libraryGeneration)).size !== cycles.length ||
    cycles.some((c) => c.readerFramesAfterReturn !== 0 || c.libraryFramesWhileReading !== 0)) {
  throw new Error("rapid transitions reused or retained the outgoing route realm");
}
summary.bootContract.rapidTransitions = cycles;
console.log(
  `boot contract: ${cycles.length} back-to-back handoffs (reader sessions ${cycles.at(-1).readerSessions} / disposals ${cycles.at(-1).readerDisposes}, library sessions ${cycles.at(-1).librarySessions} / disposals ${cycles.at(-1).libraryDisposes})`,
);

// The invariants across the whole transition.
//
// §10 used to be an ORDERING rule — "a runtime is only marked active once
// its predecessor's dispose promise has resolved" — because the outgoing
// runtime was disposed before the replacement was built. The handoff
// inverts that on purpose: the replacement is revealed first and the
// predecessor is retired behind it, which is what makes a route change a
// reveal instead of a rebuild. What must still hold, and still does, is
// every property that ordering was protecting:
//
//   * one runtime on screen at every sampled instant (twoLive / mixed /
//     twoActive), and any other frame hidden (leakedHidden);
//   * the host never blank (empty / unmarked);
//   * every runtime that was ever shown finishes disposing (waitForRetirement
//     and assertSessionBalance above).
const sampled = await stopHostSampler();
const violation = firstViolation(sampled.violations, sampled.context);
if (violation) {
  throw new Error(`runtime-host invariant violated while booting/transitioning — ${violation}`);
}
const readerSamples = sampled.samples.filter((s) => s.active === "reader");
const librarySamples = sampled.samples.filter((s) => s.active === "library");
if (readerSamples.length === 0 || librarySamples.length === 0) {
  throw new Error(`the sampler saw no ${readerSamples.length === 0 ? "reader" : "library"} active sample`);
}
// A handoff shows up as a sample where two frames coexist — the one on
// screen and the incoming or retiring one behind it. That is the policy
// working, not a leak; the frame ceiling above is its bound.
const peakFrames = Math.max(...sampled.samples.map((s) => s.frames ?? 0));
const overlapSamples = sampled.samples.filter((s) => s.retiring > 0).length;
if (peakFrames < 2) {
  throw new Error(
    `the incoming handoff never overlapped its predecessor (peak ${peakFrames}) — the handoff is still a rebuild`,
  );
}
summary.bootContract.transition = {
  samples: sampled.samples.length,
  readerSamples: readerSamples.length,
  librarySamples: librarySamples.length,
  peakFrames,
  retiringSamples: overlapSamples,
  libraryDisposes: libraryAgain.libraryDisposesCompleted,
  readerDisposes: libraryAgain.readerDisposesCompleted,
};
assertNoNewPanics("stage0 transitions", 0);
console.log(
  `boot contract: host sampled ${sampled.samples.length} times across both transitions, ` +
    `never empty, never two runtimes on screen, peak ${peakFrames} frames`,
);

// ---- 0d: the `/reader` boot path (§8) -------------------------------------
currentStage = "stage0-reader-route";
await page.goto(`${BASE}/reader?blend=1&open=${encodeURIComponent(PEARLS)}`, {
  waitUntil: "domcontentloaded",
});
const readerRoute = await waitFor("the reader runtime to boot at /reader", (x) =>
  x.bootState === "reader" && x.readerRuntimeLive === true && x.engine?.hasDocument === true, 120_000);
const readerRouteDom = await waitForDom("/reader: the reader rendered", (s) =>
  s.active === "reader" && s.reader >= 1 && s.library === 0 && !s.placeholder);
if (readerRouteDom.path !== "/reader") {
  throw new Error(`[/reader] the boot left the route at ${readerRouteDom.path}`);
}
if (readerRouteDom.reader !== 1 || readerRouteDom.library !== 0 || readerRouteDom.active !== "reader") {
  throw new Error(`[/reader] host holds reader ${readerRouteDom.reader} / library ${readerRouteDom.library} (active ${readerRouteDom.active})`);
}
if (readerRouteDom.placeholder) {
  throw new Error("[/reader] the page's boot placeholder outlived the boot");
}
if (readerRouteDom.actives !== 1) {
  throw new Error(`[/reader] expected exactly one active runtime frame, found ${readerRouteDom.actives}`);
}
assertReaderOnly(readerRoute, "/reader direct boot");
if (readerRouteDom.frames !== 1) {
  throw new Error(`[/reader] expected only Reader, found ${readerRouteDom.frames} route frames`);
}
assertArtifactLoaded("/pdf.js", "/reader");
assertArtifactLoaded("/pdf_bg.wasm", "/reader");
summary.bootContract.readerBoot = {
  path: readerRouteDom.path,
  generation: readerRoute.runtime?.generation ?? null,
};
// ---- 0e: Library never retains or prewarms ANY Reader realm ---------------
currentStage = "stage0-reader-unload";
await page.goto(`${BASE}/`, { waitUntil: "domcontentloaded" });
const idleBoot = await waitFor("Library boot without Reader", (x) =>
  x.bootState === "library" && x.activeRuntime === "library" && x.atBaseline === true, 60_000);
await waitForDom("Library alone", (s) => s.library === 1 && s.reader === 0 && !s.placeholder);
assertLibraryOnly(idleBoot, "idle Library");
const marker = await page.evaluate(() => (window.__routeLifetimeMarker = crypto.randomUUID()));
for (let i = 0; i < 4; i += 1) {
  await signalShelfIntent(`Library pointer ${i}`);
  await page.evaluate(() => {
    const frame = document.querySelector('.runtime-frame[data-mareader-slot="active"]');
    const level = frame.contentDocument.getElementById("library-level");
    for (const type of ["pointerover", "pointermove", "pointerdown", "focusin"]) {
      level.dispatchEvent(new frame.contentWindow.Event(type, { bubbles: true }));
    }
  });
  await page.waitForTimeout(300);
  assertLibraryOnly(await snap(), `Library pointer/focus/press ${i}`);
}
const noIntentBoot = await frameSlots();
if (noIntentBoot.frames !== 1 || noIntentBoot.incoming !== 0 || noIntentBoot.retiring !== 0) {
  throw new Error(`Library intent created a runtime (${JSON.stringify(noIntentBoot)})`);
}
// No Reader code is fetched without an actual open, either.
const readerResources = await page.evaluate(() => {
  const frame = document.querySelector('.runtime-frame[data-mareader-slot="active"]');
  return [window, frame.contentWindow].flatMap((w) => w.performance.getEntriesByType("resource"))
    .filter((e) => /\/(reader|pdf|reflow)(_bg\.wasm|\.js)(?:$|[?#])/.test(e.name)).map((e) => e.name);
});
if (readerResources.length) throw new Error(`Library fetched Reader artifacts: ${readerResources}`);
const generations = [];
for (let i = 0; i < 2; i += 1) {
  await clickBook("Programming Pearls", `fresh host ${i}`);
  const slots = await frameSlots();
  if (generations.includes(slots.active)) throw new Error("Reader host realm survived a Library return");
  generations.push(slots.active);
  const reading = await waitFor(`fresh host ${i}: Library realm removed`, (x) =>
    x.libraryFramesResident === 0 && x.librarySessionsCreated === x.libraryDisposesCompleted &&
    x.bakeFrameResident === false, 15_000);
  assertReaderOnly(reading, `fresh host ${i}`);
  await clickCloseNow();
  const returned = await waitFor(`fresh host ${i}: all Reader realms gone`, (x) =>
    x.bootState === "library" && x.atBaseline === true && x.readerFramesResident === 0, 15_000);
  assertSessionBalance(returned, `fresh host ${i}`);
  assertLibraryOnly(returned, `fresh host ${i}`);
  await signalShelfIntent(`fresh host ${i}: keep pointer on Library`);
  await page.waitForTimeout(700);
  assertLibraryOnly(await snap(), `fresh host ${i} after pointer`);
}
// Returning while the Reader WASM itself is still loading cancels that
// incoming host; unblocking a late response must never resurrect it.
let unblock;
const held = new Promise((resolve) => { unblock = resolve; });
let sawBlockedReader;
const readerBlocked = new Promise((resolve) => { sawBlockedReader = resolve; });
const delayReader = async (route) => {
  sawBlockedReader();
  await held;
  await route.continue().catch(() => {});
};
await page.route("**/reader_bg.wasm", delayReader);
// Focus the real row and use its Enter action. Shelf taps are decided at
// pointerup (click only swallows holds); the low-level keyboard API also
// cannot wait for the WASM navigation this test is deliberately holding.
await page.evaluate(() => {
  const doc = document.querySelector('.runtime-frame[data-mareader-slot="active"]').contentDocument;
  const row = doc.querySelector('.book-title[title*="Programming Pearls"]');
  if (!row) throw new Error("cancelled-boot proof has no Library row");
  const entry = row.closest('[role="button"]');
  if (!entry) throw new Error("cancelled-boot proof has no actionable Library row");
  entry.focus();
});
await page.keyboard.press("Enter");
await Promise.race([
  readerBlocked,
  page.waitForTimeout(5000).then(() => { throw new Error("Reader WASM was never held by the cancellation proof"); }),
]);
await page.waitForFunction(() => !!document.querySelector('iframe[data-mareader-runtime-frame="reader"][data-mareader-slot="incoming"]'));
await page.evaluate(() => history.back());
const cancelled = await waitFor("cancelled incoming Reader removed", (x) =>
  x.bootState === "library" && x.atBaseline === true && x.readerFramesResident === 0, 10_000);
unblock();
await page.unroute("**/reader_bg.wasm", delayReader);
await page.waitForTimeout(800);
assertLibraryOnly(await snap(), "late Reader response");
if (await page.evaluate(() => window.__routeLifetimeMarker) !== marker) throw new Error("route handoffs reloaded the Shell");
summary.bootContract.readerUnload = { generations, intentCreatedReader: false,
  incomingCancelled: cancelled.readerFramesResident === 0, rootPreserved: true };
assertNoNewPanics("Reader route lifetimes", 0);
console.log("boot contract: Library holds no Reader/PDF/reflow realms under idle, intent, return or cancelled boot");

// ---- 0f: Reader holds no Library, even after cancelled Library boot/bake --
currentStage = "stage0-library-unload";
await clickBook("Programming Pearls", "Library teardown cancellation setup");
const readingClean = await waitFor("Library gone while reading", (x) =>
  x.libraryFramesResident === 0 && x.librarySessionsCreated === x.libraryDisposesCompleted && !x.bakeFrameResident);
assertReaderOnly(readingClean, "before cancelled Library boot");
const originalReader = (await frameSlots()).active;
let releaseLibrary;
const heldLibrary = new Promise((resolve) => { releaseLibrary = resolve; });
let libraryRequested;
const requestedLibrary = new Promise((resolve) => { libraryRequested = resolve; });
const blockLibrary = async (route) => {
  libraryRequested();
  await heldLibrary;
  await route.continue().catch(() => {});
};
await page.route("**/library_bg.wasm", blockLibrary);
await clickCloseNow();
await Promise.race([requestedLibrary,
  page.waitForTimeout(5000).then(() => { throw new Error("Library WASM was never held by the cancellation proof"); })]);
await page.waitForFunction(() => !!document.querySelector('iframe[data-mareader-runtime-frame="library"][data-mareader-slot="incoming"]'));
await page.evaluate(() => history.back());
const libraryCancelled = await waitFor("cancelled incoming Library removed", (x) =>
  x.bootState === "reader" && x.libraryFramesResident === 0 && x.engine?.hasDocument, 15_000);
releaseLibrary();
await page.unroute("**/library_bg.wasm", blockLibrary);
await page.waitForTimeout(800);
assertReaderOnly(await snap(), "late Library boot response");
if ((await frameSlots()).active !== originalReader) throw new Error("cancelled Library boot replaced the still-visible Reader host");

// Hold a real cover's file read in the JS-only baker. Opening Reader must
// kill that Library-owned queue/page, not let it finish behind the reader.
await page.evaluate(() => localStorage.setItem("mareader.covers.v1", "{}"));
let releaseCover;
const heldCover = new Promise((resolve) => { releaseCover = resolve; });
let coverRequested;
const requestedCover = new Promise((resolve) => { coverRequested = resolve; });
const blockCover = async (route) => {
  if (new URL(route.request().frame().url()).pathname !== "/bake.html") {
    await route.continue();
    return;
  }
  coverRequested();
  await heldCover;
  await route.continue().catch(() => {});
};
await page.route("**/samples/**", blockCover);
await clickCloseNow();
await waitFor("fresh Library while cover is held", (x) => x.bootState === "library" && x.activeRuntime === "library", 30_000);
await Promise.race([requestedCover,
  page.waitForTimeout(15000).then(() => { throw new Error("cover file read was never held by the Library teardown proof"); })]);
const bakingLibrary = await snap();
if (!bakingLibrary.bakeFrameResident) throw new Error("held cover had no resident bake page");
await clickBook("Programming Pearls", "Reader closes baking Library");
// The reader this stage opens replaces the one whose retirement was still
// finishing behind the reveal when the Library came back — the host is
// allowed two frames across a transition (the sampler's "peak 2 frames"), and
// a retiring driver is registered until its teardown runs. So the wait below
// asks for the READER's side of the rest shape too, exactly as the handoff
// stages do through `waitForRetirement`, and the assertion that follows still
// holds it to one frame at rest.
const bakerClosed = await waitFor("Library and held cover page gone", (x) =>
  x.libraryFramesResident === 0 && !x.bakeFrameResident && x.readerFramesResident === 1 &&
  x.librarySessionsCreated === x.libraryDisposesCompleted);
assertReaderOnly(bakerClosed, "Library-owned bake cancellation");
releaseCover();
await page.unroute("**/samples/**", blockCover);
await page.waitForTimeout(800);
const afterLateCover = await snap();
assertReaderOnly(afterLateCover, "late cancelled cover response");
if (afterLateCover.coversAnswered !== bakerClosed.coversAnswered) throw new Error("a cancelled Library cover was answered after its owner disappeared");
if (await page.evaluate(() => window.__routeLifetimeMarker) !== marker) throw new Error("Library lifetime checks reloaded Shell");
summary.bootContract.libraryUnload = { ...summary.bootContract.libraryUnload,
  incomingCancelled: libraryCancelled.libraryFramesResident === 0,
  readerPreservedDuringCancelledBoot: true, activeBakeCancelled: true, lateCoverIgnored: true };
await clickCloseNow();
const finalLibraryOnly = await waitFor("Library after symmetric cancellation proofs", (x) => x.activeRuntime === "library" && x.atBaseline);
assertLibraryOnly(finalLibraryOnly, "symmetric route lifetime final return");
assertNoNewPanics("Library route lifetimes", 0);
console.log("boot contract: Reader holds no Library WASM or cover bake; both routes remount fresh without Shell reload");


// ---- 0f: a boot that cannot finish is VISIBLE ------------------------------
// With a runtime artifact missing, the user must see a named error state,
// never a blank pane or an endless placeholder. The server 404s the artifact
// for the injected context, so this drives the real code path an incomplete
// build produces: a missing pane artifact fails that pane (the Shell and its
// chrome stay up), and a missing Shell artifact fails the page placeholder.
currentStage = "stage0-missing-artifact";
async function failureProbe(value, url, label, read, check) {
  const failContext = await browser.newContext({ viewport: { width: 1400, height: 900 } });
  await failContext.addCookies([{ name: "mareader_boot_fail", value, url: BASE }]);
  const failPage = await failContext.newPage();
  const consoleLines = [];
  const pageErrorLines = [];
  failPage.on("console", (m) => consoleLines.push(`[${m.type()}] ${m.text()}`));
  failPage.on("pageerror", (e) => pageErrorLines.push(`${e.message}\n${e.stack ?? ""}`));
  try {
    await failPage.goto(url, { waitUntil: "domcontentloaded" });
    const started = Date.now();
    let state = null;
    for (;;) {
      state = await failPage.evaluate(read);
      if (state.failed || Date.now() - started > 40_000) break;
      await failPage.waitForTimeout(200);
    }
    if (!state.failed) {
      throw new Error(`[${label}] no error state appeared for a missing artifact (state ${JSON.stringify(state)})`);
    }
    check(state, consoleLines);
    if (pageErrorLines.length > 0) {
      throw new Error(`[${label}] the failed boot threw ${pageErrorLines.length} page error(s):\n${pageErrorLines.join("\n")}`);
    }
    return { consoleLines: consoleLines.length, ...state };
  } finally {
    await failContext.close();
  }
}

summary.bootContract.missingPane = await failureProbe(
  "pdf",
  pearlsUrl,
  "missing pane artifact",
  () => {
    const doc = document.querySelector('iframe.runtime-frame[data-mareader-slot="active"]')?.contentDocument;
    const node = doc?.querySelector('[data-pane-boot="error"]') ?? null;
    let diag = null;
    try {
      diag = JSON.parse(window.__mareaderDiagnostics?.() ?? "null");
    } catch {
      diag = null;
    }
    return {
      failed: node !== null,
      text: (node?.textContent ?? "").replace(/\s+/g, " ").trim(),
      bootState: diag?.bootState ?? null,
      reader: doc?.querySelectorAll(".reader-bg").length ?? 0,
      placeholder: document.getElementById("shell-boot") !== null,
    };
  },
  (state, lines) => {
    if (!state.text.includes("/pdf.js")) {
      throw new Error(`[missing pane artifact] the pane does not name the artifact that failed: ${state.text}`);
    }
    // The Shell itself is fine: its chrome stays up around the failed pane.
    if (state.bootState !== "reader" || state.reader !== 1 || state.placeholder) {
      throw new Error(`[missing pane artifact] the Shell did not stay up around the failed pane: ${JSON.stringify(state)}`);
    }
    if (!lines.some((line) => line.includes("[mareader] pane boot failed") && line.includes("/pdf.js"))) {
      throw new Error(`[missing pane artifact] the console does not carry the failure:\n${lines.join("\n")}`);
    }
  },
);
summary.bootContract.missingShell = await failureProbe(
  "mareader",
  `${BASE}/`,
  "missing shell artifact",
  () => {
    const boot = document.getElementById("shell-boot");
    const dot = boot?.querySelector(".shell-boot__loader > .loader-dot");
    return {
      failed: boot?.getAttribute("data-shell-boot") === "timeout",
      text: (boot?.textContent ?? "").replace(/\s+/g, " ").trim(),
      animation: dot ? getComputedStyle(dot).animationName : null,
      library: document.querySelectorAll(".lib-grid").length,
    };
  },
  (state, lines) => {
    if (!state.text.includes("did not start") || state.library !== 0) {
      throw new Error(`[missing shell artifact] the placeholder does not report the failure: ${JSON.stringify(state)}`);
    }
    // A start that failed is not a start that is slow: the mark stops.
    if (state.animation !== "none") {
      throw new Error(`[missing shell artifact] the placeholder is still animating while reporting a failed start: ${JSON.stringify(state.animation)}`);
    }
    if (!lines.some((line) => line.includes("[mareader] the shell did not start"))) {
      throw new Error(`[missing shell artifact] the console does not carry the failure:\n${lines.join("\n")}`);
    }
  },
);
console.log("boot contract: a missing runtime artifact shows a named error state, never a blank window");

// --- Stage 0c: the frameless window's own chrome --------------------------
// The bar and the caption live in the ROUTE frame, and on Windows a route frame
// is also injected with its OWN Tauri API — the configuration that used to cost
// both halves of the chrome: the frame's `event.listen` registrations went to a
// registry the backend never scripts into (so `tauri://resize` was swallowed and
// the maximize glyph froze), and the frame's bar had a drag region no listener
// could act on (so nothing was grabbable). The stub below reproduces that shape
// — an API in EVERY document — and the UA is pinned so the frameless cluster is
// mounted whichever machine runs the suite. A real browser cannot move a window
// or maximize one, so this stage asserts the COMMANDS, which is where the bug
// lived; `tools/tauri-smoke.mjs` (Linux, no window manager) can prove nothing
// about either.
currentStage = "stage0-window-chrome";
{
  const chromeContext = await browser.newContext({
    viewport: { width: 1400, height: 900 },
    userAgent: "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 " +
      "(KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36",
  });
  await chromeContext.addInitScript(() => {
    // One recorder, in the main frame's realm, because every document's stub
    // has to feed the same list: on Windows a frame gets its own injected API
    // and this is the shape that produces.
    const isTop = window.top === window;
    const shared = isTop
      ? (window.__chromeTauri = { calls: [], listeners: [], maximized: false })
      : window.top.__chromeTauri;
    if (!shared) return;
    const windowHandle = {
      isMaximized: () => Promise.resolve(shared.maximized),
      minimize: () => { shared.calls.push("window:minimize"); return Promise.resolve(); },
      toggleMaximize: () => {
        shared.calls.push("window:toggleMaximize");
        shared.maximized = !shared.maximized;
        return Promise.resolve();
      },
      close: () => { shared.calls.push("window:close"); return Promise.resolve(); },
      setBackgroundColor: () => Promise.resolve(),
    };
    const api = {
      metadata: { currentWindow: { label: "main" } },
      core: {
        invoke: (cmd) => { shared.calls.push(cmd); return Promise.resolve(undefined); },
        convertFileSrc: (path) => `https://asset.localhost/${encodeURIComponent(path)}`,
      },
      event: {
        listen: (name, handler) => {
          shared.listeners.push({ name, where: isTop ? "top" : "frame", handler });
          return Promise.resolve(() => {});
        },
        unlisten: () => Promise.resolve(),
        emit: () => Promise.resolve(),
      },
      window: { getCurrentWindow: () => windowHandle },
      dialog: {
        open: () => Promise.resolve(null),
        save: () => Promise.resolve(null),
        message: () => Promise.resolve(null),
        ask: () => Promise.resolve(null),
        confirm: () => Promise.resolve(null),
      },
    };
    window.__TAURI__ = api;
    window.__TAURI_INTERNALS__ = {
      metadata: api.metadata,
      invoke: api.core.invoke,
      convertFileSrc: api.core.convertFileSrc,
      transformCallback: () => 0,
    };
  });
  const chromePage = await chromeContext.newPage();
  try {
    await chromePage.goto(`${BASE}/`, { waitUntil: "domcontentloaded" });
    // Hover-reveal the bar (the band's own mouseenter is what raises it), so the
    // presses below land where the user's would.
    await chromePage.waitForFunction(() => {
      const doc = document.querySelector('iframe.runtime-frame[data-mareader-slot="active"]')?.contentDocument;
      const row = doc?.getElementById("toolbar-row");
      if (!row) return false;
      row.parentElement?.dispatchEvent(new MouseEvent("mouseenter", { bubbles: false }));
      return !row.inert && getComputedStyle(row).pointerEvents !== "none";
    }, null, { timeout: 45_000 });

    const regions = await chromePage.evaluate(() => {
      const doc = document.querySelector('iframe.runtime-frame[data-mareader-slot="active"]')?.contentDocument;
      if (!doc) throw new Error("[window chrome] the stubbed page mounted no route frame");
      const row = doc.getElementById("toolbar-row");
      if (!row) throw new Error("[window chrome] the mounted route carries no #toolbar-row");
      const band = row.parentElement;
      const recorder = window.__chromeTauri;
      const press = (el, detail, type) => {
        const from = recorder.calls.length;
        // dispatchEvent returns false once preventDefault() has been called: the
        // drag script's own claim on the press, and the proof a control was NOT
        // swallowed by it.
        const allowed = el.dispatchEvent(new MouseEvent(type ?? "mousedown", {
          bubbles: true, composed: true, cancelable: true, button: 0,
          detail: detail ?? 1, clientX: 40, clientY: 20,
        }));
        return { calls: recorder.calls.slice(from), defaultPrevented: !allowed };
      };
      // Any element the row's `deep` region claims and no control owns.
      let deep = null;
      for (const el of row.querySelectorAll("div,span,p")) {
        if (el.getClientRects().length === 0 || el.clientWidth < 4 || el.clientHeight < 4) continue;
        if (el.closest("[data-tauri-drag-region='false']")) continue;
        const region = el.closest("[data-tauri-drag-region]");
        if (region?.getAttribute("data-tauri-drag-region") !== "deep") continue;
        deep = el;
        break;
      }
      const button = row.querySelector("button, [role='button'], input, a[href]");
      const optOut = row.querySelector("[data-tauri-drag-region='false']");
      // A double-click: Windows and Linux maximize on the press, macOS defers
      // to the release so a drag away can cancel it (the native title bar's
      // grace). Either way EXACTLY ONE toggle must come out of the pair, on
      // whichever host runs the suite.
      const dbl = deep ? { down: press(deep, 2), up: press(deep, 2, "mouseup") } : null;
      return {
        bandAttr: band?.getAttribute("data-tauri-drag-region") ?? null,
        rowAttr: row.getAttribute("data-tauri-drag-region") ?? null,
        deepFound: deep !== null,
        deepPress: deep ? press(deep) : null,
        bandPress: band ? press(band) : null,
        dbl: dbl ? { calls: dbl.down.calls.concat(dbl.up.calls) } : null,
        buttonPress: button ? press(button) : null,
        optOutPress: optOut ? press(optOut) : null,
      };
    });
    if (regions.rowAttr !== "deep" || regions.bandAttr !== "deep") {
      throw new Error(`[window chrome] the bar is not a deep drag region (band ${JSON.stringify(regions.bandAttr)}, row ${JSON.stringify(regions.rowAttr)})`);
    }
    if (!regions.deepFound) {
      throw new Error("[window chrome] nothing inside the bar is claimed by its deep region — the region reaches no pixels");
    }
    const dragged = "plugin:window|start_dragging";
    const toggled = "plugin:window|internal_toggle_maximize";
    for (const [label, record] of [["inside the bar", regions.deepPress], ["the bar's own band", regions.bandPress]]) {
      if (record?.calls?.[0] !== dragged || record.calls.length !== 1) {
        throw new Error(`[window chrome] a press on ${label} did not start exactly one window drag: ${JSON.stringify(record)}`);
      }
      if (!record.defaultPrevented) {
        throw new Error(`[window chrome] a press on ${label} was not claimed by the drag region`);
      }
    }
    if (regions.dbl?.calls?.length !== 1 || regions.dbl.calls[0] !== toggled) {
      throw new Error(`[window chrome] a double-click in the bar did not toggle maximization exactly once: ${JSON.stringify(regions.dbl)}`);
    }
    for (const [label, record] of [["a control", regions.buttonPress], ["the search field's pill", regions.optOutPress]]) {
      if (!record) {
        throw new Error(`[window chrome] the bar has no ${label} to test the exclusion against`);
      }
      if (record.calls.length !== 0 || record.defaultPrevented) {
        throw new Error(`[window chrome] the bar's drag region swallowed a press on ${label}: ${JSON.stringify(record)}`);
      }
    }

    const caption = await chromePage.evaluate(() => {
      const doc = document.querySelector('iframe.runtime-frame[data-mareader-slot="active"]')?.contentDocument;
      if (!doc) throw new Error("[window chrome] the stubbed page mounted no route frame");
      const buttons = [...doc.querySelectorAll(".window-controls button")];
      const maximize = buttons.find((b) => (b.getAttribute("aria-label") ?? b.title) === "Maximize") ?? null;
      maximize?.click();
      return { found: maximize !== null, labels: buttons.map((b) => b.getAttribute("aria-label") ?? b.title ?? null) };
    });
    if (!caption.found) {
      throw new Error(`[window chrome] the caption cluster has no Maximize button (${JSON.stringify(caption.labels)})`);
    }
    await chromePage.waitForFunction(() => window.__chromeTauri.calls.includes("window:toggleMaximize"), null, { timeout: 10_000 });
    const emitResize = (width, height) => chromePage.evaluate(([w, h]) => {
      // The backend delivers an emitted event by scripting the MAIN frame, so a
      // frame's listener has to be registered there to be reachable at all —
      // which is what the caption's whole state machine depends on.
      window.__chromeTauri.listeners
        .filter((l) => l.name === "tauri://resize")
        .forEach((l) => l.handler({ event: "tauri://resize", id: 1, payload: { width: w, height: h } }));
      return null;
    }, [width, height]);
    const labelIs = (want) => chromePage.waitForFunction((text) => {
      const doc = document.querySelector('iframe.runtime-frame[data-mareader-slot="active"]')?.contentDocument;
      if (!doc) return false;
      return [...doc.querySelectorAll(".window-controls button")]
        .some((b) => (b.getAttribute("aria-label") ?? b.title) === text);
    }, want, { timeout: 10_000 });
    await emitResize(1600, 1000);
    await labelIs("Restore");
    const stateAfterMaximize = await chromePage.evaluate(() => ({
      maximized: window.__chromeTauri.maximized,
      listeners: window.__chromeTauri.listeners.map(({ name, where }) => ({ name, where })),
    }));
    const resizeRegistrations = stateAfterMaximize.listeners.filter((l) => l.name === "tauri://resize");
    if (resizeRegistrations.length === 0 || resizeRegistrations.some((l) => l.where !== "top")) {
      throw new Error(`[window chrome] the maximized probe did not register its resize listener on the host frame: ${JSON.stringify(stateAfterMaximize.listeners)}`);
    }
    if (stateAfterMaximize.maximized !== true) {
      throw new Error(`[window chrome] the caption says Restore while the window says otherwise: ${JSON.stringify(stateAfterMaximize)}`);
    }
    // And back again, same path, no reload: a probe that answered once and never
    // again is exactly what the report described.
    await chromePage.evaluate(() => {
      const doc = document.querySelector('iframe.runtime-frame[data-mareader-slot="active"]').contentDocument;
      const restore = [...doc.querySelectorAll(".window-controls button")]
        .find((b) => (b.getAttribute("aria-label") ?? b.title) === "Restore");
      restore?.click();
      return null;
    });
    await chromePage.waitForFunction(() => window.__chromeTauri.calls.filter((c) => c === "window:toggleMaximize").length === 2, null, { timeout: 10_000 });
    await emitResize(1400, 900);
    await labelIs("Maximize");
    summary.windowChrome = {
      deepPress: regions.deepPress,
      bandPress: regions.bandPress,
      doubleClick: regions.dbl,
      buttonPress: regions.buttonPress,
      optOutPress: regions.optOutPress,
      caption: caption.labels,
      resizeListeners: resizeRegistrations.length,
      roundTrip: ["Maximize", "Restore", "Maximize"],
    };
    console.log(`window chrome: ${JSON.stringify(summary.windowChrome)}`);
  } finally {
    await chromeContext.close();
  }
}
console.log("window chrome: the bar drags the window, the caption follows its state, and a frame's events land where the backend can reach them");

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
  const gen = await page.evaluate(() =>
    document.querySelector("#runtime-host .runtime-frame[data-mareader-slot=\"active\"]")?.contentWindow?.PDFReader.beginRenderGeneration());
  await page.evaluate((w) => {
    const list = document.querySelector("#runtime-host .runtime-frame[data-mareader-slot=\"active\"]")?.contentDocument?.querySelector("#page-list");
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
    {
      const api = document.querySelector("#runtime-host .runtime-frame[data-mareader-slot=\"active\"]")?.contentWindow?.PDFReader;
      return api.renderTrace().filter((e) => e.gen === g);
    }, gen);
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
const panicsBeforeRenderRace = panicCount;
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
    assertNoNewPanics("close during render", panicsBeforeRenderRace);
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
const panicsBeforePrefetchRace = panicCount;
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
    assertNoNewPanics("close during prefetch", panicsBeforePrefetchRace);
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
const panicsBeforeSearchRace = panicCount;
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
  // The find bar is the active pane's own chrome: it lives in the pane's frame.
  const searchBox = page
    .frameLocator('.runtime-frame[data-mareader-slot="active"]')
    .locator('iframe.pane-frame:not([data-frame-hidden])')
    .contentFrame()
    .locator('input[placeholder^="Search in document"]');
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
        const btn = document.querySelector("#runtime-host .runtime-frame[data-mareader-slot=\"active\"]")?.contentDocument?.querySelector('button[title*="Close this book"]');
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
    assertNoNewPanics("close during search", panicsBeforeSearchRace);
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
// raster recycler, the library's caches — never felt a cycle. This workload
// rides the real application path instead: click the book open from the
// library (the row the reader recorded on open), work the pages, click the
// toolbar close, and repeat in the SAME live page.
// Every close must still return every reader-owned resource to baseline.
// The Shell RECYCLES a reader frame after a close: the session is disposed
// (fully drained — that is what this stage asserts) and a fresh warm session
// mounts in the same document, so the wasm world, and with it the disposal
// epoch, carries on. The epoch rule is therefore per frame: a fresh frame's
// open claims 1; a recycled frame's open claims the previous close + 1; and
// every close claims exactly one more than its open.
/** Open the fixture from the library and wait for THAT open: a runtime
 *  generation `fresh` accepts, live with its document and no render in
 *  flight. Waiting on liveness alone raced the click — the reader kept from
 *  the previous close already satisfies it, so a fast run read the old
 *  session back before the new one existed. A generation that never becomes
 *  fresh times out here into the latest snapshot, and the caller's own
 *  assertion names the failure. */
// The generation of the last reader an open actually landed in. A stage's
// "new runtime" base must be this, not the generation snapped after a close:
// by then the warm slot may already have booted the NEXT reader, whose
// generation is minted before the click that opens it.
let lastOpenedGeneration = 0;
async function openFromLibrary(cycle, fresh = () => true) {
  const card = page.frameLocator('.runtime-frame[data-mareader-slot="active"]').locator('.book-title[title*="Programming Pearls"]').first();
  try {
    await card.click({ timeout: 5_000 });
  } catch {
    // The grid's tap is pointerup-owned; HTMLElement.click only swallows a
    // completed hold. The row's Enter handler is the real alternate open.
    await page.evaluate(() => {
      const t = [...document.querySelector("#runtime-host .runtime-frame[data-mareader-slot=\"active\"]")?.contentDocument?.querySelectorAll(".book-title")]
        .find((el) => (el.textContent ?? "").includes("Programming Pearls"));
      if (!t) throw new Error("book row not found in the library");
      t.click();
    });
  }
  const opened = (x) =>
    x.readerRuntimeLive === true &&
    x.engine?.hasDocument === true &&
    x.engine.activeRenders === 0;
  let o;
  try {
    o = await waitFor(`same-page open ${cycle}`, (x) =>
      opened(x) && fresh(x.runtime?.generation ?? 0), 45_000);
  } catch {
    o = await waitFor(`same-page open ${cycle}`, opened, 5_000);
  }
  lastOpenedGeneration = o.runtime?.generation ?? lastOpenedGeneration;
  return o;
}
// The shell-relative base is the runtime generation only: every same-page
// open must be a NEW runtime generation (a disposed runtime is never revived
// in place). The warm slot changes WHEN that generation is minted — the
// reader boots before the click now, so the generation no longer advances on
// the click — which is why the assertion is "never repeats" rather than
// "+1 per cycle". The disposal epoch belongs to the frame's wasm world:
// a fresh frame opens at 1, a recycled one continues from its last close
// (the session is new — the generation check above proves that — but the
// module is the one the frame already loaded).
const generationBase = (await snap()).runtime?.generation ?? 1;
const usedGenerations = new Set();
console.log("same-page stage: runtime generation base", generationBase);
let lastReaderFrame = null;
let lastCloseEpoch = null;
summary.samePageRecycledOpens = 0;
for (let cycle = 1; cycle <= 10; cycle += 1) {
  const o = await openFromLibrary(cycle, (gen) => gen > generationBase && !usedGenerations.has(gen));
  const openFrame = (await frameSlots()).active;
  const recycled = openFrame !== null && openFrame === lastReaderFrame;
  const expectedOpen = recycled ? lastCloseEpoch + 1 : 1;
  if (o.disposalEpoch !== expectedOpen) {
    throw new Error(
      `same-page open ${cycle}: epoch ${o.disposalEpoch}, expected ${expectedOpen} ` +
        `(${recycled ? `recycled frame ${openFrame}: one claim past its last close` : "a fresh frame's first claim is the open"})`,
    );
  }
  if (recycled) summary.samePageRecycledOpens += 1;
  {
    const gen = o.runtime?.generation ?? 0;
    if (gen <= generationBase || usedGenerations.has(gen)) {
      throw new Error(
        `same-page open ${cycle}: reader session ${gen} is not a new runtime ` +
          `(base ${generationBase}, already used ${[...usedGenerations].join(",")}) — the disposed runtime was revived`,
      );
    }
    usedGenerations.add(gen);
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
    (x) => x.atBaseline === true && x.runtime?.state === "disposed", 45_000);
  assertDrained(c, `same-page cycle ${cycle}`, o.disposalEpoch + 1);
  lastReaderFrame = openFrame;
  lastCloseEpoch = c.disposalEpoch;
  if (c.runtime?.state !== "disposed") {
    throw new Error(`same-page cycle ${cycle}: runtime ${c.runtime?.state}, expected disposed`);
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

// --- Stage 12: workload I — queued callbacks across the close --------------
currentStage = "stage12-callback-x10";
// The red-CI guide's Patch E, as a standing stage: the failure class is a
// frame or timer armed while the reader was alive, firing after the route
// disposed its owner. The earlier race stages land the close inside ENGINE
// work; this one lands it inside the UI frame machinery — a zoom arms the
// floating title's frame, the strip's rescale frame and the anchor-settle rAF
// chain — then leaves at once and waits SEVERAL frames, so every armed
// callback has had its chance to fire into a disposed owner. A trap fails the
// stage through assertNoNewPanics; the residue a trapped artifact would leave
// behind fails the drained baseline.
const waitFrames = (n) =>
  page.evaluate(
    (count) =>
      new Promise((resolve) => {
        let seen = 0;
        const step = () => {
          seen += 1;
          if (seen >= count) resolve(seen);
          else requestAnimationFrame(step);
        };
        requestAnimationFrame(step);
      }),
    n,
  );
const callbackGenBase = lastOpenedGeneration || ((await snap()).runtime?.generation ?? 1);
const usedCallbackGenerations = new Set();
for (let cycle = 1; cycle <= 10; cycle += 1) {
  const o = await openFromLibrary(cycle, (gen) => gen > callbackGenBase && !usedCallbackGenerations.has(gen));
  {
    const gen = o.runtime?.generation ?? 0;
    if (gen <= callbackGenBase || usedCallbackGenerations.has(gen)) {
      throw new Error(
        `callback cycle ${cycle}: reader session ${gen} is not a new runtime ` +
          `(base ${callbackGenBase}) — the disposed runtime was revived`,
      );
    }
    usedCallbackGenerations.add(gen);
  }
  // Arm the frame machinery through the app's real zoom pipeline, then move
  // the page: the volume, the titlebar and the anchor loop all queue frames
  // that are still pending when the close lands.
  await page.mouse.click(700, 450);
  await page.keyboard.press("+");
  await waitFrames(2);
  await page.keyboard.press("PageDown");
  const panicsBeforeCycle = panicCount;
  await clickCloseNow();
  await waitFrames(8);
  await page.waitForTimeout(120);
  assertNoNewPanics(`callback cycle ${cycle}`, panicsBeforeCycle);
  const c = await waitFor(`the disposal baseline (callback cycle ${cycle})`,
    (x) => x.atBaseline === true && x.runtime?.state === "disposed", 45_000);
  // Same-page opens run in a recycled frame: the close claims one past the
  // open, whatever epoch that frame's wasm world had reached.
  assertDrained(c, `callback cycle ${cycle}`, o.disposalEpoch + 1);
  if (c.runtime?.state !== "disposed") {
    throw new Error(`callback cycle ${cycle}: runtime ${c.runtime?.state}, expected disposed`);
  }
  // This stage's own assertions, spelled out rather than implied by the
  // counter balance: the runtime is gone, both engine lanes are idle, and
  // every piece of virtualizer machinery (instances, observers, timers) is
  // at zero — the exact residue a surviving callback would keep alive.
  if (c.readerRuntimeLive !== false) {
    throw new Error(`callback cycle ${cycle}: reader runtime still live`);
  }
  if (c.engine.activeRenders !== 0 || c.engine.activePrefetches !== 0) {
    throw new Error(`callback cycle ${cycle}: engine still working (renders ${c.engine.activeRenders}, prefetches ${c.engine.activePrefetches})`);
  }
  if (c.virtualizerLive !== 0 || c.virtualizerObservers !== 0 || c.virtualizerTimers !== 0) {
    throw new Error(`callback cycle ${cycle}: virtualizer machinery alive (live ${c.virtualizerLive}, observers ${c.virtualizerObservers}, timers ${c.virtualizerTimers})`);
  }
  summary.callbackCycles = cycle;
}
stages.afterCallbackDiscipline = await snap();
console.log("queued-callback discipline x10 clean: no disposal panic, every armed callback fired into a disposed owner without trapping");

// --- Stage 13: a real mixed-format split workspace ------------------------
currentStage = "stage13-split-workspace";
// The production path with two live panes: a PDF, then a Markdown document
// placed BESIDE it through the host's one open command (the web build's
// `__mareaderOpenIn` hook drives exactly what the menu's split and the
// file dialog's "Open Beside…" drive). Both run their own format session;
// the host owns the layout, the divider and the focus; closing the PDF
// pane leaves the Markdown pane alive and reading.
const SPLIT_NOTES = "/samples/Split Notes.md";
const activeFrame = '#runtime-host .runtime-frame[data-mareader-slot="active"]';

/** The layout's leaves, in order (the snapshot's `layout` is the tree). */
function layoutLeaves(node) {
  if (!node) return [];
  if (node.leaf !== undefined) return [node.leaf];
  return [...layoutLeaves(node.split.first), ...layoutLeaves(node.split.second)];
}

/** Two ready panes, one PDF and one Markdown, each holding its session. */
function assertSplitWorkspace(s, label, pdfPane) {
  const host = s.host;
  if (host.lifecycle !== "live") throw new Error(`[${label}] host is ${host.lifecycle}`);
  if (host.panes.length !== 2) throw new Error(`[${label}] ${host.panes.length} panes, expected 2`);
  const pdf = host.panes.find((p) => p.paneId === pdfPane);
  const md = host.panes.find((p) => p.paneId !== pdfPane);
  if (!pdf || pdf.format !== "pdf") throw new Error(`[${label}] the PDF pane is gone: ${JSON.stringify(host.panes)}`);
  if (md.format !== "markdown") throw new Error(`[${label}] the second pane is ${md.format}, expected markdown`);
  for (const pane of [pdf, md]) {
    if (pane.lifecycle !== "ready") throw new Error(`[${label}] pane ${pane.paneId} is ${pane.lifecycle}`);
    if (pane.resources.documentSession !== true) throw new Error(`[${label}] pane ${pane.paneId} holds no document session`);
    if (!(pane.viewportArea > 0)) throw new Error(`[${label}] pane ${pane.paneId} reports viewport area ${pane.viewportArea}`);
    if (!(pane.resources.zoom > 0)) throw new Error(`[${label}] pane ${pane.paneId} reports zoom ${pane.resources.zoom}`);
  }
  if (pdf.documentId === md.documentId) throw new Error(`[${label}] both panes name document ${pdf.documentId}`);
  if (typeof md.paneId !== "number" || md.paneId === pdf.paneId) throw new Error(`[${label}] pane ids ${pdf.paneId} / ${md.paneId}`);
  const focused = host.panes.filter((p) => p.focused);
  if (focused.length !== 1 || focused[0].paneId !== host.activePane) {
    throw new Error(`[${label}] ${focused.length} focused panes, active ${host.activePane}`);
  }
  // The PDF engine holds exactly the PDF pane's session: the Markdown pane
  // runs no engine session at all.
  if (s.engine.sessionsLive !== 1) throw new Error(`[${label}] ${s.engine.sessionsLive} engine sessions, expected the PDF pane's one`);
  const split = host.layout?.split;
  if (!split || split.axis !== "horizontal") throw new Error(`[${label}] layout is not a left|right split: ${JSON.stringify(host.layout)}`);
  const leaves = layoutLeaves(host.layout);
  if (leaves.join() !== `${pdf.paneId},${md.paneId}`) throw new Error(`[${label}] layout leaves ${leaves}, expected PDF then Markdown`);
  // Side by side, not stacked: the Markdown pane starts where the PDF ends.
  if (!(pdf.bounds.x + pdf.bounds.width <= md.bounds.x + 1) || Math.abs(pdf.bounds.y - md.bounds.y) > 1) {
    throw new Error(`[${label}] panes are not side by side: ${JSON.stringify([pdf.bounds, md.bounds])}`);
  }
  return { pdf, md, split };
}

/** Wait for `predicate` AND for the layout to have landed: every pane's
 *  rendered entry measures the box the host reports for it. The digest is
 *  pushed on a beat, so right after a re-layout (a split, a drag's last
 *  frame, a close) the snapshot and the DOM can each be one frame from the
 *  other; a box that never converges still fails, at the deadline. */
async function waitForSettledLayout(label, predicate, timeoutMs = 30_000) {
  const deadline = Date.now() + timeoutMs;
  let last = null;
  while (Date.now() < deadline) {
    const s = await snap();
    if (s && predicate(s)) {
      const boxes = await page.evaluate((sel) => {
        const doc = document.querySelector(sel)?.contentDocument;
        const out = {};
        for (const el of doc?.querySelectorAll("[data-pane-id]") ?? []) {
          const r = el.getBoundingClientRect();
          out[el.dataset.paneId] = { width: r.width, height: r.height };
        }
        return out;
      }, activeFrame);
      last = { panes: s.host.panes.map((p) => ({ id: p.paneId, host: p.bounds })), boxes };
      const agree = s.host.panes.every((p) => {
        const b = boxes[p.paneId];
        return b && Math.abs(b.width - p.bounds.width) <= 2 && Math.abs(b.height - p.bounds.height) <= 2;
      });
      if (agree) return s;
    }
    await page.waitForTimeout(100);
  }
  throw new Error(`[${label}] the layout never settled: ${JSON.stringify(last)}`);
}

/** Every placed pane is SHOWN (inactive is not hidden), with a live box. */
async function paneEntries() {
  return page.evaluate((sel) => {
    const doc = document.querySelector(sel)?.contentDocument;
    return [...(doc?.querySelectorAll("[data-pane-id]") ?? [])].map((el) => {
      const r = el.getBoundingClientRect();
      const cs = el.ownerDocument.defaultView.getComputedStyle(el);
      return {
        id: Number(el.dataset.paneId),
        active: el.dataset.paneActive === "true",
        visible: cs.visibility !== "hidden" && cs.pointerEvents !== "none" && r.width > 0 && r.height > 0,
      };
    });
  }, activeFrame);
}

{
  const panicsBefore = panicCount;
  // This stage proves fully per-pane looks — each pane its own Light / Dark
  // / Dim — so it runs with the shared-mode setting (on by default) off.
  // Settings are read at boot; the open below reboots on them.
  await page.evaluate(() => {
    const key = "mareader.settings.v1";
    let s = {};
    try { s = JSON.parse(localStorage.getItem(key) ?? "{}") ?? {}; } catch { s = {}; }
    s.workspace = { ...(s.workspace ?? {}), sharedBaseMode: false };
    localStorage.setItem(key, JSON.stringify(s));
  });
  const opened = await openBook(pearlsUrl);
  const pdfPane = opened.host.panes[0].paneId;
  const placed = await page.evaluate(([sel, path]) => {
    const hook = document.querySelector(sel)?.contentWindow?.__mareaderOpenIn;
    if (typeof hook !== "function") throw new Error("the web build's __mareaderOpenIn hook is missing");
    return hook(path, "right");
  }, [activeFrame, SPLIT_NOTES]);
  if (placed !== true) throw new Error("[split] the host refused the Markdown pane beside the PDF");

  const both = await waitForSettledLayout("split: PDF | Markdown both ready", (s) =>
    s.host?.panes?.length === 2 &&
    s.host.panes.every((p) => p.lifecycle === "ready" && p.resources?.documentSession === true) &&
    s.host.panes.some((p) => p.format === "markdown"), 60_000);
  const { pdf, md } = assertSplitWorkspace(both, "split", pdfPane);
  // The pane the open created took focus.
  if (both.host.activePane !== md.paneId) throw new Error(`[split] active pane ${both.host.activePane}, expected the new Markdown pane ${md.paneId}`);
  await assertPaneBox(both, "split: pdf", pdf);
  await assertPaneBox(both, "split: markdown", md);
  await page.waitForFunction((sel) => {
    const doc = document.querySelector(sel)?.contentDocument;
    return !!doc?.querySelector(".reader-bg.split-workspace [data-pane-active='true'] .pane-focus-outline");
  }, activeFrame, { timeout: 5_000 });
  const focusPaint = await page.evaluate(([sel]) => {
    const doc = document.querySelector(sel)?.contentDocument;
    const root = doc?.querySelector(".reader-bg");
    const entries = [...(doc?.querySelectorAll("[data-pane-id]") ?? [])];
    const active = entries.find((entry) => entry.dataset.paneActive === "true");
    const inactive = entries.find((entry) => entry !== active);
    const outline = active?.querySelector(".pane-focus-outline");
    return {
      split: root?.classList.contains("split-workspace"),
      autoColor: root?.style.getPropertyValue("--pane-outline-color").trim(),
      activeZ: Number(getComputedStyle(active).zIndex),
      inactiveZ: Number(getComputedStyle(inactive).zIndex),
      outline: !!outline && getComputedStyle(outline).boxShadow.includes("2px"),
    };
  }, [activeFrame]);
  if (!focusPaint.split || focusPaint.autoColor !== "var(--color-accent)"
      || focusPaint.activeZ <= focusPaint.inactiveZ || !focusPaint.outline) {
    throw new Error(`[pane focus] the active outline is not painted above every pane: ${JSON.stringify(focusPaint)}`);
  }

  // Pane decoration belongs in Settings → Theme, not the title-bar palette
  // menu. The independent-theme toggle remains there, after every appearance
  // dial, so it is easy to find without displacing the main palette controls.
  await frameClick('button[title="Appearance"]', "appearance menu placement");
  await page.waitForFunction((sel) =>
    !!document.querySelector(sel)?.contentDocument?.querySelector('[data-setting="independent-themes"]'), activeFrame, { timeout: 5_000 });
  const menuPlacement = await page.evaluate((sel) => {
    const doc = document.querySelector(sel)?.contentDocument;
    const grain = doc?.querySelector('[data-appearance-section="film-grain"]');
    const independent = doc?.querySelector('[data-setting="independent-themes"]');
    return {
      paneControls: !!doc?.querySelector('[data-setting="split-pane-appearance"]'),
      independentAtEnd: !!grain && !!independent &&
        !!(grain.compareDocumentPosition(independent) & Node.DOCUMENT_POSITION_FOLLOWING),
    };
  }, activeFrame);
  if (menuPlacement.paneControls || !menuPlacement.independentAtEnd) {
    throw new Error(`[pane appearance] title-bar menu placement is wrong: ${JSON.stringify(menuPlacement)}`);
  }
  await frameClick('button[title="Appearance"]', "close appearance menu");

  // Split decoration is mounted only while two panes are placed. Exercise
  // the controls in the Theme tab, including the requested uniform outer
  // margin and internal gutter.
  await frameClick('button[title="Reader settings"]', "split pane appearance");
  await frameClick('button[aria-label="Theme"]', "split pane appearance");
  const decorationVisible = await page.evaluate((sel) =>
    !!document.querySelector(sel)?.contentDocument?.querySelector('[data-setting="split-pane-appearance"]'), activeFrame);
  if (!decorationVisible) throw new Error("[pane appearance] split-only controls are missing with two panes");
  await frameClick('[data-setting="split-pane-appearance"] [data-pane-outline-color="red"]', "outline color");
  await frameClick('[data-setting="split-pane-appearance"] [data-pane-corners="rounded"]', "rounded pane corners");
  await frameClick('[data-setting="pane-shadow"] [role="switch"]', "pane shadow");
  await page.evaluate(([sel, values]) => {
    const doc = document.querySelector(sel)?.contentDocument;
    for (const [setting, value] of values) {
      const input = doc?.querySelector(`[data-setting="${setting}"] input[type="range"]`);
      if (!input) throw new Error(`missing range for ${setting}`);
      const setter = Object.getOwnPropertyDescriptor(doc.defaultView.HTMLInputElement.prototype, "value").set;
      setter.call(input, String(value));
      input.dispatchEvent(new doc.defaultView.Event("input", { bubbles: true }));
    }
  }, [activeFrame, [["pane-gap", 12], ["pane-outline-width", 5]]]);
  const originalBounds = new Map(both.host.panes.map((pane) => [pane.paneId, pane.bounds]));
  const decorated = await waitForSettledLayout("split decoration gutter", (s) =>
    s.host?.panes?.length === 2 && s.host.panes.every((pane) => pane.bounds.width < originalBounds.get(pane.paneId).width - 4));
  const paneDecoration = await page.evaluate(([sel, activeId]) => {
    const doc = document.querySelector(sel)?.contentDocument;
    const root = doc?.querySelector(".reader-bg");
    const slot = doc?.querySelector("main");
    const slotRect = slot?.getBoundingClientRect();
    const entry = doc?.querySelector(`[data-pane-id="${activeId}"]`);
    const outline = entry?.querySelector(".pane-focus-outline");
    const entries = [...(doc?.querySelectorAll("[data-pane-id]") ?? [])];
    const active = entries.find((node) => node.dataset.paneActive === "true");
    const inactive = entries.find((node) => node !== active);
    return {
      color: root?.style.getPropertyValue("--pane-outline-color").trim(),
      width: root?.style.getPropertyValue("--pane-outline-width").trim(),
      radius: getComputedStyle(entry).borderRadius,
      shadow: getComputedStyle(entry).boxShadow,
      outline: getComputedStyle(outline).boxShadow,
      activeZ: Number(getComputedStyle(active).zIndex),
      inactiveZ: Number(getComputedStyle(inactive).zIndex),
      slot: { width: slotRect?.width, height: slotRect?.height },
      boxes: entries.map((node) => {
        const r = node.getBoundingClientRect();
        return {
          x: r.x - slotRect.left,
          right: r.right - slotRect.left,
          y: r.y - slotRect.top,
          bottom: r.bottom - slotRect.top,
          width: r.width,
        };
      }).sort((a, b) => a.x - b.x),
      outerMargins: {
        left: Math.min(...entries.map((node) => node.getBoundingClientRect().left - slotRect.left)),
        right: slotRect.right - Math.max(...entries.map((node) => node.getBoundingClientRect().right)),
        top: Math.min(...entries.map((node) => node.getBoundingClientRect().top - slotRect.top)),
        bottom: slotRect.bottom - Math.max(...entries.map((node) => node.getBoundingClientRect().bottom)),
      },
    };
  }, [activeFrame, decorated.host.activePane]);
  const visibleGap = Math.abs(paneDecoration.boxes[1].x - paneDecoration.boxes[0].right);
  const outerMargins = Object.values(paneDecoration.outerMargins);
  if (paneDecoration.color !== "#e56b64" || paneDecoration.width !== "5px" || paneDecoration.radius !== "10px" || paneDecoration.shadow === "none"
      || !paneDecoration.outline.includes("5px") || !paneDecoration.outline.includes("rgb(229, 107, 100)") || paneDecoration.activeZ <= paneDecoration.inactiveZ
      || Math.abs(visibleGap - 12) > 1 || outerMargins.some((margin) => Math.abs(margin - 12) > 1)) {
    throw new Error(`[pane appearance] controls did not paint the requested pane box: ${JSON.stringify({ paneDecoration, visibleGap })}`);
  }
  // Restore product defaults before the remainder of the lifecycle replay.
  await frameClick('[data-setting="split-pane-appearance"] [data-pane-outline-color="auto"]', "restore outline auto");
  await frameClick('[data-setting="split-pane-appearance"] [data-pane-corners="square"]', "restore square pane corners");
  await frameClick('[data-setting="pane-shadow"] [role="switch"]', "restore pane shadow");
  await page.evaluate(([sel, setting, value]) => {
    const doc = document.querySelector(sel)?.contentDocument;
    const input = doc?.querySelector(`[data-setting="${setting}"] input[type="range"]`);
    const setter = Object.getOwnPropertyDescriptor(doc.defaultView.HTMLInputElement.prototype, "value").set;
    setter.call(input, String(value));
    input.dispatchEvent(new doc.defaultView.Event("input", { bubbles: true }));
  }, [activeFrame, "pane-gap", 0]);
  await page.evaluate(([sel, setting, value]) => {
    const doc = document.querySelector(sel)?.contentDocument;
    const input = doc?.querySelector(`[data-setting="${setting}"] input[type="range"]`);
    const setter = Object.getOwnPropertyDescriptor(doc.defaultView.HTMLInputElement.prototype, "value").set;
    setter.call(input, String(value));
    input.dispatchEvent(new doc.defaultView.Event("input", { bubbles: true }));
  }, [activeFrame, "pane-outline-width", 2]);
  await page.evaluate((sel) => document.querySelector(sel).contentWindow.focus(), activeFrame);
  await page.keyboard.press("Escape");
  await page.waitForFunction((sel) =>
    !document.querySelector(sel)?.contentDocument?.querySelector('[role="dialog"][aria-label="Reader settings"]'), activeFrame);
  await waitForSettledLayout("split decoration restored", (s) =>
    s.host?.panes?.length === 2 && s.host.panes.every((pane) => {
      const original = originalBounds.get(pane.paneId);
      return Math.abs(pane.bounds.x - original.x) < 2 && Math.abs(pane.bounds.y - original.y) < 2
        && Math.abs(pane.bounds.width - original.width) < 2 && Math.abs(pane.bounds.height - original.height) < 2;
    }));

  const entries = await paneEntries();
  if (entries.length !== 2 || !entries.every((e) => e.visible)) {
    throw new Error(`[split] both panes must be shown and live: ${JSON.stringify(entries)}`);
  }
  if (entries.filter((e) => e.active).length !== 1) throw new Error(`[split] ${JSON.stringify(entries)} marks not exactly one active entry`);

  // Focus follows the pointer: a press inside the PDF pane makes it the
  // one active pane (the host's capture listener, before the content).
  await page.evaluate(([sel, id]) => {
    const f = document.querySelector(sel);
    const target = f.contentDocument.querySelector(`[data-pane-id="${id}"] [data-pane-root]`);
    if (!target) throw new Error(`pane ${id} has no root`);
    target.dispatchEvent(new f.contentWindow.PointerEvent("pointerdown", { bubbles: true, composed: true }));
  }, [activeFrame, pdfPane]);
  await waitFor("[split] the pressed PDF pane became active", (s) =>
    s.host?.activePane === pdfPane && s.host.panes.filter((p) => p.focused).length === 1, 10_000);

  // The divider: a real pointer drag moves the split's RATIO; both panes
  // are handed their new boxes and neither is recreated.
  const handle = await page.evaluate((sel) => {
    const f = document.querySelector(sel);
    const d = f.contentDocument.querySelector('[role="separator"][data-split-id]');
    if (!d) return null;
    const fr = f.getBoundingClientRect();
    const r = d.getBoundingClientRect();
    return { x: fr.left + r.left + r.width / 2, y: fr.top + r.top + r.height / 2 };
  }, activeFrame);
  if (!handle) throw new Error("[split] the host drew no divider");
  const beforeDrag = await snap();
  await page.mouse.move(handle.x, handle.y);
  await page.mouse.down();
  for (let step = 1; step <= 10; step += 1) {
    await page.mouse.move(handle.x - step * 15, handle.y);
  }
  await page.mouse.up();
  const dragged = await waitForSettledLayout("split: the divider drag re-laid both panes", (s) => {
    const p = s.host?.panes?.find((x) => x.paneId === pdfPane);
    return p && p.bounds.width < pdf.bounds.width - 100;
  }, 15_000);
  const afterDrag = assertSplitWorkspace(dragged, "split: dragged", pdfPane);
  if (dragged.host.panesCreated !== both.host.panesCreated) throw new Error("[split] a divider drag created panes");
  if (!(afterDrag.split.ratio >= 0.15 && afterDrag.split.ratio <= 0.85)) throw new Error(`[split] ratio ${afterDrag.split.ratio} escaped its clamp`);
  const widthSum = afterDrag.pdf.bounds.width + afterDrag.md.bounds.width;
  if (Math.abs(widthSum - (pdf.bounds.width + md.bounds.width)) > 2) throw new Error(`[split] the drag changed the total width (${widthSum})`);
  await assertPaneBox(dragged, "split: dragged pdf", afterDrag.pdf);
  await assertPaneBox(dragged, "split: dragged markdown", afterDrag.md);

  // Independent themes are a real pane-local appearance path, not merely a
  // per-pane map. Exercise the visible switch and base controls, and emulate
  // the engine's two published paper scopes while blend is active: the PDF's
  // local paper must not bleed into the MD pane or the shared workspace base.
  const appearance = 'button[title="Appearance"]';
  await page.evaluate(([sel, appearance]) => {
    const doc = document.querySelector(sel)?.contentDocument;
    doc?.querySelector(appearance)?.click();
  }, [activeFrame, appearance]);
  const themeSwitch = '[role="switch"][title="Independent theme for each pane"]';
  const switchReady = await page.evaluate(([sel, selector]) =>
    !!document.querySelector(sel)?.contentDocument?.querySelector(selector), [activeFrame, themeSwitch]);
  if (!switchReady) throw new Error("[pane themes] independent switch is missing in a two-pane workspace");
  await page.evaluate(([sel, selector]) => {
    document.querySelector(sel)?.contentDocument?.querySelector(selector)?.click();
  }, [activeFrame, themeSwitch]);
  await page.waitForFunction(([sel]) =>
    document.querySelector(sel)?.contentDocument?.querySelector(".reader-bg")?.classList.contains("independent-themes"), [activeFrame]);

  // The new MD pane had focus after the divider interaction/open. Explicitly
  // request it through the real pane capture path, then choose Dark in the
  // actual appearance menu.
  await page.evaluate(([sel, id]) => {
    const f = document.querySelector(sel);
    const target = f.contentDocument.querySelector(`[data-pane-id="${id}"] [data-pane-root]`);
    target.dispatchEvent(new f.contentWindow.PointerEvent("pointerdown", { bubbles: true, composed: true }));
  }, [activeFrame, md.paneId]);
  await waitFor("[pane themes] Markdown pane focused", (s) => s.host?.activePane === md.paneId, 10_000);
  await page.waitForFunction(([sel]) => !!document.querySelector(sel)?.contentDocument?.querySelector('button[title="Appearance"]'), [activeFrame], { timeout: 10_000 });
  await page.evaluate(([sel, appearance]) => {
    const doc = document.querySelector(sel)?.contentDocument;
    if (!doc?.querySelector('button[title="Dark"]')) doc?.querySelector(appearance)?.click();
  }, [activeFrame, appearance]);
  await page.waitForFunction(([sel]) => !!document.querySelector(sel)?.contentDocument?.querySelector('button[title="Dark"]'), [activeFrame], { timeout: 10_000 });
  await page.evaluate((sel) => document.querySelector(sel)?.contentDocument?.querySelector('button[title="Dark"]')?.click(), activeFrame);
  await page.waitForFunction(([sel, id]) => {
    const root = document.querySelector(sel)?.contentDocument?.querySelector(`[data-pane-id="${id}"] [data-pane-root]`);
    return root && getComputedStyle(root).getPropertyValue("--color-paper").trim() !== "#ffffff";
  }, [activeFrame, md.paneId], { timeout: 10_000 });
  const mdDark = await page.evaluate(([sel, id]) => {
    const doc = document.querySelector(sel)?.contentDocument;
    const root = doc?.querySelector(`[data-pane-id="${id}"] [data-pane-root]`);
    return root ? getComputedStyle(root).getPropertyValue("--color-paper").trim() : "";
  }, [activeFrame, md.paneId]);
  const selectedMdBase = await page.evaluate((sel) =>
    document.querySelector(sel)?.contentDocument?.querySelector('button[title="Dark"]')?.getAttribute("aria-pressed"), [activeFrame]);
  if (!mdDark || mdDark === "#ffffff" || selectedMdBase !== "true") {
    throw new Error(`[pane themes] MD's selected Dark paper/menu state did not land: ${JSON.stringify({ mdDark, selectedMdBase })}`);
  }

  // Deliberately divergent test colours make the ownership contract obvious:
  // shared red on :root, local green on the PDF root, and MD's own dark base.
  const blendScopes = await page.evaluate(([sel, pdfId, mdId]) => {
    const doc = document.querySelector(sel)?.contentDocument;
    const workspace = doc?.querySelector(".reader-bg");
    const pdfRoot = doc?.querySelector(`[data-pane-id="${pdfId}"] [data-pane-root]`);
    const mdRoot = doc?.querySelector(`[data-pane-id="${mdId}"] [data-pane-root]`);
    doc.documentElement.style.setProperty("--pdf-paper-baked", "#d02020");
    pdfRoot.style.setProperty("--pane-pdf-paper-baked", "#a0c060");
    // Blend is workspace state every pane frame mirrors onto its own
    // `.reader-bg`: the poke lands on all of them, as the mirror would.
    doc.querySelectorAll(".reader-bg").forEach((bg) => bg.classList.add("blend"));
    return {
      sharedBackdrop: getComputedStyle(workspace).backgroundColor,
      pdfPane: getComputedStyle(pdfRoot).backgroundColor,
      mdPane: getComputedStyle(mdRoot).backgroundColor,
      localPdf: getComputedStyle(pdfRoot).getPropertyValue("--pane-paper").trim(),
      localMd: getComputedStyle(mdRoot).getPropertyValue("--pane-paper").trim(),
    };
  }, [activeFrame, pdfPane, md.paneId]);
  if (blendScopes.pdfPane !== "rgb(160, 192, 96)" || blendScopes.mdPane === "rgb(160, 192, 96)" || blendScopes.sharedBackdrop === "rgb(208, 32, 32)") {
    throw new Error(`[pane themes] blended PDF paper escaped its pane: ${JSON.stringify(blendScopes)}`);
  }
  await page.evaluate(([sel]) => {
    const doc = document.querySelector(sel)?.contentDocument;
    doc?.querySelectorAll(".reader-bg").forEach((bg) => bg.classList.remove("blend"));
    doc?.documentElement.style.removeProperty("--pdf-paper-baked");
    doc?.querySelectorAll("[data-pane-root]").forEach((root) => root.style.removeProperty("--pane-pdf-paper-baked"));
  }, [activeFrame]);

  // Now theme the PDF itself. This checks that changing the focused PDF
  // edits its own scoped canvas pipeline, while the adjacent Markdown retains
  // its distinct look. The engine smoke separately asserts independent pixel
  // baking for two simultaneously live PDF sessions.
  await page.evaluate(([sel, id]) => {
    const f = document.querySelector(sel);
    f.contentDocument.querySelector(`[data-pane-id="${id}"] [data-pane-root]`)
      .dispatchEvent(new f.contentWindow.PointerEvent("pointerdown", { bubbles: true, composed: true }));
  }, [activeFrame, pdfPane]);
  await waitFor("[pane themes] PDF pane focused", (s) => s.host?.activePane === pdfPane, 10_000);
  await page.waitForFunction(([sel]) => !!document.querySelector(sel)?.contentDocument?.querySelector('button[title="Appearance"]'), [activeFrame], { timeout: 10_000 });
  await page.evaluate(([sel, appearance]) => {
    const doc = document.querySelector(sel)?.contentDocument;
    if (!doc?.querySelector('button[title="Dim"]')) doc?.querySelector(appearance)?.click();
  }, [activeFrame, appearance]);
  await page.waitForFunction(([sel]) => !!document.querySelector(sel)?.contentDocument?.querySelector('button[title="Dim"]'), [activeFrame], { timeout: 10_000 });
  await page.evaluate((sel) => document.querySelector(sel)?.contentDocument?.querySelector('button[title="Dim"]')?.click(), activeFrame);
  await page.waitForFunction(([sel, pdfId]) => {
    const root = document.querySelector(sel)?.contentDocument?.querySelector(`[data-pane-id="${pdfId}"] [data-pane-root]`);
    return root && !!getComputedStyle(root).getPropertyValue("--canvas-filter").trim()
      && getComputedStyle(root).getPropertyValue("--color-paper").trim() !== "#ffffff";
  }, [activeFrame, pdfPane], { timeout: 10_000 });
  const pdfLook = await page.evaluate(([sel, pdfId, mdId]) => {
    const doc = document.querySelector(sel)?.contentDocument;
    const pdfRoot = doc?.querySelector(`[data-pane-id="${pdfId}"] [data-pane-root]`);
    const mdRoot = doc?.querySelector(`[data-pane-id="${mdId}"] [data-pane-root]`);
    const cs = getComputedStyle(pdfRoot);
    return {
      pdfPaper: cs.getPropertyValue("--color-paper").trim(),
      pdfFilter: cs.getPropertyValue("--canvas-filter").trim(),
      mdPaper: getComputedStyle(mdRoot).getPropertyValue("--color-paper").trim(),
      independent: doc.querySelector(".reader-bg").classList.contains("independent-themes"),
    };
  }, [activeFrame, pdfPane, md.paneId]);
  const selectedPdfBase = await page.evaluate((sel) =>
    document.querySelector(sel)?.contentDocument?.querySelector('button[title="Dim"]')?.getAttribute("aria-pressed"), [activeFrame]);
  if (!pdfLook.independent || !pdfLook.pdfFilter || pdfLook.pdfPaper === pdfLook.mdPaper || selectedPdfBase !== "true") {
    throw new Error(`[pane themes] PDF's own Dim look/menu state did not diverge from MD: ${JSON.stringify({ pdfLook, selectedPdfBase })}`);
  }
  // Exercise the opposite family in that SAME focused PDF pane. It must be
  // possible to choose Light for the PDF while the adjacent Markdown remains
  // Dark; this catches a global base control masquerading as per-pane UI.
  await page.evaluate((sel) => document.querySelector(sel)?.contentDocument?.querySelector('button[title="Light"]')?.click(), activeFrame);
  await page.waitForFunction(([sel, pdfId, mdId]) => {
    const doc = document.querySelector(sel)?.contentDocument;
    const pdfRoot = doc?.querySelector(`[data-pane-id="${pdfId}"] [data-pane-root]`);
    const mdRoot = doc?.querySelector(`[data-pane-id="${mdId}"] [data-pane-root]`);
    return pdfRoot && mdRoot
      && getComputedStyle(pdfRoot).getPropertyValue("--color-paper").trim() === "#ffffff"
      && getComputedStyle(mdRoot).getPropertyValue("--color-paper").trim() !== "#ffffff";
  }, [activeFrame, pdfPane, md.paneId], { timeout: 10_000 });
  const splitLooks = await page.evaluate(([sel, pdfId, mdId]) => {
    const doc = document.querySelector(sel)?.contentDocument;
    const pdfRoot = doc?.querySelector(`[data-pane-id="${pdfId}"] [data-pane-root]`);
    const mdRoot = doc?.querySelector(`[data-pane-id="${mdId}"] [data-pane-root]`);
    return {
      pdf: getComputedStyle(pdfRoot).getPropertyValue("--color-paper").trim(),
      markdown: getComputedStyle(mdRoot).getPropertyValue("--color-paper").trim(),
      lightSelected: doc?.querySelector('button[title="Light"]')?.getAttribute("aria-pressed"),
    };
  }, [activeFrame, pdfPane, md.paneId]);
  if (splitLooks.lightSelected !== "true") throw new Error(`[pane themes] the PDF control did not follow the focused pane: ${JSON.stringify(splitLooks)}`);
  // Restore the toggle before the existing split-close lifecycle assertions.
  await page.evaluate(([sel, appearance, selector]) => {
    const doc = document.querySelector(sel)?.contentDocument;
    if (!doc?.querySelector(selector)) doc?.querySelector(appearance)?.click();
    doc?.querySelector(selector)?.click();
  }, [activeFrame, appearance, themeSwitch]);
  await page.waitForFunction(([sel]) =>
    !document.querySelector(sel)?.contentDocument?.querySelector(".reader-bg")?.classList.contains("independent-themes"), [activeFrame]);
  console.log(`[pane themes] independent MD/PDF looks, mixed-format blend isolation: ${JSON.stringify({ mdDark, blendScopes, pdfLook, splitLooks })}`);

  // Close the PDF pane through the host's own control: the Markdown pane
  // keeps its session, takes focus and the whole slot; the PDF's engine
  // session and every raster it owned go with its pane.
  await page.evaluate(([sel, id]) => {
    const btn = document.querySelector(sel)?.contentDocument?.querySelector(`[data-pane-close="${id}"] button`);
    if (!btn) throw new Error(`pane ${id} has no close control`);
    btn.click();
  }, [activeFrame, pdfPane]);
  const alone = await waitForSettledLayout("split: the PDF pane closed, the Markdown pane lives", (s) =>
    s.host?.panes?.length === 1 && s.host.panes[0].paneId === md.paneId &&
    s.engine?.sessionsLive === 0 && s.engine.pageCanvasBytesEst === 0 &&
    s.engine.thumbnailRasterBytesEst === 0, 30_000);
  const survivor = alone.host.panes[0];
  await frameClick('button[title="Reader settings"]', "hide pane controls outside split");
  await frameClick('button[aria-label="Theme"]', "hide pane controls outside split");
  const splitControlsAfterClose = await page.evaluate((sel) =>
    !!document.querySelector(sel)?.contentDocument?.querySelector('[data-setting="split-pane-appearance"]'), activeFrame);
  if (splitControlsAfterClose) throw new Error("[pane appearance] split-only controls remained with one pane");
  await page.evaluate((sel) => document.querySelector(sel).contentWindow.focus(), activeFrame);
  await page.keyboard.press("Escape");
  await page.waitForFunction((sel) =>
    !document.querySelector(sel)?.contentDocument?.querySelector('[role="dialog"][aria-label="Reader settings"]'), activeFrame);
  if (survivor.lifecycle !== "ready" || survivor.resources.documentSession !== true || survivor.format !== "markdown") {
    throw new Error(`[split] the surviving pane is not reading: ${JSON.stringify(survivor)}`);
  }
  if (alone.host.activePane !== md.paneId || survivor.focused !== true) throw new Error(`[split] the survivor did not take focus (${alone.host.activePane})`);
  if (alone.host.layout?.leaf !== md.paneId) throw new Error(`[split] layout after close: ${JSON.stringify(alone.host.layout)}`);
  if (Math.abs(survivor.bounds.width - widthSum) > 2) throw new Error(`[split] the survivor did not take the slot (${survivor.bounds.width} of ${widthSum})`);
  await assertPaneBox(alone, "split: survivor", survivor);
  if (alone.disposalEpoch !== 3) throw new Error(`[split] disposal epoch ${alone.disposalEpoch} after the pane close, expected 3 (two opens, one pane dispose)`);
  if ((await paneEntries()).length !== 1) throw new Error("[split] the closed pane's entry is still in the slot");
  assertNoNewPanics("split workspace", panicsBefore);

  summary.splitWorkspace = {
    panes: both.host.panes.map((p) => ({ paneId: p.paneId, format: p.format, bounds: p.bounds })),
    dragRatio: afterDrag.split.ratio,
    dragRenders: dragged.engine.rendersStarted - beforeDrag.engine.rendersStarted,
    survivor: survivor.paneId,
  };
  stages.afterSplitWorkspace = alone;
  // Four mints: the two opens, the PDF pane's dispose, the survivor's.
  await closeAndWaitBaseline("split workspace", false, 4);
  console.log(`split workspace: PDF | Markdown, divider ratio ${afterDrag.split.ratio}, closed the PDF, the Markdown pane read on`);
}

// --- Stage 13b: independent themes at the split boundary -----------------
currentStage = "stage13b-pane-theme-boundary";
// The mode needs the split it serves. Closing back to ONE pane stands it
// down and hands the surviving pane's colour to the window theme, so the
// pane left and the shared chrome around it agree; the stored toggle
// survives, so the next split brings the mode back by itself, with the
// survivor's colour untouched and the new pane in a colour of its own.
{
  const panicsBefore = panicCount;
  const openedTheme = await openLight(PEARLS);
  const themePane = openedTheme.host.panes[0].paneId;

  // --- Stage 13c: the rail's thumbnails follow the theme -----------------
  currentStage = "stage13c-rail-thumb-themes";
  // The rail's canvases live in the workspace host and hold a picture the
  // PANE's engine baked for the look in force when it was requested. A
  // theme change must reach them: the host re-renders every settled cell
  // when its pane says the look was re-baked, and the frame re-bakes that
  // cell's raster from the raw one instead of answering with a picture of
  // the look before it. The rail is measured by DOWN-SAMPLING the painted
  // cards' own pixels in the active frame's document, so the numbers are
  // the engine's bake and nothing a CSS layer paints over it.
  {
    const panicsBeforeThumbs = panicCount;
    await frameClick('button[title="Toggle sidebar"]', "[thumb themes] the thumbnail rail");
    const railPixels = () => page.evaluate((sel) => {
      const doc = document.querySelector(sel)?.contentDocument;
      if (!doc) return { count: 0, mean: null };
      const nodes = doc.querySelectorAll("#thumb-scroll canvas.thumb-canvas");
      let probe = doc.defaultView.__thumbProbe;
      if (!probe) {
        probe = doc.createElement("canvas");
        probe.width = 16;
        probe.height = 16;
        doc.defaultView.__thumbProbe = probe;
      }
      const ctx = probe.getContext("2d", { willReadFrequently: true });
      let count = 0;
      let sum = 0;
      for (const cv of nodes) {
        if (!cv.width || !cv.height || cv.classList.contains("thumb-canvas-blank")) continue;
        ctx.clearRect(0, 0, 16, 16);
        ctx.drawImage(cv, 0, 0, 16, 16);
        const px = ctx.getImageData(0, 0, 16, 16).data;
        let lum = 0;
        for (let i = 0; i < px.length; i += 4) lum += 0.2126 * px[i] + 0.7152 * px[i + 1] + 0.0722 * px[i + 2];
        sum += lum / (px.length / 4);
        count += 1;
      }
      return { count, mean: count ? sum / count : null };
    }, activeFrame);
    const waitRail = async (label, ok, timeoutMs = 25_000) => {
      const deadline = Date.now() + timeoutMs;
      let last = null;
      while (Date.now() < deadline) {
        last = await railPixels();
        if (last.count >= 2 && ok(last)) return last;
        await page.waitForTimeout(100);
      }
      throw new Error(`[thumb themes] ${label}: ${JSON.stringify(last)}`);
    };
    // The cards: painted, uncovered, at least a row of them.
    const first = await waitRail("the rail painted its first thumbnails", () => true);
    // Which way to move the base: the dials are a 3-way choice, and the
    // window's look is whatever the run left behind, so the stage reads the
    // current base and flips it rather than assuming one.
    await frameClick('button[title="Appearance"]', "[thumb themes] the appearance menu");
    const baseNow = await page.evaluate((sel) => {
      const doc = document.querySelector(sel)?.contentDocument;
      for (const name of ["Light", "Dark", "Dim"]) {
        const el = doc?.querySelector(`button[title="${name}"]`);
        if (el?.getAttribute("aria-pressed") === "true") return name;
      }
      return null;
    }, activeFrame);
    if (!baseNow) throw new Error("[thumb themes] the appearance menu shows no base to move");
    const moved = baseNow === "Light" ? "Dark" : "Light";
    // Dark inverts the bake and Dim darkens it; both read far darker than
    // Light, which is the direction this stage asserts.
    const darker = moved !== "Light";
    await frameClick(`button[title="${moved}"]`, `[thumb themes] the ${moved} base`);
    const after = await waitRail(`the thumbs follow the ${moved} base`,
      (t) => (darker ? t.mean < first.mean - 30 : t.mean > first.mean + 30));
    // Put the base back the way the run left it, and the picture with it.
    await frameClick(`button[title="${baseNow}"]`, `[thumb themes] the ${baseNow} base, back`);
    const back = await waitRail("the thumbs follow the base back",
      (t) => (darker ? t.mean > first.mean - 15 : t.mean < first.mean + 15));
    await frameClick('button[title="Appearance"]', "[thumb themes] close the appearance menu");
    // The rail closes from its own header — the toolbar's toggle only exists
    // while the rail is shut — and the cards release with the slide.
    await frameClick('button[title="Close sidebar"]', "[thumb themes] close the thumbnail rail");
    await page.waitForFunction((sel) => {
      const doc = document.querySelector(sel)?.contentDocument;
      const cards = doc?.querySelectorAll("#thumb-scroll canvas.thumb-canvas") ?? [];
      for (const cv of cards) if (cv.width) return false;
      return true;
    }, activeFrame, { timeout: 10_000 });
    console.log(
      `[thumb themes] the rail's cards read ${first.mean.toFixed(1)} under ${baseNow}, ` +
        `${after.mean.toFixed(1)} under ${moved}, ${back.mean.toFixed(1)} back (${after.count} cards)`,
    );
    summary.railThumbThemes = {
      base: baseNow, moved, from: first.mean, to: after.mean, back: back.mean, cards: after.count,
    };
    assertNoNewPanics("rail thumb themes", panicsBeforeThumbs);
  }
  currentStage = "stage13b-pane-theme-boundary";
  if ((await openIn(SPLIT_NOTES, "right")) !== true) throw new Error("[theme boundary] the host refused the Markdown pane beside the PDF");

  const themed = await waitForSettledLayout("[theme boundary] PDF | Markdown both ready", (s) =>
    s.host?.panes?.length === 2 &&
    s.host.panes.every((p) => p.lifecycle === "ready" && p.resources?.documentSession === true), 60_000);
  const mdTheme = themed.host.panes.find((p) => p.paneId !== themePane);
  if (themed.host.activePane !== mdTheme.paneId) throw new Error(`[theme boundary] active pane ${themed.host.activePane}, expected the new Markdown pane ${mdTheme.paneId}`);

  /** The window's paper and every pane's own, in the active frame's
   *  document. A pane root with no look of its own carries no tokens, and
   *  a custom property inherits, so it answers the window's value — which
   *  is exactly what the single-pane state must show. */
  const papers = () => page.evaluate((sel) => {
    const doc = document.querySelector(sel)?.contentDocument;
    if (!doc) return null;
    // A custom property's value is raw text, and one colour has more than one
    // spelling in this app: the window's root reads `#fff` where a pane root
    // reads `#ffffff`. Every value therefore goes through a colour property
    // and comes back as the browser's own computed form before anything is
    // compared, so equal colours compare equal and different ones differ.
    const paper = (el) => {
      if (!el) return "";
      const raw = getComputedStyle(el).getPropertyValue("--color-paper").trim();
      if (!raw) return "";
      const probe = doc.createElement("span");
      probe.style.color = raw;
      if (!probe.style.color) return raw;
      doc.body.appendChild(probe);
      const computed = doc.defaultView.getComputedStyle(probe).color;
      probe.remove();
      return computed || raw;
    };
    const panes = {};
    for (const entry of doc.querySelectorAll("[data-pane-id]")) {
      const frame = entry.querySelector("iframe.pane-frame:not([data-frame-hidden])");
      // The root is under the entry, or in the pane frame that entry holds
      // (the pane's surface and its document frame are separate documents;
      // only one of them carries the root).
      const root = entry.querySelector("[data-pane-root]")
        ?? frame?.contentDocument?.querySelector("[data-pane-root]");
      panes[entry.dataset.paneId] = paper(root);
    }
    return {
      window: paper(doc.documentElement),
      independent: !!doc.querySelector(".reader-bg.independent-themes"),
      panes,
    };
  }, activeFrame);
  const waitPapers = async (label, predicate, timeoutMs = 10_000) => {
    const deadline = Date.now() + timeoutMs;
    let last = null;
    while (Date.now() < deadline) {
      last = await papers();
      if (last && predicate(last)) return last;
      await page.waitForTimeout(100);
    }
    throw new Error(`[theme boundary] ${label}: ${JSON.stringify(last)}`);
  };

  // Which pane keeps the window's look is the host's ACTIVE pane at the
  // moment the mode comes on, so hand the Markdown pane focus the way a
  // reader does before flipping the switch: a press on the pane's own root.
  // The root is looked up the same way `papers()` looks it up — under the
  // pane entry, or in the pane frame that entry holds — because the pane's
  // surface and the pane's document frame are separate documents and only
  // one of them carries the root. The event is built by the constructor of
  // the window that owns that root.
  await page.evaluate(([sel, id]) => {
    const doc = document.querySelector(sel)?.contentDocument;
    const entry = doc?.querySelector(`[data-pane-id="${id}"]`);
    const frame = entry?.querySelector("iframe.pane-frame:not([data-frame-hidden])");
    const target = entry?.querySelector("[data-pane-root]")
      ?? frame?.contentDocument?.querySelector("[data-pane-root]");
    if (!target) throw new Error(`pane ${id} has no root of its own`);
    const win = target.ownerDocument.defaultView;
    target.dispatchEvent(new win.PointerEvent("pointerdown", { bubbles: true, composed: true }));
  }, [activeFrame, mdTheme.paneId]);
  await waitFor("[theme boundary] the Markdown pane focused", (s) => s.host?.activePane === mdTheme.paneId, 10_000);

  await frameClick('button[title="Appearance"]', "[theme boundary] the appearance menu");
  await frameClick('[data-setting="independent-themes"] [role="switch"]', "[theme boundary] independent themes on");
  // The pane the open created is focused, so IT keeps the window's look and
  // the PDF already on screen takes a tint of its own: the two panes and the
  // window are three different papers, which is the state that must not
  // survive the collapse.
  const split = await waitPapers("the split shows a look of its own, apart from the window",
    (p) => p.independent && p.panes[mdTheme.paneId] === p.window && p.panes[themePane] !== p.window);

  // The collapse: the Markdown pane goes, the tinted PDF pane stays.
  await page.evaluate(([sel, id]) => {
    const btn = document.querySelector(sel)?.contentDocument?.querySelector(`[data-pane-close="${id}"] button`);
    if (!btn) throw new Error(`pane ${id} has no close control`);
    btn.click();
  }, [activeFrame, mdTheme.paneId]);
  const aloneTheme = await waitForSettledLayout("[theme boundary] the last split collapsed", (s) =>
    s.host?.panes?.length === 1 && s.host.panes[0].paneId === themePane, 30_000);
  if (aloneTheme.host.activePane !== themePane) throw new Error(`[theme boundary] the survivor did not take focus (${aloneTheme.host.activePane})`);
  const collapsed = await waitPapers("the mode stood down and the window took the pane's colour",
    (p) => !p.independent && p.window === split.panes[themePane] && p.panes[themePane] === split.panes[themePane]);

  // The stored toggle is on: the next split brings the mode back, the
  // survivor keeps the colour it had, and the new pane gets its own.
  if ((await openIn(SPLIT_NOTES, "right")) !== true) throw new Error("[theme boundary] the host refused the pane that carries the mode back");
  const resumed = await waitForSettledLayout("[theme boundary] the mode returns with the split", (s) =>
    s.host?.panes?.length === 2 &&
    s.host.panes.every((p) => p.lifecycle === "ready" && p.resources?.documentSession === true), 60_000);
  const newcomer = resumed.host.panes.find((p) => p.paneId !== themePane);
  const again = await waitPapers("the mode is back, the survivor kept its colour, the new pane has one",
    (p) => p.independent && p.panes[themePane] === split.panes[themePane] &&
      p.panes[newcomer.paneId] !== p.panes[themePane]);

  // The facts, before anything is put back: a failure in the restore below
  // still leaves the numbers this stage proved in the log.
  summary.paneThemeBoundary = {
    pane: themePane, window: split.window, split: split.panes,
    collapsedWindow: collapsed.window, resumed: again.panes,
  };
  console.log(`pane theme boundary: the split's papers ${JSON.stringify(split.panes)} against the window ${split.window}; the collapse promoted ${collapsed.window} and stood the mode down; the next split brought it back as ${JSON.stringify(again.panes)}`);

  // Restore the toggle for the stages that follow. The popover the mode-on
  // click opened is still on screen — a press inside a pane does not dismiss
  // it — so the menu is reopened only if that stopped being true.
  const switchOnScreen = () => page.evaluate((sel) =>
    !!document.querySelector(sel)?.contentDocument
      ?.querySelector('[data-setting="independent-themes"] [role="switch"]'), activeFrame);
  if (!(await switchOnScreen())) {
    await frameClick('button[title="Appearance"]', "[theme boundary] the appearance menu, reopened");
  }
  await frameClick('[data-setting="independent-themes"] [role="switch"]', "[theme boundary] independent themes off");
  await page.waitForFunction((sel) =>
    !document.querySelector(sel)?.contentDocument?.querySelector(".reader-bg.independent-themes"), activeFrame, { timeout: 5_000 });
  await frameClick('button[title="Appearance"]', "[theme boundary] close the appearance menu");
  assertNoNewPanics("independent theme boundary", panicsBefore);
  const beforeThemeClose = await snap();
  await closeAndWaitBaseline("pane theme boundary", false, beforeThemeClose.disposalEpoch + beforeThemeClose.host.panes.length);
}

// --- Stage 13d: independent textures at the split boundary ----------------
currentStage = "stage13d-pane-texture-boundary";
// The texture family has its own per-pane mode, the same shape as the colour
// one and routed by the same rule: turning it on keeps the focused pane's
// pattern and hands every other pane a mode of its own; a routed pick — the
// mode or either dial — lands on the focused pane alone and on nothing beside
// it; and switching the mode off folds the window's texture back over every
// pane. The last split's collapse promotes the survivor's texture into the
// window theme, the way the colour stage proves one turn earlier, so the
// texture a reader ends on is the texture the next launch opens with. A
// Markdown pane is on stage deliberately: its pattern rides the scroller that
// IS its paper, which is the half of the contract that used to be missing —
// and the half whose absence used to hide the picker entirely.
{
  const panicsBeforeTexture = panicCount;
  const openedTextured = await openLight(PEARLS);
  const pdfPane = openedTextured.host.panes[0].paneId;
  if ((await openIn(SPLIT_NOTES, "right")) !== true) throw new Error("[texture boundary] the host refused the Markdown pane beside the PDF");
  const textured = await waitForSettledLayout("[texture boundary] PDF | Markdown both ready", (s) =>
    s.host?.panes?.length === 2 &&
    s.host.panes.every((p) => p.lifecycle === "ready" && p.resources?.documentSession === true), 60_000);
  const mdPane = textured.host.panes.find((p) => p.paneId !== pdfPane).paneId;
  if (textured.host.activePane !== mdPane) throw new Error(`[texture boundary] active pane ${textured.host.activePane}, expected the new Markdown pane ${mdPane}`);

  // The `texture-*` class is the naming contract `styles/textures.css` and
  // `TextureMode::css_class` share, restated here on purpose: a mode that
  // renames its class in Rust without renaming it in the stylesheet paints
  // nothing, and that is exactly the bug class this stage exists to catch.
  const CLASS_OF = {
    None: null,
    "Real paper": "texture-paper",
    Lined: "texture-lined",
    Grid: "texture-grid",
    Dotted: "texture-dotted",
    Cross: "texture-cross",
  };

  // Per pane: the class its carriers wear — a PDF page host, or the reflowable
  // scroller for a text document — and the two dials its root paints as its
  // OWN. A pane with no look of its own paints neither and reads null,
  // inheriting the window; that difference is what turns this probe into a
  // routing assert rather than a look at the same number twice. `chosen` is
  // the menu's own answer: which mode the picker marks pressed, read from the
  // same `aria-pressed` the reader sees.
  const textures = () => page.evaluate((sel) => {
    const doc = document.querySelector(sel)?.contentDocument;
    if (!doc) return null;
    const read = (root) => {
      const carrier = root.querySelector(".pdf-page")
        ?? root.querySelector(".paginated-scroller, .tx-stream, .tx-strip");
      const mode = [...(carrier?.classList ?? [])].find((c) => c.startsWith("texture-")) ?? null;
      return {
        mode,
        opacity: root.style.getPropertyValue("--texture-opacity").trim() || null,
        scale: root.style.getPropertyValue("--texture-scale-user").trim() || null,
      };
    };
    const panes = {};
    for (const entry of doc.querySelectorAll("[data-pane-id]")) {
      const frame = entry.querySelector("iframe.pane-frame:not([data-frame-hidden])");
      // The root is under the entry, or in the pane frame that entry holds —
      // the pane's surface and the pane's document frame are separate
      // documents, and only one of them carries the root.
      const root = entry.querySelector("[data-pane-root]")
        ?? frame?.contentDocument?.querySelector("[data-pane-root]");
      panes[entry.dataset.paneId] = root
        ? read(root)
        : { mode: "no-root", opacity: null, scale: null };
    }
    const grid = doc.querySelector('[data-appearance-section="page-texture"] .grid-cols-3');
    return {
      windowDial: doc.documentElement.style.getPropertyValue("--texture-opacity").trim() || null,
      independent: !!doc.querySelector(".reader-bg.independent-themes"),
      section: !!grid,
      chosen: (grid?.querySelector('[aria-pressed="true"]')?.textContent ?? "").replace(/\s+/g, " ").trim(),
      panes,
    };
  }, activeFrame);
  const waitTextures = async (label, predicate, timeoutMs = 20_000) => {
    const deadline = Date.now() + timeoutMs;
    let last = null;
    while (Date.now() < deadline) {
      last = await textures();
      if (last && predicate(last)) return last;
      await page.waitForTimeout(100);
    }
    throw new Error(`[texture boundary] ${label}: ${JSON.stringify(last)}`);
  };
  // The picker's own clicks, by the label a reader reads: the mode chips carry
  // no test-only attribute, and the button whose text says "Grid" is the
  // button the menu offers for Grid.
  const clickTexture = (name) => page.evaluate(([sel, name]) => {
    const doc = document.querySelector(sel)?.contentDocument;
    const buttons = [...(doc?.querySelectorAll('[data-appearance-section="page-texture"] .grid-cols-3 button') ?? [])];
    const btn = buttons.find((el) => (el.textContent ?? "").replace(/\s+/g, " ").trim() === name);
    if (!btn) throw new Error(`the texture grid has no "${name}" button (${buttons.length} buttons)`);
    btn.click();
  }, [activeFrame, name]);
  const dialTextureOpacity = (value) => page.evaluate(([sel, value]) => {
    const doc = document.querySelector(sel)?.contentDocument;
    const input = doc?.querySelector('[data-appearance-section="page-texture"] input[type="range"][aria-label="Texture opacity"]');
    if (!input) throw new Error("the texture section has no opacity dial");
    const setter = Object.getOwnPropertyDescriptor(doc.defaultView.HTMLInputElement.prototype, "value").set;
    setter.call(input, String(value));
    input.dispatchEvent(new doc.defaultView.Event("input", { bubbles: true }));
  }, [activeFrame, value]);

  await frameClick('button[title="Appearance"]', "[texture boundary] the appearance menu");
  // The picker is there for the Markdown pane: the section used to stand down
  // while the focused document was reflowable, on the theory that only a
  // raster can carry a pattern.
  await waitTextures("the texture section shows while a Markdown pane is focused", (p) => p.section);
  const atRest = await waitTextures("the split rests on the window's own texture",
    (p) => p.chosen in CLASS_OF && p.panes[pdfPane].mode === p.panes[mdPane].mode
      && p.panes[pdfPane].opacity === null && p.panes[mdPane].opacity === null);

  // The mode comes on. The pane in front — the Markdown one the reader just
  // opened and is looking at — keeps the texture it shows; the PDF beside it
  // is handed a different mode, chosen at random out of the ones not already
  // on screen, and each pane now paints its own two dials.
  await frameClick('[data-setting="independent-textures"] [role="switch"]', "[texture boundary] independent textures on");
  const perPane = await waitTextures("each pane took a texture of its own",
    (p) => p.panes[mdPane].mode === atRest.panes[mdPane].mode
      && p.panes[pdfPane].mode !== p.panes[mdPane].mode
      && p.panes[pdfPane].opacity !== null && p.panes[mdPane].opacity !== null);

  // Both per-pane modes at once, in the order a reader would try them. Colour
  // independence joining a split that already textures per pane must give every
  // pane a colour of its own WITHOUT shuffling the patterns it is showing: a
  // switch owns the family it re-seeds, and the map survives the colour mode
  // folding for the same reason — the texture preference never asked for either.
  await frameClick('[data-setting="independent-themes"] [role="switch"]', "[texture boundary] independent themes beside the texture mode");
  const both = await waitTextures("each pane took a colour of its own, patterns unchanged",
    (p) => p.independent && p.panes[pdfPane].mode === perPane.panes[pdfPane].mode
      && p.panes[mdPane].mode === perPane.panes[mdPane].mode);
  await frameClick('[data-setting="independent-themes"] [role="switch"]', "[texture boundary] independent themes off again");
  const colourOff = await waitTextures("the textures stayed per pane when colour stood down",
    (p) => !p.independent && p.panes[pdfPane].mode === both.panes[pdfPane].mode
      && p.panes[mdPane].mode === both.panes[mdPane].mode
      && p.panes[pdfPane].mode !== p.panes[mdPane].mode);

  // A pick, then a dial, with the Markdown pane in front: both move THAT pane
  // and leave its neighbour exactly as the mode left it. Before the fix the
  // class never moved at all — a per-pane edit lands in the pane's own look,
  // not in the settings a pane's copy of the world is read from.
  const options = await page.evaluate((sel) => [...(document.querySelector(sel)?.contentDocument
    ?.querySelectorAll('[data-appearance-section="page-texture"] .grid-cols-3 button') ?? [])]
    .map((el) => (el.textContent ?? "").replace(/\s+/g, " ").trim()), activeFrame);
  // A mode the Markdown pane is not already wearing and the PDF pane will not
  // be caught wearing either, so the pick below can only pass by moving one
  // pane; "None" is out because its dials are inert by design.
  const target = options.find((name) => name !== atRest.chosen
    && CLASS_OF[name] && CLASS_OF[name] !== perPane.panes[pdfPane].mode);
  if (!target) throw new Error(`[texture boundary] the grid offers nothing to switch to: ${JSON.stringify(options)}`);
  await clickTexture(target);
  await waitTextures(`the Markdown pane took ${target} and the PDF did not`,
    (p) => p.chosen === target && p.panes[mdPane].mode === CLASS_OF[target]
      && p.panes[pdfPane].mode === colourOff.panes[pdfPane].mode);
  dialTextureOpacity(40);
  const dialed = await waitTextures("the Markdown pane's own opacity moved, its neighbour's did not",
    (p) => p.panes[mdPane].opacity === "0.400"
      && p.panes[pdfPane].opacity === perPane.panes[pdfPane].opacity
      && p.panes[pdfPane].opacity === p.windowDial);

  // The collapse: the Markdown pane, whose texture the window never saw,
  // goes. The surviving PDF pane's own texture is promoted into the window
  // theme as the mode stands down, and the pane stops painting dials of its
  // own because the window now says the same thing.
  await page.evaluate(([sel, id]) => {
    const btn = document.querySelector(sel)?.contentDocument?.querySelector(`[data-pane-close="${id}"] button`);
    if (!btn) throw new Error(`pane ${id} has no close control`);
    btn.click();
  }, [activeFrame, mdPane]);
  await waitForSettledLayout("[texture boundary] the Markdown pane closed", (s) =>
    s.host?.panes?.length === 1 && s.host.panes[0].paneId === pdfPane, 30_000);
  const promoted = await waitTextures("the window took the survivor's texture",
    (p) => p.panes[pdfPane].mode === perPane.panes[pdfPane].mode
      && p.panes[pdfPane].opacity === null && p.windowDial === perPane.panes[pdfPane].opacity);

  // The stored preference survived the stand-down, so the next split brings
  // the mode back by itself: the survivor keeps what the window just learned
  // from it, and the pane born beside it is handed a mode of its own.
  if ((await openIn(SPLIT_NOTES, "right")) !== true) throw new Error("[texture boundary] the host refused the pane that carries the mode back");
  const back = await waitForSettledLayout("[texture boundary] the mode returns with the split", (s) =>
    s.host?.panes?.length === 2 &&
    s.host.panes.every((p) => p.lifecycle === "ready" && p.resources?.documentSession === true), 60_000);
  const newcomer = back.host.panes.find((p) => p.paneId !== pdfPane).paneId;
  const again = await waitTextures("the mode is back: survivor unchanged, newcomer its own",
    (p) => p.panes[pdfPane].mode === promoted.panes[pdfPane].mode
      && p.panes[newcomer].mode !== p.panes[pdfPane].mode);

  // Switch the mode off by hand: one texture for every pane again — the
  // window's, which is the promoted one — and no pane paints dials. Then put
  // the window back where the stage found it, so the stages that follow read
  // the workspace they expect.
  await frameClick('[data-setting="independent-textures"] [role="switch"]', "[texture boundary] independent textures off");
  await waitTextures("one texture for every pane, the window's own",
    (p) => p.panes[pdfPane].mode === p.panes[newcomer].mode
      && p.panes[pdfPane].mode === again.panes[pdfPane].mode
      && p.panes[pdfPane].opacity === null && p.panes[newcomer].opacity === null);
  await clickTexture(atRest.chosen);
  const restored = await waitTextures("the window's texture is as the stage found it",
    (p) => p.panes[pdfPane].mode === atRest.panes[pdfPane].mode
      && p.panes[newcomer].mode === atRest.panes[pdfPane].mode
      && p.chosen === atRest.chosen);
  await frameClick('button[title="Appearance"]', "[texture boundary] close the appearance menu");

  console.log(
    `[texture boundary]: the split's textures ${atRest.panes[pdfPane].mode} | ${atRest.panes[mdPane].mode} ` +
      `became ${again.panes[pdfPane].mode} | ${again.panes[newcomer].mode}, the Markdown pane moved to ${target} at 0.400, ` +
      `the collapse promoted ${promoted.panes[pdfPane].mode} to the window, and the mode off left both at ${restored.panes[pdfPane].mode}`,
  );
  summary.paneTextureBoundary = {
    rest: atRest.panes, perPane: again.panes, promoted: promoted.panes[pdfPane],
    promotedDial: promoted.windowDial, target, withColour: both.panes,
  };
  assertNoNewPanics("independent texture boundary", panicsBeforeTexture);
  const beforeTextureClose = await snap();
  await closeAndWaitBaseline("pane texture boundary", false, beforeTextureClose.disposalEpoch + beforeTextureClose.host.panes.length);
}

// --- Stage 14: the Library panel, the one split-drag source --------------
currentStage = "stage14-drag-drop";
// The production drag: a file row of the reader's Library panel (the rail's
// third tab, a compact tree of the library) carries its file to a split of
// the REAL workspace — the host measures its slot and its panes, the
// preview is geometry only (nothing opens until the drop), and the drop is
// the host's one workspace command, whose pane takes focus. Escape, a
// release back over the rail and a full workspace change nothing. The same
// panel lists the open panes once there is more than one (a click focuses,
// × closes), and a plain click on a row opens the file in the focused pane
// (the default of the tree-click setting). A light Markdown fixture
// throughout: this proves the drag, not a PDF's render timing.

/** A light document opened from the URL, in one ready pane. */
async function openLight(path) {
  await page.goto(`${BASE}/?open=${encodeURIComponent(path)}`, { waitUntil: "domcontentloaded" });
  return waitFor(`${path} to open in one pane`, (s) =>
    s.readerRuntimeLive === true && s.host?.lifecycle === "live" &&
    s.host.panes?.length === 1 && s.host.panes[0].lifecycle === "ready" &&
    s.host.panes[0].resources?.documentSession === true, 60_000);
}

/** An element of the active frame's document, boxed in PAGE coordinates. */
async function frameBox(inner) {
  return page.evaluate(([sel, inner]) => {
    const f = document.querySelector(sel);
    const el = f?.contentDocument?.querySelector(inner);
    if (!el) return null;
    const fr = f.getBoundingClientRect();
    const r = el.getBoundingClientRect();
    return { x: fr.left + r.left, y: fr.top + r.top, width: r.width, height: r.height };
  }, [activeFrame, inner]);
}

/** The drop preview the active frame draws, if any. */
async function dropPreview() {
  return page.evaluate((sel) => {
    const doc = document.querySelector(sel)?.contentDocument;
    const el = doc?.querySelector("[data-drop-preview]");
    if (!el) return null;
    return {
      word: el.dataset.dropPreview,
      pane: Number(el.dataset.dropPane),
      label: el.textContent,
      announced: doc.querySelector("[data-drop-announce]")?.textContent ?? "",
    };
  }, activeFrame);
}

async function waitPreview(label, predicate, timeoutMs = 10_000) {
  const deadline = Date.now() + timeoutMs;
  let last = null;
  while (Date.now() < deadline) {
    last = await dropPreview();
    if (predicate(last)) return last;
    await page.waitForTimeout(50);
  }
  throw new Error(`[${label}] the preview never matched: ${JSON.stringify(last)}`);
}

/** What the Library panel shows: its file rows and its open-pane tabs
 *  (`null` while the panel is not the rail's shown tab). */
async function libraryPanel() {
  return page.evaluate((sel) => {
    const doc = document.querySelector(sel)?.contentDocument;
    const panel = doc?.querySelector("[data-library-panel]");
    if (!panel || panel.closest(".invisible") || panel.getBoundingClientRect().width === 0) return null;
    return {
      files: [...panel.querySelectorAll("[data-lib-file]")].map((el) => ({
        path: el.dataset.libFile,
        name: (el.textContent ?? "").trim(),
      })),
      tabs: [...panel.querySelectorAll("[data-open-tab]")].map((el) => ({
        pane: Number(el.dataset.openTab),
        selected: el.querySelector('[role="tab"]')?.getAttribute("aria-selected") === "true",
      })),
    };
  }, activeFrame);
}

async function waitPanel(label, predicate, timeoutMs = 10_000) {
  const deadline = Date.now() + timeoutMs;
  let last = null;
  while (Date.now() < deadline) {
    last = await libraryPanel();
    if (last && predicate(last)) return last;
    await page.waitForTimeout(100);
  }
  throw new Error(`[${label}] the Library panel never matched: ${JSON.stringify(last)}`);
}

/** Open the rail on its Library tab. The title bar's toggle sits under the
 *  window drag region (a plain browser swallows a hit-tested click there),
 *  so both clicks are dispatched on the buttons: the same handlers. */
async function openLibraryPanel(label, needles) {
  await page.evaluate((sel) => {
    document.querySelector(sel)?.contentDocument?.querySelector('button[title="Toggle sidebar"]')?.click();
  }, activeFrame);
  const deadline = Date.now() + 10_000;
  while (!(await page.evaluate((sel) => {
    const btn = document.querySelector(sel)?.contentDocument?.querySelector('button[title="Library"]');
    if (!btn) return false;
    btn.click();
    return true;
  }, activeFrame))) {
    if (Date.now() > deadline) throw new Error(`[${label}] the rail shows no Library tab`);
    await page.waitForTimeout(100);
  }
  const panel = await waitPanel(`${label}: the tree lists the fixtures`, (p) =>
    needles.every((n) => p.files.some((f) => f.name.includes(n))), 15_000);
  // The rail slides in: let its rows stop moving before anything aims at them.
  let before = await rowBox(needles[0]);
  for (let i = 0; i < 40; i += 1) {
    await page.waitForTimeout(100);
    const now = await rowBox(needles[0]);
    if (now && before && Math.abs(now.x - before.x) < 0.5 && Math.abs(now.y - before.y) < 0.5) break;
    before = now;
  }
  return panel;
}

/** The Library panel row naming `needle`, boxed in PAGE coordinates. */
async function rowBox(needle) {
  return page.evaluate(([sel, needle]) => {
    const f = document.querySelector(sel);
    const el = [...(f?.contentDocument?.querySelectorAll("[data-lib-file]") ?? [])]
      .find((n) => n.dataset.libFile === needle || (n.textContent ?? "").includes(needle));
    if (!el) return null;
    el.scrollIntoView({ block: "nearest" });
    const fr = f.getBoundingClientRect();
    const r = el.getBoundingClientRect();
    return { x: fr.left + r.left, y: fr.top + r.top, width: r.width, height: r.height };
  }, [activeFrame, needle]);
}

/** Press the Library row naming `needle` with the real mouse and carry the
 *  pointer to `to` (page coordinates) in steps. The button stays down. */
async function liftRow(needle, to) {
  const box = await rowBox(needle);
  if (!box) throw new Error(`the Library panel has no row for ${needle}`);
  const from = { x: box.x + Math.min(48, box.width / 2), y: box.y + box.height / 2 };
  // The row must be what a press there hits.
  const hit = await page.evaluate(([sel, x, y]) => {
    const f = document.querySelector(sel);
    const fr = f.getBoundingClientRect();
    const el = f.contentDocument.elementFromPoint(x - fr.left, y - fr.top);
    return el?.closest("[data-lib-file]") ? null : (el?.outerHTML ?? "nothing").slice(0, 160);
  }, [activeFrame, from.x, from.y]);
  if (hit) throw new Error(`the ${needle} row is covered by ${hit}`);
  await page.mouse.move(from.x, from.y);
  await page.mouse.down();
  const steps = 14;
  for (let step = 1; step <= steps; step += 1) {
    await page.mouse.move(from.x + ((to.x - from.x) * step) / steps, from.y + ((to.y - from.y) * step) / steps);
  }
  return from;
}

/** A plain click (the real mouse, no movement) on the row naming `needle`
 *  (its name, or its path exactly). */
async function clickRow(needle) {
  const box = await rowBox(needle);
  if (!box) throw new Error(`the Library panel has no row for ${needle}`);
  await page.mouse.click(box.x + Math.min(48, box.width / 2), box.y + box.height / 2);
}

/** Click `selector` in the active frame once it exists. */
async function frameClick(selector, label, timeoutMs = 10_000) {
  const deadline = Date.now() + timeoutMs;
  while (!(await page.evaluate(([sel, inner]) => {
    const el = document.querySelector(sel)?.contentDocument?.querySelector(inner);
    if (!el) return false;
    el.click();
    return true;
  }, [activeFrame, selector]))) {
    if (Date.now() > deadline) throw new Error(`[${label}] ${selector} never appeared`);
    await page.waitForTimeout(100);
  }
}

/** Settings → Workspace → the row-click choice, then close the modal. */
async function chooseLibraryClick(choice) {
  const label = `settings: library click ${choice}`;
  const railWasOpen = (await libraryPanel()) !== null;
  await frameClick('button[title="Reader settings"]', label);
  await frameClick('button[aria-label="Workspace"]', label);
  await frameClick(`[data-setting="library-click"] [data-choice="${choice}"]`, label);
  const deadline = Date.now() + 5_000;
  while ((await page.evaluate(([sel, choice]) =>
    document.querySelector(sel)?.contentDocument
      ?.querySelector(`[data-setting="library-click"] [data-choice="${choice}"]`)
      ?.getAttribute("aria-checked"), [activeFrame, choice])) !== "true") {
    if (Date.now() > deadline) throw new Error(`[${label}] the choice never took`);
    await page.waitForTimeout(50);
  }
  // Close it, and wait for the dialog itself to be gone: a sheet still
  // fading out would take the next click meant for the rail.
  const dialogUp = () => page.evaluate((sel) =>
    !!document.querySelector(sel)?.contentDocument?.querySelector('[role="dialog"][aria-label="Reader settings"]'), activeFrame);
  await page.evaluate((sel) => document.querySelector(sel).contentWindow.focus(), activeFrame);
  await page.keyboard.press("Escape");
  for (const [attempt, last] of [["escape", false], ["close button", true]]) {
    const until = Date.now() + 4_000;
    while (await dialogUp()) {
      if (Date.now() > until) break;
      await page.waitForTimeout(50);
    }
    if (!(await dialogUp())) break;
    if (last) throw new Error(`[${label}] the settings modal never closed (tried ${attempt})`);
    await frameClick('[role="dialog"][aria-label="Reader settings"] button[title="Close"]', label, 2_000);
  }
  await page.waitForTimeout(150);
  // One Escape peels one layer: the press that dismissed the sheet is not
  // also the sidebar's, so the Library tab is still there to click.
  if (railWasOpen && (await libraryPanel()) === null) {
    throw new Error(`[${label}] closing the settings sheet folded the rail away too`);
  }
}

/** Nothing about the workspace changed, and no drag is left behind. */
function assertUnchanged(s, before, label) {
  if (s.host.panes.length !== before.host.panes.length) throw new Error(`[${label}] ${s.host.panes.length} panes, expected ${before.host.panes.length}`);
  if (s.host.panesCreated !== before.host.panesCreated) throw new Error(`[${label}] a pane was created`);
  if (JSON.stringify(s.host.layout) !== JSON.stringify(before.host.layout)) throw new Error(`[${label}] the layout changed: ${JSON.stringify(s.host.layout)}`);
  if (s.host.drag !== "idle") throw new Error(`[${label}] the drag session is ${s.host.drag}`);
}

const everyReady = (s, n) => s.host?.panes?.length === n &&
  s.host.panes.every((p) => p.lifecycle === "ready" && p.resources?.documentSession === true);

{
  const panicsBefore = panicCount;
  const one = await openLight(SPLIT_NOTES);
  const first = one.host.panes[0];
  if (one.host.drag !== "idle") throw new Error(`[drag] a fresh workspace reports drag ${one.host.drag}`);
  const opened = await openLibraryPanel("drag", ["Split Notes", "Programming Pearls"]);
  // One pane: no tabs strip.
  if (opened.tabs.length !== 0) throw new Error(`[drag] one pane shows ${opened.tabs.length} open tabs`);

  // Right: into the pane's right tenth, halfway down.
  const firstBox = await frameBox(`[data-pane-id="${first.paneId}"]`);
  await liftRow("Split Notes", { x: firstBox.x + firstBox.width * 0.92, y: firstBox.y + firstBox.height * 0.5 });
  const right = await waitPreview("drag: the right-edge preview", (p) => p?.word === "right" && p.pane === first.paneId);
  if (!right.announced.includes("split right")) throw new Error(`[drag] the live region says ${JSON.stringify(right.announced)}`);
  if (!right.label.includes("Split Notes")) throw new Error(`[drag] the preview names ${JSON.stringify(right.label)}`);
  const during = await snap();
  if (during.host.drag !== "overTarget") throw new Error(`[drag] mid-drag phase ${during.host.drag}`);
  if (during.host.panes.length !== 1 || during.host.panesCreated !== one.host.panesCreated) {
    throw new Error("[drag] something opened before the drop");
  }
  await page.mouse.up();
  const two = await waitForSettledLayout("drag: dropped on the right", (s) => everyReady(s, 2), 30_000);
  const second = two.host.panes.find((p) => p.paneId !== first.paneId);
  if (two.host.activePane !== second.paneId || !second.focused) throw new Error(`[drag] the dropped pane did not take focus (${two.host.activePane})`);
  if (two.host.layout?.split?.axis !== "horizontal" || layoutLeaves(two.host.layout).join() !== `${first.paneId},${second.paneId}`) {
    throw new Error(`[drag] a right drop laid out ${JSON.stringify(two.host.layout)}`);
  }
  // The row's file in a pane of its own, not a move. (Its document id is the
  // library row's; the URL open that seeded the first pane names the path.)
  if (second.format !== "markdown" || !second.documentId) throw new Error(`[drag] the new pane shows ${second.format} ${second.documentId}`);
  if (two.host.drag !== "idle" || (await dropPreview()) !== null) throw new Error("[drag] the drag outlived its drop");
  await assertPaneBox(two, "drag: first", two.host.panes.find((p) => p.paneId === first.paneId));
  await assertPaneBox(two, "drag: second", second);
  // Two panes: the tabs strip lists both, the focused one selected.
  const tabsTwo = await waitPanel("drag: the open tabs", (p) => p.tabs.length === 2);
  if (tabsTwo.tabs.map((t) => t.pane).join() !== `${first.paneId},${second.paneId}` ||
      tabsTwo.tabs.find((t) => t.selected)?.pane !== second.paneId) {
    throw new Error(`[drag] the open tabs are ${JSON.stringify(tabsTwo.tabs)}`);
  }

  // Bottom of the new pane: a nested split.
  const secondBox = await frameBox(`[data-pane-id="${second.paneId}"]`);
  await liftRow("Split Notes", { x: secondBox.x + secondBox.width * 0.5, y: secondBox.y + secondBox.height * 0.93 });
  await waitPreview("drag: the bottom-edge preview", (p) => p?.word === "bottom" && p.pane === second.paneId);
  await page.mouse.up();
  const three = await waitForSettledLayout("drag: dropped at the bottom", (s) => everyReady(s, 3), 30_000);
  const third = three.host.panes.find((p) => p.paneId !== first.paneId && p.paneId !== second.paneId);
  if (three.host.activePane !== third.paneId) throw new Error(`[drag] the second drop's pane did not take focus (${three.host.activePane})`);
  const nested = three.host.layout?.split?.second?.split;
  if (three.host.layout?.split?.axis !== "horizontal" || nested?.axis !== "vertical" ||
      layoutLeaves(three.host.layout).join() !== `${first.paneId},${second.paneId},${third.paneId}`) {
    throw new Error(`[drag] a bottom drop laid out ${JSON.stringify(three.host.layout)}`);
  }

  // Escape mid drag: the preview goes, the release does nothing. Aimed at
  // the first pane's bottom as it is now: beside the docked rail and two
  // splits it is too narrow for a side split, which is correctly not
  // offered.
  const firstNow = await frameBox(`[data-pane-id="${first.paneId}"]`);
  const firstBottom = { x: firstNow.x + firstNow.width * 0.5, y: firstNow.y + firstNow.height * 0.93 };
  await liftRow("Split Notes", firstBottom);
  await waitPreview("drag: a drag before Escape", (p) => p?.pane === first.paneId);
  await page.evaluate((sel) => document.querySelector(sel).contentWindow.focus(), activeFrame);
  await page.keyboard.press("Escape");
  await waitPreview("drag: Escape clears the drag", (p) => p === null);
  await page.mouse.up();
  await page.waitForTimeout(300);
  assertUnchanged(await snap(), three, "drag: escape");

  // Carried out and brought back over the rail: nothing opens.
  const home = await liftRow("Split Notes", firstBottom);
  await waitPreview("drag: a drag over the workspace", (p) => p !== null);
  await page.mouse.move(home.x, home.y, { steps: 10 });
  await waitPreview("drag: back over the rail, no target", (p) => p === null);
  await page.mouse.up();
  await page.waitForTimeout(300);
  assertUnchanged(await snap(), three, "drag: released over the rail");

  // The tabs: a click focuses its pane, × closes it.
  await page.evaluate(([sel, id]) => {
    document.querySelector(sel)?.contentDocument?.querySelector(`[data-open-tab="${id}"] [role="tab"]`)?.click();
  }, [activeFrame, first.paneId]);
  await waitFor("tabs: the first pane focused", (s) => s.host?.activePane === first.paneId, 10_000);
  await page.evaluate(([sel, id]) => {
    const btn = document.querySelector(sel)?.contentDocument?.querySelector(`[data-open-tab-close="${id}"]`);
    if (!btn) throw new Error(`tab ${id} has no close control`);
    btn.click();
  }, [activeFrame, third.paneId]);
  const closedTab = await waitForSettledLayout("tabs: × closed the third pane", (s) =>
    s.host?.panes?.length === 2 && !s.host.panes.some((p) => p.paneId === third.paneId), 20_000);
  if (closedTab.host.activePane !== first.paneId) throw new Error(`[tabs] closing a background tab moved focus to ${closedTab.host.activePane}`);
  await waitPanel("tabs: the strip follows the close", (p) => p.tabs.length === 2 &&
    p.tabs.every((t) => t.pane !== third.paneId));

  // A plain click on a row opens it in the focused pane (the default).
  const pearlsBox = await rowBox("Programming Pearls");
  await page.mouse.click(pearlsBox.x + Math.min(48, pearlsBox.width / 2), pearlsBox.y + pearlsBox.height / 2);
  const replaced = await waitForSettledLayout("click: the focused pane shows the PDF", (s) =>
    everyReady(s, 2) && s.host.panes.find((p) => p.paneId === first.paneId)?.format === "pdf" &&
    s.engine?.sessionsLive === 1, 45_000);
  if (replaced.host.panesCreated !== closedTab.host.panesCreated) throw new Error("[click] a click created a pane instead of replacing");
  if (replaced.host.panes.find((p) => p.paneId === second.paneId)?.format !== "markdown") {
    throw new Error("[click] the other pane changed");
  }

  // The row-click setting, changed where a user changes it (Settings →
  // Workspace). "Open as a new split": a file that is not open gets a new
  // pane beside the focused one — BELOW it here, since beside the docked
  // rail the column has no room for another side split. "Do nothing": a
  // click is ignored, even on a file that is open in another pane.
  const others = (await libraryPanel()).files.filter((f) =>
    !/Split Notes|Programming Pearls/.test(`${f.name} ${f.path}`));
  if (others.length === 0) throw new Error("[click] the library holds no third file to open");
  await chooseLibraryClick("split");
  const beforeSplitClick = await snap();
  await clickRow(others[0].path);
  // On a failure, say what the click left behind: whether a pane was made
  // at all, and in what state.
  const splitClick = await waitForSettledLayout("click: a new split beside the focused pane", (s) => everyReady(s, 3), 45_000)
    .catch(async (error) => {
      const now = await snap();
      throw new Error(`${error.message}\n  clicked ${JSON.stringify(others[0])} of ${JSON.stringify(others.map((f) => f.path))}` +
        `\n  panes created ${beforeSplitClick.host.panesCreated} -> ${now?.host?.panesCreated}, active ${now?.host?.activePane}, drag ${now?.host?.drag}` +
        `\n  panes ${JSON.stringify(now?.host?.panes?.map((p) => ({ id: p.paneId, format: p.format, lifecycle: p.lifecycle, bounds: p.bounds })))}` +
        `\n  docStatus ${now?.docStatus} ${now?.docError ?? ""}` +
        `\n  errors ${JSON.stringify(errorLog.slice(-6))}` +
        `\n  stage log ${JSON.stringify(huntLog.filter((l) => l.startsWith(`[${currentStage}]`) && !/integrity|willReadFrequently/.test(l)).slice(-20))}`);
    });
  const clicked = splitClick.host.panes.find((p) => p.paneId !== first.paneId && p.paneId !== second.paneId);
  const firstAfter = splitClick.host.panes.find((p) => p.paneId === first.paneId);
  if (splitClick.host.activePane !== clicked.paneId) throw new Error(`[click] the split's pane did not take focus (${splitClick.host.activePane})`);
  // Beside the focused pane: to its right when a side split fits, else below
  // it (the column beside the docked rail may be too narrow).
  const rightOf = Math.abs(clicked.bounds.x - (firstAfter.bounds.x + firstAfter.bounds.width)) <= 2 && Math.abs(clicked.bounds.y - firstAfter.bounds.y) <= 2;
  const below = Math.abs(clicked.bounds.y - (firstAfter.bounds.y + firstAfter.bounds.height)) <= 2 && Math.abs(clicked.bounds.x - firstAfter.bounds.x) <= 2;
  if (!rightOf && !below) {
    throw new Error(`[click] the split is not beside the focused pane: ${JSON.stringify([firstAfter.bounds, clicked.bounds])}`);
  }
  await chooseLibraryClick("dragonly");
  await clickRow("Split Notes");
  await page.waitForTimeout(600);
  const ignored = await snap();
  if (ignored.host.activePane !== clicked.paneId || ignored.host.panesCreated !== splitClick.host.panesCreated) {
    throw new Error(`[click] "Do nothing" acted on a click (active ${ignored.host.activePane})`);
  }
  await chooseLibraryClick("replace");

  // A full workspace offers nothing: with a fourth pane (MAX_PANES) a drag
  // finds no target over any pane, and its release changes nothing. Beside
  // the docked rail the columns are too narrow for another side split, so
  // the second column — focused from its tab — is split down.
  await page.evaluate(([sel, id]) => {
    document.querySelector(sel)?.contentDocument?.querySelector(`[data-open-tab="${id}"] [role="tab"]`)?.click();
  }, [activeFrame, second.paneId]);
  await waitFor("tabs: the second pane focused", (s) => s.host?.activePane === second.paneId, 10_000);
  if ((await openIn(SPLIT_NOTES, "down")) !== true) throw new Error("[drag] the host refused a pane below the second");
  const four = await waitForSettledLayout("drag: a full workspace", (s) => everyReady(s, 4), 30_000);
  const leftBox = await frameBox(`[data-pane-id="${first.paneId}"]`);
  await liftRow("Split Notes", { x: leftBox.x + leftBox.width * 0.92, y: leftBox.y + leftBox.height * 0.5 });
  await page.waitForTimeout(300);
  const full = await snap();
  if ((await dropPreview()) !== null || full.host.drag !== "overWorkspace") {
    throw new Error(`[drag] a full workspace offered a target (drag ${full.host.drag})`);
  }
  await page.mouse.up();
  await page.waitForTimeout(300);
  assertUnchanged(await snap(), four, "drag: full workspace release");
  assertNoNewPanics("drag and drop", panicsBefore);

  summary.dragDrop = {
    panes: three.host.panes.map((p) => ({ paneId: p.paneId, bounds: p.bounds })),
    layout: three.host.layout,
    focusedAfterDrops: [two.host.activePane, three.host.activePane],
    tabsAfterClose: closedTab.host.panes.map((p) => p.paneId),
    clickReplaced: replaced.host.panes.find((p) => p.paneId === first.paneId)?.format,
  };
  // The reader disposed mid-drag: the button is still down, the session
  // live, when the workspace goes — and the disposed host reports no drag.
  await liftRow("Split Notes", { x: leftBox.x + leftBox.width * 0.5, y: leftBox.y + leftBox.height * 0.5 });
  await page.waitForTimeout(200);
  const held = await snap();
  if (held.host.drag === "idle") throw new Error("[drag] the drag before the dispose never went live");
  // One mint per open and per pane dispose: what the live workspace has
  // claimed so far, plus one for each pane the close disposes.
  const disposed = await closeAndWaitBaseline("drag and drop", false, held.disposalEpoch + held.host.panes.length);
  await page.mouse.up();
  if (disposed.host.drag !== "idle") throw new Error(`[drag] the disposed host still reports drag ${disposed.host.drag}`);
  console.log(`drag and drop: Library rows dropped right and nested bottom; Escape, a release over the rail and a full workspace left the tree alone; tabs focus and close; a click replaced the focused pane; a dispose mid-drag (${held.host.drag}) left drag ${disposed.host.drag}`);
}

// --- Stage 15: split workspace memory and lifecycle -----------------------
currentStage = "stage15-split-memory";
// A PDF pane stays open while a Markdown pane is split beside it and closed
// three times: the PDF's session is the same one throughout, and each close
// takes the workspace back to its PDF-only baseline. Then PDF + Markdown +
// TXT, and the reader's dispose releases every pane owner.
const PLAIN_NOTES = "/samples/Plain Notes.txt";

async function openIn(path, target) {
  return page.evaluate(([sel, path, target]) => {
    const hook = document.querySelector(sel)?.contentWindow?.__mareaderOpenIn;
    if (typeof hook !== "function") throw new Error("the web build's __mareaderOpenIn hook is missing");
    return hook(path, target);
  }, [activeFrame, path, target]);
}

{
  const panicsBefore = panicCount;
  const opened = await openBook(pearlsUrl);
  const pdfPane = opened.host.panes[0].paneId;
  const pdfDocument = opened.host.panes[0].documentId;
  const alone = await waitForSettledLayout("memory: the PDF alone", (s) => s.host?.panes?.length === 1, 15_000);
  const baseline = {
    paneLive: alone.paneLive, virtualizerLive: alone.virtualizerLive, sessionsOpened: alone.engine.sessionsOpened,
    virtualizersCreated: alone.virtualizersCreated, virtualizersDisposed: alone.virtualizersDisposed,
  };
  const heapAfterClose = [];
  for (let cycle = 1; cycle <= 3; cycle += 1) {
    if ((await openIn(SPLIT_NOTES, "right")) !== true) throw new Error(`[memory ${cycle}] the host refused the Markdown pane`);
    const both = await waitForSettledLayout(`memory ${cycle}: PDF | Markdown`, (s) =>
      s.host?.panes?.length === 2 &&
      s.host.panes.every((p) => p.lifecycle === "ready" && p.resources?.documentSession === true), 30_000);
    const md = both.host.panes.find((p) => p.paneId !== pdfPane);
    await page.evaluate(([sel, id]) => {
      const btn = document.querySelector(sel)?.contentDocument?.querySelector(`[data-pane-close="${id}"] button`);
      if (!btn) throw new Error(`pane ${id} has no close control`);
      btn.click();
    }, [activeFrame, md.paneId]);
    let closed = null;
    for (const deadline = Date.now() + 20_000; Date.now() < deadline; await page.waitForTimeout(100)) {
      closed = await snap();
      if (closed?.host?.panes?.length === 1 && closed.host.panes[0].paneId === pdfPane) break;
    }
    if (!(closed?.host?.panes?.length === 1 && closed.host.panes[0].paneId === pdfPane)) {
      throw new Error(`[memory ${cycle}] closing Markdown pane ${md.paneId} (PDF ${pdfPane}) left: ` + JSON.stringify({
        bootState: closed?.bootState, live: closed?.readerRuntimeLive, host: closed?.host?.lifecycle,
        active: closed?.host?.activePane, layout: closed?.host?.layout,
        panes: closed?.host?.panes?.map((p) => ({ id: p.paneId, format: p.format, lifecycle: p.lifecycle })),
        panics: panicCount - panicsBefore, errors: errorLog.slice(-8),
      }));
    }
    await waitForSettledLayout(`memory ${cycle}: the Markdown pane closed`, (s) =>
      s.host?.panes?.length === 1 && s.host.panes[0].paneId === pdfPane, 30_000);
    let back = null;
    for (const deadline = Date.now() + 15_000; Date.now() < deadline; await page.waitForTimeout(100)) {
      back = await snap();
      if (back.paneLive === baseline.paneLive && back.virtualizerLive === baseline.virtualizerLive) break;
    }
    if (back.paneLive !== baseline.paneLive || back.virtualizerLive !== baseline.virtualizerLive) {
      throw new Error(`[memory ${cycle}] not back at the PDF-only baseline ${JSON.stringify(baseline)}: ` +
        JSON.stringify({ paneLive: back.paneLive, virtualizerLive: back.virtualizerLive,
          panesCreated: back.panesCreated, panesDisposed: back.panesDisposed,
          virtualizersCreated: back.virtualizersCreated, virtualizersDisposed: back.virtualizersDisposed,
          host: back.host.panes.map((p) => ({ id: p.paneId, format: p.format, lifecycle: p.lifecycle })) }));
    }
    const pdf = back.host.panes[0];
    if (pdf.documentId !== pdfDocument || pdf.resources.documentSession !== true || back.engine.sessionsLive !== 1) {
      throw new Error(`[memory ${cycle}] the PDF pane lost its session: ${JSON.stringify(pdf)}`);
    }
    if (back.engine.sessionsOpened !== baseline.sessionsOpened) throw new Error(`[memory ${cycle}] the PDF was reopened`);
    heapAfterClose.push(back.wasmHeapBytes);
  }
  if ((await openIn(SPLIT_NOTES, "right")) !== true) throw new Error("[memory] the host refused the Markdown pane");
  await waitFor("memory: PDF | Markdown", (s) => s.host?.panes?.length === 2 &&
    s.host.panes.every((p) => p.lifecycle === "ready"), 30_000);
  if ((await openIn(PLAIN_NOTES, "right")) !== true) throw new Error("[memory] the host refused the TXT pane");
  const all = await waitForSettledLayout("memory: PDF | Markdown | TXT", (s) =>
    s.host?.panes?.length === 3 &&
    s.host.panes.every((p) => p.lifecycle === "ready" && p.resources?.documentSession === true), 30_000);
  const formats = all.host.panes.map((p) => p.format).sort().join();
  if (formats !== "markdown,pdf,text") throw new Error(`[memory] the three panes are ${formats}`);
  assertNoNewPanics("split memory", panicsBefore);
  summary.splitMemory = {
    baseline,
    heapAfterMarkdownClose: heapAfterClose,
    threeFormats: all.host.panes.map((p) => ({ paneId: p.paneId, format: p.format })),
  };
  stages.afterSplitMemory = all;
  // Twelve mints: the PDF, three Markdown opens and their three pane
  // disposes, the Markdown and TXT opens, and the three panes' disposes.
  await closeAndWaitBaseline("split memory", false, 12);
  console.log(`split memory: the PDF kept its session across 3 Markdown split/close cycles (heap after each close ${heapAfterClose.join(", ")}); PDF + Markdown + TXT disposed clean`);
}

// --- Zoom with animation off, and the noise layer's runtime state ---------
// Both stages reboot the app per settings state: settings are read at boot,
// so a reload is the one way to put a runtime in a known motion state.
const SETTINGS_KEY = "mareader.settings.v1";
const plainPearlsUrl = `${BASE}/?open=${encodeURIComponent(PEARLS)}`;

async function writeSettings(patch) {
  await page.evaluate(([key, patch]) => {
    let s = {};
    try { s = JSON.parse(localStorage.getItem(key) ?? "{}") ?? {}; } catch { s = {}; }
    for (const [section, fields] of Object.entries(patch)) {
      s[section] = { ...(s[section] ?? {}), ...fields };
    }
    localStorage.setItem(key, JSON.stringify(s));
  }, [SETTINGS_KEY, patch]);
}

/** Install a per-rAF sampler in the active reader frame. Every frame it
 *  records the scroll offset, the vertical strip's extent, and the page host
 *  under the viewport centre: its CSS width, its `--scale-factor`, its
 *  canvas' pixel grid, and whether that canvas is BLANK (zero-sized, or one
 *  flat colour when downsampled — a cleared canvas, never a rendered page). */
async function startZoomSampler() {
  await page.evaluate(() => {
    const f = document.querySelector('#runtime-host .runtime-frame[data-mareader-slot="active"]');
    const w = f.contentWindow;
    const d = f.contentDocument;
    const ext = d.querySelector('[data-strip-extent="vertical"]');
    if (!ext) throw new Error("vertical strip extent not found");
    const sc = ext.parentElement.parentElement;
    const probe = d.createElement("canvas");
    probe.width = 24;
    probe.height = 24;
    const pctx = probe.getContext("2d", { willReadFrequently: true });
    const blank = (c) => {
      if (!c || !(c.width > 0) || !(c.height > 0)) return true;
      pctx.clearRect(0, 0, 24, 24);
      pctx.drawImage(c, 0, 0, 24, 24);
      const px = pctx.getImageData(0, 0, 24, 24).data;
      for (let i = 4; i < px.length; i += 4) {
        if (px[i] !== px[0] || px[i + 1] !== px[1] || px[i + 2] !== px[2] || px[i + 3] !== px[3]) return false;
      }
      return true;
    };
    const samples = [];
    w.__zoomSamples = samples;
    w.__zoomSampling = true;
    // Ground truth for ordering: every style write under the strip, every
    // scroll event and the keydown, stamped with the frame they fell in.
    let frameNo = 0;
    const events = [];
    w.__zoomEvents = events;
    const kind = (el) => el.dataset?.stripExtent ? "ext"
      : el.classList?.contains("pdf-page") ? `host${el.id.replace(/\D/g, "")}`
      : el.firstElementChild?.classList?.contains("pdf-page") ? `wrap${el.firstElementChild.id.replace(/\D/g, "")}`
      : null;
    const mo = new w.MutationObserver((records) => {
      events.push("D");
      for (const rec of records) {
        const k = kind(rec.target);
        if (k) events.push(`${frameNo}:${Math.round(w.performance.now())}:${k}`);
      }
    });
    mo.observe(sc, { subtree: true, attributes: true, attributeFilter: ["style"] });
    w.__zoomMo = mo;
    sc.addEventListener("scroll", () => events.push(`${frameNo}:${Math.round(w.performance.now())}:scroll${Math.round(sc.scrollTop)}`));
    d.addEventListener("keydown", () => events.push(`${frameNo}:${Math.round(w.performance.now())}:key`), true);
    // Recorded AFTER each frame, not in its rAF callback: rAF runs before
    // the frame's layout and ResizeObserver delivery, both of which can still
    // move things before the paint. A macrotask posted from rAF runs once the
    // frame is out, so each sample is the state that was actually painted.
    const chan = new w.MessageChannel();
    const record = () => {
      if (!w.__zoomSampling) return;
      events.push(`${frameNo}:${Math.round(w.performance.now())}:S${samples.length}`);
      const r = sc.getBoundingClientRect();
      const cy = r.top + r.height / 2;
      // The page under the centre line; when the line falls in the gap
      // between two pages, the nearer of them.
      const gap = (h) => {
        const b = h.getBoundingClientRect();
        return b.top > cy ? b.top - cy : b.bottom < cy ? cy - b.bottom : 0;
      };
      const host = [...d.querySelectorAll(".pdf-page")]
        .reduce((best, h) => (best && gap(best) <= gap(h) ? best : h), null);
      const canvas = host?.querySelector("canvas:not(.page-snapshot)") ?? null;
      const scale = parseFloat(/--scale-factor:\s*([0-9.]+)/.exec(host?.getAttribute("style") ?? "")?.[1] ?? "NaN");
      samples.push({
        top: sc.scrollTop,
        vh: sc.clientHeight,
        ext: parseFloat(ext.style.height),
        host: host?.id ?? null,
        hostW: host ? host.getBoundingClientRect().width : 0,
        scale,
        canvasW: canvas?.width ?? 0,
        blank: blank(canvas),
        dpr: w.devicePixelRatio,
        pages: [...d.querySelectorAll(".pdf-page")].map((h) => {
          const b = h.getBoundingClientRect();
          return `${h.id.replace(/\D/g, "")}:${Math.round(b.top - r.top + sc.scrollTop)}+${Math.round(b.height)}`;
        }).join(","),
      });
    };
    chan.port1.onmessage = record;
    const tick = () => {
      if (!w.__zoomSampling) return;
      frameNo += 1;
      chan.port2.postMessage(0);
      w.requestAnimationFrame(tick);
    };
    w.requestAnimationFrame(tick);
  });
}

async function stopZoomSampler() {
  return page.evaluate(() => {
    const f = document.querySelector('#runtime-host .runtime-frame[data-mareader-slot="active"]');
    const w = f.contentWindow;
    w.__zoomSampling = false;
    w.__zoomMo?.disconnect();
    return w.__zoomSamples ?? [];
  });
}

const near = (a, b, tol) => Math.abs(a - b) <= tol;

/** Read the page at the centre once the zoom's renders have landed: the
 *  engine reports no render in flight, and the centre host carries a painted
 *  canvas whose `--scale-factor` holds still across two reads. */
async function waitCrisp(label, timeoutMs = 20_000) {
  const started = Date.now();
  let prev = null;
  for (;;) {
    const s = await snap();
    if (s?.engine?.activeRenders === 0) {
      await startZoomSampler();
      await page.waitForTimeout(100);
      const xs = await stopZoomSampler();
      const last = xs[xs.length - 1];
      if (last && !last.blank && prev && prev.scale === last.scale && prev.canvasW === last.canvasW) {
        return last;
      }
      prev = last ?? null;
    }
    if (Date.now() - started > timeoutMs) {
      throw new Error(`[${label}] the zoom's renders never settled: ${JSON.stringify(prev)}`);
    }
    await page.waitForTimeout(150);
  }
}

currentStage = "zoom-animation-off";
{
  await writeSettings({ animations: { enabled: false } });
  await openBook(plainPearlsUrl);
  const motionOff = await page.evaluate(() => {
    const f = document.querySelector('#runtime-host .runtime-frame[data-mareader-slot="active"]');
    return f.contentDocument.documentElement.classList.contains("animations-off");
  });
  if (!motionOff) throw new Error("zoom-off: the reader frame is not in the animations-off state");
  // Move off the first page so the anchor has room on both sides.
  await page.mouse.click(700, 450);
  for (let i = 0; i < 3; i++) {
    await page.keyboard.press("PageDown");
    await page.waitForTimeout(120);
  }
  const before = await waitCrisp("zoom-off baseline");

  // Tests 1-5: one "+", sampled every frame from before the press until the
  // commit's render has landed.
  await startZoomSampler();
  await waitFrames(3);
  await page.keyboard.press("+");
  await page.waitForTimeout(900);
  const xs = await stopZoomSampler();
  const zoomEvents = await page.evaluate(() => {
    const f = document.querySelector('#runtime-host .runtime-frame[data-mareader-slot="active"]');
    return (f.contentWindow.__zoomEvents ?? []).slice(0, 120);
  });
  const after = await waitCrisp("zoom-off commit");
  const extTol = (e) => Math.max(4, e * 0.004);
  const pre = xs.filter((x) => near(x.ext, before.ext, extTol(before.ext)));
  const post = xs.filter((x) => near(x.ext, after.ext, extTol(after.ext)));
  const mid = xs.filter((x) => !pre.includes(x) && !post.includes(x));
  summary.zoomOff = {
    frames: xs.length,
    preFrames: pre.length,
    postFrames: post.length,
    midFrames: mid.length,
    blankFrames: xs.filter((x) => x.blank).length,
    before: { scale: before.scale, ext: before.ext, top: before.top },
    after: { scale: after.scale, ext: after.ext, top: after.top },
  };
  console.log(`zoom-off: ${JSON.stringify(summary.zoomOff)}`);
  if (!(after.scale > before.scale)) {
    throw new Error(`zoom-off: "+" did not zoom in (scale ${before.scale} -> ${after.scale})`);
  }
  // 1/2: the display lands on the target in one step — no frame between.
  if (mid.length > 0) {
    throw new Error(`zoom-off: ${mid.length} frame(s) showed an intermediate layout: ${JSON.stringify(mid.slice(0, 3))}`);
  }
  if (pre.length === 0 || post.length === 0) {
    throw new Error(`zoom-off: sampler missed a side of the change (pre ${pre.length}, post ${post.length})`);
  }
  const firstPost = xs.indexOf(post[0]);
  if (xs.slice(firstPost).some((x) => pre.includes(x))) {
    throw new Error("zoom-off: the layout went back to the old scale after landing");
  }
  // 3: no blank canvas at any frame while the crisp render was pending.
  const blanks = xs.filter((x) => x.blank);
  if (blanks.length > 0) {
    throw new Error(`zoom-off: ${blanks.length} frame(s) showed a blank canvas: ${JSON.stringify(blanks[0])}`);
  }
  // 4/5: the scroll offset is final ON the landing frame (no clamped write
  // corrected a frame later), and nothing moves it afterwards.
  const landing = xs[firstPost];
  const settled = post[post.length - 1];
  if (!near(landing.top, settled.top, 2)) {
    throw new Error(`zoom-off: the landing frame's scroll ${landing.top} was corrected later to ${settled.top}`);
  }
  // The document point under the viewport centre stays put (gaps do not
  // scale, hence the tolerance).
  const f = after.scale / before.scale;
  const expected = (before.top + before.vh / 2) * f - before.vh / 2;
  if (!near(settled.top, expected, before.vh * 0.03 + 24)) {
    throw new Error(`zoom-off: scroll ${settled.top} does not hold the centre anchor (expected ~${expected.toFixed(1)})`);
  }
  // The landing frame already shows the settled page, at its settled size:
  // the same host under the viewport centre, as wide as it ends up. (Pages
  // in a scanned book differ in size, so the comparison is per host.)
  if (landing.host !== settled.host || !near(landing.hostW, settled.hostW, 2)) {
    const around = xs.slice(Math.max(0, firstPost - 2), firstPost + 4)
      .map((x) => `${x.host} w=${x.hostW} s=${x.scale} ext=${x.ext} top=${x.top} cw=${x.canvasW} pages=[${x.pages}]`);
    console.log(`zoom-off events: ${zoomEvents.join(" ")}`);
    throw new Error(`zoom-off: landing frame ${landing.host} w=${landing.hostW} != settled ${settled.host} w=${settled.hostW}; frames: ${around.join(" | ")}`);
  }

  // Test 9: a rapid "+ + - +" burst ends at a final, settled state, with no
  // blank frame on the way. Every step commits synchronously, so the burst
  // equals the steps one by one: two more "-" land back on `after`.
  await startZoomSampler();
  for (const k of ["+", "+", "-", "+"]) await page.keyboard.press(k);
  await page.waitForTimeout(900);
  const burst = await stopZoomSampler();
  const burstEnd = await waitCrisp("zoom-off burst");
  const burstBlank = burst.filter((x) => x.blank);
  if (burstBlank.length > 0) {
    throw new Error(`zoom-off burst: ${burstBlank.length} blank frame(s): ${JSON.stringify(burstBlank[0])}`);
  }
  if (!(burstEnd.scale > after.scale)) {
    throw new Error(`zoom-off burst: net two steps in, but scale ${after.scale} -> ${burstEnd.scale}`);
  }
  await page.keyboard.press("-");
  await page.waitForTimeout(200);
  await page.keyboard.press("-");
  const back = await waitCrisp("zoom-off return");
  if (!near(back.scale, after.scale, 1e-6)) {
    throw new Error(`zoom-off burst: two steps back landed at ${back.scale}, expected ${after.scale}`);
  }
  summary.zoomOffBurst = { frames: burst.length, endScale: burstEnd.scale, backScale: back.scale };

  // Test 8: animation ON is unchanged — the same "+" still interpolates
  // through intermediate layouts and lands on a crisp render.
  await writeSettings({ animations: { enabled: true } });
  await openBook(plainPearlsUrl);
  await page.mouse.click(700, 450);
  for (let i = 0; i < 3; i++) {
    await page.keyboard.press("PageDown");
    await page.waitForTimeout(120);
  }
  const onBefore = await waitCrisp("zoom-on baseline");
  await startZoomSampler();
  await waitFrames(2);
  await page.keyboard.press("+");
  await page.waitForTimeout(900);
  const onXs = await stopZoomSampler();
  const onAfter = await waitCrisp("zoom-on commit");
  const onMid = onXs.filter((x) =>
    !near(x.ext, onBefore.ext, extTol(onBefore.ext)) && !near(x.ext, onAfter.ext, extTol(onAfter.ext)));
  summary.zoomOn = { frames: onXs.length, midFrames: onMid.length, blankFrames: onXs.filter((x) => x.blank).length };
  console.log(`zoom-on: ${JSON.stringify(summary.zoomOn)}`);
  if (onMid.length === 0) throw new Error("zoom-on: the tween no longer interpolates");
  if (summary.zoomOn.blankFrames > 0) throw new Error("zoom-on: a blank canvas frame during the tween");
  await clickCloseNow();
  await waitFor("the disposal baseline (zoom stage)", (x) => x.runtime?.state === "disposed", 45_000);
}
console.log("zoom with animation off is one discrete commit: no intermediate frame, no blank canvas, scroll final on the landing frame");

currentStage = "noise-runtime-state";
{
  // The animated grain, proven in the running frames rather than read off
  // the stylesheet: overlay count, the classes that drive it, the ::after's
  // computed animation, and its transform sampled over time. A computed
  // animation whose transform moves here, while the app shows still grain,
  // is a compositor problem; one that does not run is a cascade problem.
  const probeNoise = () => page.evaluate(async () => {
    const read = (slot) => {
      const f = document.querySelector(`#runtime-host .runtime-frame[data-mareader-slot="${slot}"]`);
      const d = f?.contentDocument;
      if (!d?.body) return null;
      const overlays = d.querySelectorAll(".noise-overlay");
      const o = overlays[0] ?? null;
      const cs = o ? f.contentWindow.getComputedStyle(o, "::after") : null;
      return {
        overlays: overlays.length,
        html: d.documentElement.className,
        body: d.body.className,
        name: cs?.animationName ?? null,
        duration: cs?.animationDuration ?? null,
        iterations: cs?.animationIterationCount ?? null,
        playState: cs?.animationPlayState ?? null,
        transform: cs?.transform ?? null,
      };
    };
    const active = read("active");
    const transforms = new Set();
    for (let i = 0; i < 12; i++) {
      const r = read("active");
      if (r?.transform) transforms.add(r.transform);
      await new Promise((res) => setTimeout(res, 70));
    }
    return { active, otherRoutes: document.querySelectorAll('#runtime-host .runtime-frame:not([data-mareader-slot="active"])').length,
      libraryFrames: document.querySelectorAll('iframe[data-mareader-runtime-frame="library"]').length,
      distinctTransforms: transforms.size };
  });
  // The loading mark, the same way: the real markup mounted in the live
  // frame, its first dot's computed animation and transform sampled. It
  // chases (travels) unless the OS asks for reduced motion — the app's own
  // animations switch must not freeze it into three still dots.
  const probeLoader = () => page.evaluate(async () => {
    const f = document.querySelector('#runtime-host .runtime-frame[data-mareader-slot="active"]');
    const d = f.contentDocument;
    const w = f.contentWindow;
    const box = d.createElement("div");
    box.className = "loader";
    box.style.cssText = "width:72px;position:fixed;left:0;top:0";
    for (const k of ["a", "b", "c"]) {
      const dot = d.createElement("span");
      dot.className = `loader-dot loader-dot-${k}`;
      box.appendChild(dot);
    }
    d.body.appendChild(box);
    const dot = box.firstElementChild;
    const transforms = new Set();
    const opacities = new Set();
    let cs = w.getComputedStyle(dot);
    for (let i = 0; i < 14; i++) {
      cs = w.getComputedStyle(dot);
      transforms.add(cs.transform);
      opacities.add(cs.opacity);
      await new Promise((res) => setTimeout(res, 70));
    }
    const out = {
      name: cs.animationName,
      duration: cs.animationDuration,
      iterations: cs.animationIterationCount,
      transforms: transforms.size,
      opacities: opacities.size,
    };
    box.remove();
    return out;
  });
  summary.loader = {};
  summary.noise = {};
  for (const animations of [true, false]) {
    for (const reduced of [false, true]) {
      await page.emulateMedia({ reducedMotion: reduced ? "reduce" : "no-preference" });
      await writeSettings({
        animations: { enabled: animations },
        appearance: { noise: "animated", noise_intensity: 60 },
      });
      await openBook(plainPearlsUrl);
      await page.waitForTimeout(400);
      const r = await probeNoise();
      const key = `animations_${animations ? "on" : "off"}__reduced_${reduced ? "on" : "off"}`;
      summary.noise[key] = r;
      console.log(`noise ${key}: ${JSON.stringify(r)}`);
      if (r.active?.overlays !== 1) throw new Error(`noise ${key}: ${r.active?.overlays} overlays in the active frame`);
      if (!/\bnoise-animated\b/.test(r.active.body)) throw new Error(`noise ${key}: body lacks noise-animated (${r.active.body})`);
      if (r.otherRoutes !== 0) throw new Error(`noise ${key}: ${r.otherRoutes} Library/extra route frames survived while reading`);
      // The grain is content, not UI motion: it crawls unless the OS asks for
      // reduced motion, whatever the app's animation switch says.
      const shouldRun = !reduced;
      const runs = r.active.name !== "none" && r.distinctTransforms > 1;
      if (runs !== shouldRun) {
        const cause = r.active.name === "none" || r.active.iterations === "1" ? "cascade" : "animation computed but transform static";
        throw new Error(`noise ${key}: grain ${runs ? "runs" : "is still"} but should ${shouldRun ? "run" : "be still"} (${cause})`);
      }
      const l = await probeLoader();
      summary.loader[key] = l;
      console.log(`loader ${key}: ${JSON.stringify(l)}`);
      if (reduced) {
        if (l.name !== "loader-fade" || l.opacities < 2) {
          throw new Error(`loader ${key}: reduced motion should breathe in place, got ${JSON.stringify(l)}`);
        }
      } else if (l.name !== "loader-hop-a" || l.iterations !== "infinite" || l.transforms < 2) {
        throw new Error(`loader ${key}: the mark should chase, got ${JSON.stringify(l)}`);
      }
    }
  }
  await page.emulateMedia({ reducedMotion: "no-preference" });
  await writeSettings({ appearance: { noise: "off" }, animations: { enabled: true } });
}
console.log("animated noise runs in the live frame in exactly the states it should");
console.log("the loading mark chases unless the OS asks for reduced motion");

currentStage = "boot-paint";
{
  // The next launch's first frame wears the paper this one painted: seed a
  // remembered paper, reload, and the placeholder must show it before any
  // wasm runs; once the shell has painted, the real paper replaces the seed.
  const BOOT_PAINT_KEY = "mareader.boot-paint.v1";
  const seed = "rgb(12, 34, 56)";
  await page.evaluate(([k, v]) => localStorage.setItem(k, v), [BOOT_PAINT_KEY, `${seed}|dark`]);
  await page.reload();
  await page.waitForFunction(() => window.__shellBoot?.removedAt !== null, null, { timeout: 45_000 });
  const boot = await page.evaluate(() => window.__shellBoot);
  if (boot.background !== seed) {
    throw new Error(`boot-paint: the placeholder wore ${boot.background}, not the remembered ${seed}`);
  }
  await page.waitForFunction(([k, v]) => {
    const now = localStorage.getItem(k);
    return now !== null && !now.startsWith(v);
  }, [BOOT_PAINT_KEY, seed], { timeout: 15_000 });
  summary.bootPaint = {
    seeded: boot.background,
    stored: await page.evaluate((k) => localStorage.getItem(k), BOOT_PAINT_KEY),
  };
  console.log(`boot-paint: ${JSON.stringify(summary.bootPaint)}`);
}
console.log("the boot placeholder runs the app's mark on the remembered paper");

currentStage = "pane-runtime-regressions";
summary.paneRuntimes = await verifyPaneRuntimes({
  page, openBook, openIn, waitFor, waitForSettledLayout, frameClick, writeSettings,
  closeAndWaitBaseline, snap,
  urls: { pdf: pearlsUrl },
  paths: { pdf: PEARLS, otherPdf: DEEP_OUTLINE, markdown: SPLIT_NOTES, text: PLAIN_NOTES },
});

currentStage = "cleanup-runtime-regressions";
summary.cleanupRuntime = await verifyCleanupRuntime({ page, browser, base: BASE,
  paths: { pdf: PEARLS, text: PLAIN_NOTES }, openBook, waitFor, snap });
currentStage = "window-state-regressions";
summary.windowState = await verifyWindowState({ browser, base: BASE });

// --- Guards ----------------------------------------------------------------
summary.consoleErrorsUnrelated = otherErrorCount;
if (otherErrorCount > 0) {
  console.log(`${otherErrorCount} console error(s) outside the disposal-panic class (not asserted):`);
  console.log(errorLog.join("\n"));
}

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
