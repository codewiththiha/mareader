// window.PDFReader facade. The implementation lives in public/engine/*
// (loader, renderer, thumbnails, search, theme); this file wires the public
// API and document teardown. Compiled to public/pdfEngine.js and loaded by
// the browser as an ES module.

export {};

import type { PDFReaderApi, Stats } from "./engine/types";
import { disposeScratch, releaseCanvas } from "./engine/canvas";
import { coverDataUrl, destroyTask, open, resolveOutline, takePendingFile } from "./engine/loader";
import {
  cancelPage,
  registerPage,
  renderPage,
  rerenderLivePages,
  unregisterPage,
} from "./engine/renderer";
import { onMotionPublished, resetScheduler, schedulerStats, unschedule } from "./engine/scheduler";
import {
  configureBudget,
  currentBudget,
  predictionStats,
  resetMotion,
  setMotion,
} from "./engine/motion";
import { reclaim, totalBytes } from "./engine/memory";
import { resetPlaceholder, republishPlaceholder } from "./engine/placeholder";
import {
  blitThumb,
  cancelThumb,
  hasThumb,
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
  session,
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
  for (const [canvasId, st] of session.stateByCanvasId) {
    st.dead = true;
    try { st.renderTask && st.renderTask.cancel(); } catch (_) { /* ignore */ }
    try { st.textLayer && st.textLayer.cancel(); } catch (_) { /* ignore */ }
    // Drop what is still waiting for a lane; the in-flight render is the
    // cancel above, and pdf.js is the only thing that can stop it.
    unschedule(canvasId);
    session.releasePageSurfaces(st);
  }
}

async function destroy(): Promise<void> {
  try {
    // The advisory worker cleanup, run while the document is still alive:
    // pdf.cleanup() drops the resolved-page and font caches pdf.js holds for
    // it. Nothing after this point can — the teardown below nulls the
    // document and fires the worker's death, so a shelf-side sweep() arriving
    // after destroy resolves finds no document to clean.
    session.sweepPdf();
    cancelAndReleasePages();
    // The queue, the motion and the miniature all belong to the document that
    // is going away: a request left waiting would render into a canvas the
    // teardown just released, a fling's published windows would pace the next
    // book's first renders, and a placeholder of the last cover would stand in
    // for the next document's pages.
    resetScheduler();
    resetMotion();
    resetPlaceholder();
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
      const lt = session.loadingTask;
      session.setLoadingTask(null);
      destroyTask(lt).catch(() => { /* fire-and-forget */ });
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
  // The placeholder miniature is themed like every other raster: baked under
  // Light, it is a white rectangle in Dark, and the boxes painting it are on
  // screen exactly while the reader scrolls.
  await republishPlaceholder();
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
  const predictions = predictionStats();
  return {
    pages: session.stateByCanvasId.size,
    thumbs: session.thumbCache.size,
    thumbLimit: THUMB_CACHE_MAX,
    thumbTasks: session.thumbTasks.size,
    scheduler: schedulerStats(),
    predictions: { made: predictions.predictions, hits: predictions.hits },
    memory: {
      bytes: totalBytes(),
      budget: currentBudget().maxBytes,
      previews: [...session.stateByCanvasId.values()].filter((st) => !st.dead && st.preview).length,
    },
  };
}

/** One scroll frame from the strip that owns the scroller. The three calls
 *  are one transaction: adopt the motion, re-order (and drop) what is waiting
 *  against it, then weigh the ledger — a movement that just made a page
 *  irrelevant is also the moment its surfaces become reclaimable. */
function setScrollMotion(
  phase: string,
  direction: number,
  predictedPage: number,
  firstFull: number,
  lastFull: number,
  firstPreview: number,
  lastPreview: number,
  delayMs: number,
  workers: number,
): void {
  setMotion(
    phase,
    direction,
    predictedPage,
    firstFull,
    lastFull,
    firstPreview,
    lastPreview,
    delayMs,
    workers,
  );
  onMotionPublished();
  reclaim(true);
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
  open,
  resolveOutline,
  destroy,
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
  setScrollMotion,
  configureMotion: configureBudget,
} satisfies PDFReaderApi;

// The engine contract is fixed by the Rust bridge: surface integrity beats
// extensibility, so freeze the object (has_pdf_reader only checks existence).
Object.freeze(globalThis.PDFReader);

