// =====================================================================
// pdfEngine.ts — window.PDFReader facade.
//
// Implementation lives in public/engine/* (loader, renderer, thumbnails,
// search, theme). This file only wires the public API and document teardown.
// Compiled to public/pdfEngine.js; the browser loads it as an ES module.
// =====================================================================

export {};

import type { PDFReaderApi, Stats } from "./engine/types";
import { disposeScratch, releaseCanvas } from "./engine/canvas";
import { coverDataUrl, open, resolveOutline, takePendingFile } from "./engine/loader";
import {
  cancelPage,
  registerPage,
  renderPage,
  rerenderLivePages,
  unregisterPage,
} from "./engine/renderer";
import {
  blitThumb,
  cancelThumb,
  hasThumb,
  prefetchThumb,
  renderThumb,
} from "./engine/thumbnails";
import { buildSearchIndex, clearHighlights, search, setActiveMatch } from "./engine/search";
import { rebakeTheme, setScrubModeInternal } from "./engine/theme/scrub";
import { invalidatePipeline } from "./engine/theme/pipeline";
import { paintAllVisibleThumbs } from "./engine/theme/thumbnails";
import {
  getCachedPaper,
  persistPaper,
  resetPaperForDocument,
  samplePaperPage,
  setPaper,
  setPaperActive,
  takePaperFrame,
} from "./engine/paper";
import { installSelectionTracker } from "./engine/selection";
import {
  ENGINE_VERSION,
  loadingTask,
  releasePageSurfaces,
  releaseThumbEntry,
  setCurrentPath,
  setLoadingTask,
  setNumPages,
  setPdf,
  stateByCanvasId,
  THUMB_CACHE_MAX,
  thumbCache,
  thumbCancelled,
  thumbLive,
  thumbTasks,
  textIndex,
  highlightsByPage,
  setSearchQuery,
  setActiveMatchValue,
  sweepPdf,
  themeScrubActive,
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

async function destroy(): Promise<void> {
  for (const st of stateByCanvasId.values()) {
    st.dead = true;
    try { st.renderTask && st.renderTask.cancel(); } catch (_) { /* ignore */ }
    try { st.textLayer && st.textLayer.cancel(); } catch (_) { /* ignore */ }
    if (st.queueHandle) {
      cancelAnimationFrame(st.queueHandle);
      st.queueHandle = 0;
    }
    releasePageSurfaces(st);
  }
  stateByCanvasId.clear();
  for (const task of thumbTasks.values()) {
    try { task.cancel(); } catch (_) { /* ignore */ }
  }
  thumbTasks.clear();
  thumbCancelled.clear();
  thumbLive.clear();
  for (const entry of thumbCache.values()) releaseThumbEntry(entry);
  thumbCache.clear();
  textIndex.clear();
  highlightsByPage.clear();
  setSearchQuery("");
  setActiveMatchValue(null);
  if (loadingTask) {
    const lt = loadingTask;
    setLoadingTask(null);
    Promise.resolve(lt.destroy()).catch(() => { /* fire-and-forget */ });
  }
  setPdf(null);
  setNumPages(0);
  setCurrentPath(null);
  resetPaperForDocument();
  disposeScratch();
}

(globalThis as unknown as { __pdfDestroy?: () => Promise<void> }).__pdfDestroy = destroy;

// Rust invokes these APIs fire-and-forget. Keep one promise chain so a pause
// in a tint drag cannot interleave `scrub off → bake` with a new `scrub on`.
// A failed mutation is reported but deliberately swallowed so it never poisons
// the queue and blocks every later appearance change.
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
  // rebake (or page rerender) against that same pipeline here.
  if (themeScrubActive) return;
  await rebakeTheme();
  // Pages without a distinct raw must re-render from pdf.js (never
  // double-filter). Thumbs were already refreshed in rebakeTheme.
  let needsRerender = false;
  for (const st of stateByCanvasId.values()) {
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
  for (const [canvasId, { page }] of thumbLive) {
    const entry = thumbCache.get(page);
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

function stats(): Stats {
  return {
    pages: stateByCanvasId.size,
    thumbs: thumbCache.size,
    thumbLimit: THUMB_CACHE_MAX,
    thumbTasks: thumbTasks.size,
  };
}

function releaseAllSurfaces(): void {
  for (const st of stateByCanvasId.values()) {
    st.dead = true;
    try { st.renderTask && st.renderTask.cancel(); } catch (_) { /* ignore */ }
    try { st.textLayer && st.textLayer.cancel(); } catch (_) { /* ignore */ }
    if (st.queueHandle) {
      cancelAnimationFrame(st.queueHandle);
      st.queueHandle = 0;
    }
    releasePageSurfaces(st);
  }
  for (const entry of thumbCache.values()) releaseThumbEntry(entry);
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
    for (const st of stateByCanvasId.values()) {
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
installSelectionTracker();

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
  buildSearchIndex,
  search,
  setActiveMatch,
  clearHighlights,
  refreshTheme,
  setScrubMode,
  setPaper,
  setPaperActive,
  persistPaper,
  takePaperFrame,
  samplePaperPage,
  getCachedPaper,
  sweep: () => {
    sweepPdf();
  },
  takePendingFile,
  prefetchThumb,
} satisfies PDFReaderApi;
