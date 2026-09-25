// window.PDFReader facade. The implementation lives in public/engine/*
// (loader, renderer, thumbnails, search, theme); this file wires the public
// API and document teardown. Compiled to public/pdfEngine.js and loaded by
// the browser as an ES module.

export {};

import type { PDFReaderApi, Stats } from "./engine/types";
import {
  disposeScratch,
  pooledIntermediateBytesEstimate,
  releaseCanvas,
} from "./engine/canvas";
import { coverDataUrl, destroyTask, open, resolveOutline, takePendingFile } from "./engine/loader";
import {
  beginRenderGeneration,
  cancelPage,
  drainPageLane,
  pageLaneGauge,
  readRenderTrace,
  registerPage,
  renderPage,
  rerenderLivePages,
  unregisterPage,
} from "./engine/renderer";
import {
  blitThumb,
  cancelThumb,
  hasThumb,
  thumbGenerationSize,
  thumbLaneGauge,
  prefetchThumb,
  renderThumb,
  resetThumbLane,
} from "./engine/thumbnails";
import {
  clearHighlights,
  extractPageText,
  setActiveMatch,
  setSearchContext,
} from "./engine/search";
import { rebakeTheme, setScrubModeInternal } from "./engine/theme/scrub";
import { invalidatePipeline } from "./engine/theme/pipeline";
import { publishBakedPaper, watchPaperTokens } from "./engine/theme/paper";
import { paintAllVisibleThumbs } from "./engine/theme/thumbnails";
import {
  resetPaperForDocument,
  samplePaperPage,
  setPaper,
  setPaperActive,
  takePaperFrame,
} from "./engine/paper";
import {
  ENGINE_VERSION,
  lifecycleEvent,
  session,
  setLifecycleLog,
  THUMB_CACHE_MAX,
} from "./engine/state";

declare global {
  interface Window {
    pdfjsLib: unknown;
  }
  // eslint-disable-next-line no-var
  var pdfjsLib: unknown;
  // eslint-disable-next-line no-var
  var __TAURI__:
    | {
        core: {
          invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;
          convertFileSrc: (p: string) => string;
        };
      }
    | undefined;
  // eslint-disable-next-line no-var
  var PDFReader: PDFReaderApi;
}

/** Cancel and release every live page surface. Shared by `destroy` and the
 *  `pagehide` handler: both must stop in-flight renders and free the surfaces,
 *  and only one of them goes on to null the document out. */
function cancelAndReleasePages(): void {
  for (const st of session.stateByCanvasId.values()) {
    st.dead = true;
    try { st.renderTask && st.renderTask.cancel(); } catch (_) { /* ignore */ }
    try { st.textLayer && st.textLayer.cancel(); } catch (_) { /* ignore */ }
    // A queued render's rAF is deliberately NOT cancelled: the callback's
    // dead-state guard resolves the caller with a drop. Cancelling here
    // would orphan that promise — an await that never settles, for a job
    // the counters never even saw.
    session.releasePageSurfaces(st);
  }
}

async function destroy(): Promise<void> {
  // A dispose is only a session dispose when a session is actually here:
  // open() runs destroy() as its first act, and that call disposes nothing.
  const hadSession = session.pdf !== null;
  if (hadSession) {
    session.sessionsDestroyed += 1;
    // First act of dying: wake every in-flight prefetch await so none of
    // them can step onto the worker during its death throes (a task born
    // in this window would await a promise the destroyed worker never
    // settles, and its active-prefetch slot would never drain).
    session.noteDocumentGone();
    // The idle sweeper belongs to the dying document: leaving it armed lets
    // a timer fire over the NEXT document thirty seconds later.
    session.clearIdleTimer();
    lifecycleEvent("pdf_session:dispose_begin");
  }
  try {
    // The advisory worker cleanup, run while the document is still alive:
    // pdf.cleanup() drops the resolved-page and font caches pdf.js holds for
    // it. Nothing after this point can — the teardown below nulls the
    // document and fires the worker's death, so a shelf-side sweep() arriving
    // after destroy resolves finds no document to clean.
    session.sweepPdf();
    cancelAndReleasePages();
    // Every state is dead now: the drain cascade resolves each queued job
    // as a drop instead of leaving the closures (and their callers'
    // resolvers) parked in the array across the dispose.
    drainPageLane();
    session.stateByCanvasId.clear();
    for (const task of session.thumbTasks.values()) {
      try { task.cancel(); } catch (_) { /* ignore */ }
    }
    session.thumbTasks.clear();
    session.thumbCancelled.clear();
    session.thumbLive.clear();
    // The lane's queued jobs and per-id generation counters belong to this
    // document: the epoch invalidates the queue, and the counters go with it
    // so the next document's recycled `thumb-{page}` ids start clean.
    resetThumbLane();
    for (const entry of session.thumbCache.values()) session.releaseThumbEntry(entry);
    session.thumbCache.clear();
    session.setSearchQuery("");
    session.setActiveMatchValue(null);
    if (session.loadingTask) {
      // Guarded behind a WeakSet in loader.ts: open's own timeout may be
      // destroying the same task right now, and a second destroy() on a
      // pdf.js LoadingTask double-frees the worker.
      //
      // AWAITED, not fire-and-forget: the reader's dispose-complete marker
      // fires after this resolves, and it must mean "the worker is dead",
      // not "worker death was scheduled". The close tail already runs in
      // its own background task, so the wait costs the UI nothing; the
      // open flow's own pre-destroy gains the same truth for free, still
      // bounded by the open timeout behind it.
      const lt = session.loadingTask;
      session.setLoadingTask(null);
      await destroyTask(lt);
    }
  } finally {
    // Teardown always completes: a release that throws must not skip the
    // document null-out, or the next open() sees a half-dead session.
    session.setPdf(null);
    session.setNumPages(0);
    session.setCurrentPath(null);
    resetPaperForDocument();
    // The document's paper goes with it: setPdf's null-out cleared
    // --pdf-paper, and the backdrop's pre-themed twin must not outlive the
    // book it was themed for.
    publishBakedPaper();
    disposeScratch();
    if (hadSession) lifecycleEvent("pdf_session:dispose_complete");
  }
}

(globalThis as unknown as { __pdfDestroy?: () => Promise<void> }).__pdfDestroy = destroy;

// Rust invokes these fire-and-forget, so all mutations ride one promise
// chain: a pause in a tint drag cannot interleave `scrub off -> bake` with a
// new `scrub on`. A failed mutation is reported but swallowed so it never
// poisons the queue and blocks every later appearance change.
let themeChain: Promise<void> = Promise.resolve();

function enqueueTheme(work: () => Promise<void>): Promise<void> {
  themeChain = themeChain
    .then(work, work)
    .catch((e: unknown) => {
      const msg = (e as { message?: string })?.message ?? e;
      console.warn("[pdfEngine] theme mutation failed:", msg);
    });
  return themeChain;
}

async function refreshThemeInternal(): Promise<void> {
  invalidatePipeline();
  // A slider commit arrives while scrub owns raw, individually tagged
  // canvases. Exit performs the single final bake, so do not enqueue a second
  // rebake (or page rerender) against that same pipeline here. The backdrop's
  // published paper still settles: the rasters belong to the scrub, but the
  // token move this was called for — a texture's fold, paper.ts stage three —
  // is already on the root style.
  if (session.themeScrubActive) {
    publishBakedPaper();
    return;
  }
  await rebakeTheme();
  // Pages without a distinct raw must re-render from pdf.js (never
  // double-filter). Thumbs were already refreshed in rebakeTheme.
  let needsRerender = false;
  for (const st of session.stateByCanvasId.values()) {
    if (!st.dead && st.canvas && (!st.rawCanvas || st.rawCanvas === st.canvas)) {
      needsRerender = true;
      break;
    }
  }
  if (needsRerender) await rerenderLivePages();
  // Rebake already updated thumbCache; blit onto every visible sidebar
  // canvas. If a cache entry lost its unbaked raw, re-render that thumb
  // from pdf.js the same way live pages do.
  const thumbJobs: Promise<unknown>[] = [];
  for (const [canvasId, { page }] of session.thumbLive) {
    const entry = session.thumbCache.get(page);
    if (!entry || !entry.display || (entry.display as ImageBitmap).width <= 0) {
      thumbJobs.push(renderThumb(canvasId, page, entry?.scale || 0.25));
    }
  }
  if (thumbJobs.length) await Promise.all(thumbJobs);
  paintAllVisibleThumbs();
}

function refreshTheme(): Promise<void> {
  return enqueueTheme(refreshThemeInternal);
}

function setScrubMode(on: boolean): Promise<void> {
  return enqueueTheme(() => setScrubModeInternal(on));
}

// Not enqueued: a retention flag, not a canvas mutation. The theme queue
// serializes raster swaps; a menu toggle must neither wait behind a bake
// nor delay one, and setting session state is synchronous anyway.
function setAppearanceMenuOpen(on: boolean): void {
  session.setAppearanceMenuOpen(on);
}

function stats(): Stats {
  let activeRenders = 0;
  let pageCanvasBytes = 0;
  let rawRetentionBytes = 0;
  for (const st of session.stateByCanvasId.values()) {
    if (st.renderTask) activeRenders += 1;
    // Engine-owned raster estimates: w*h*4 RGBA, by convention. These are
    // correlation numbers for the baseline (what the engine holds), never a
    // physical allocation query and never a share of a "total RAM".
    if (st.canvas) pageCanvasBytes += st.canvas.width * st.canvas.height * 4;
    if (st.rawCanvas && st.rawCanvas !== st.canvas) {
      rawRetentionBytes += st.rawCanvas.width * st.rawCanvas.height * 4;
    }
  }
  let thumbnailRasterBytes = 0;
  for (const t of session.thumbCache.values()) {
    for (const c of [t.raw, t.display]) {
      if (c) thumbnailRasterBytes += c.width * c.height * 4;
    }
  }
  const pageLane = pageLaneGauge();
  const thumbLane = thumbLaneGauge();
  return {
    pageQueue: pageLane.pageQueue,
    pageActive: pageLane.pageActive,
    thumbQueue: thumbLane.thumbQueue,
    thumbActive: thumbLane.thumbActive,
    pages: session.stateByCanvasId.size,
    thumbs: session.thumbCache.size,
    thumbLimit: THUMB_CACHE_MAX,
    thumbTasks: session.thumbTasks.size,
    activeRenders,
    activePrefetches: session.prefetchesActive,
    hasDocument: session.pdf !== null,
    hasLoadingTask: session.loadingTask !== null,
    sessionsOpened: session.sessionsOpened,
    sessionsDestroyed: session.sessionsDestroyed,
    workersCreated: session.workersCreated,
    workersTerminated: session.workersTerminated,
    rendersStarted: session.rendersStarted,
    rendersCompleted: session.rendersCompleted,
    rendersCancelled: session.rendersCancelled,
    rendersFailed: session.rendersFailed,
    rendersQueued: session.rendersQueued,
    rendersDropped: session.rendersDropped,
    prefetchesStarted: session.prefetchesStarted,
    prefetchesCompleted: session.prefetchesCompleted,
    prefetchesDropped: session.prefetchesDropped,
    documentPages: session.numPages,
    thumbGenerationSize: thumbGenerationSize(),
    rawRetentionTimers: session.rawRetentionTimers(),
    sweepTimerArmed: session.sweepTimerArmed(),
    pageCanvasBytesEst: pageCanvasBytes,
    thumbnailRasterBytesEst: thumbnailRasterBytes,
    rawRetentionBytesEst: rawRetentionBytes,
    pooledIntermediateBytesEst: pooledIntermediateBytesEstimate(),
  };
}

function releaseAllSurfaces(): void {
  cancelAndReleasePages();
  for (const entry of session.thumbCache.values()) session.releaseThumbEntry(entry);
  try {
    document.querySelectorAll("canvas").forEach((c) => releaseCanvas(c as HTMLCanvasElement));
  } catch (_) { /* document already torn down */ }
  disposeScratch();
}

globalThis.addEventListener("pagehide", releaseAllSurfaces);
try {
  globalThis.addEventListener("visibilitychange", () => {
    if (document.visibilityState !== "hidden") return;
    // Keep the live baked canvases (so coming back isn't a blank page)
    // but drop idle raws, scratch, and worker caches.
    for (const st of session.stateByCanvasId.values()) {
      if (st.rawCanvas && st.rawCanvas !== st.canvas) {
        releaseCanvas(st.rawCanvas);
        st.rawCanvas = null;
      }
    }
    disposeScratch();
  });
} catch (_) {
  /* no document */
}
// The selection tracker is NOT installed here: it is format-agnostic and
// lives in the reader bundle (public/readerEngine.ts), which index.html loads
// first. Nothing in this facade depends on it.

// The standing watch over the tokens the published backdrop paper is
// computed from (public/engine/theme/paper.ts): a drag repaints the root
// per frame and a texture click never reaches the scheduler at all, so the
// publish rides the mutations instead of waiting to be called. Installed
// with the other module-lifetime listeners; self-guarded where there is no
// MutationObserver (the node smoke harness).
watchPaperTokens();

globalThis.PDFReader = {
  version: () => ENGINE_VERSION,
  beginRenderGeneration,
  renderTrace: readRenderTrace,
  open,
  resolveOutline,
  destroy,
  setLifecycleLog,
  registerPage,
  unregisterPage,
  cancelPage,
  renderPage,
  renderThumb,
  cancelThumb,
  hasThumb,
  blitThumb,
  coverDataUrl,
  stats,
  extractPageText,
  setSearchContext,
  setActiveMatch,
  clearHighlights,
  refreshTheme,
  setScrubMode,
  setAppearanceMenuOpen,
  setPaper,
  setPaperActive,
  takePaperFrame,
  samplePaperPage,
  sweep: () => {
    session.sweepPdf();
  },
  sweepSnapshots: () => {
    session.sweepSnapshots();
  },
  takePendingFile,
  prefetchThumb,
} satisfies PDFReaderApi;

// The engine contract is fixed by the Rust bridge: surface integrity beats
// extensibility, so freeze the object (has_pdf_reader only checks existence).
Object.freeze(globalThis.PDFReader);

