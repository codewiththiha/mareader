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
import { coverDataUrl, open, takePendingFile } from "./engine/loader";
import {
  cancelPage,
  preparePagesForScrub,
  registerPage,
  renderPage,
  rerenderLivePages,
  unregisterPage,
} from "./engine/renderer";
import {
  blitThumb,
  cancelThumb,
  hasThumb,
  renderThumb,
} from "./engine/thumbnails";
import {
  buildSearchIndex,
  clearHighlights,
  search,
  setActiveMatch,
  setHighlightMode,
} from "./engine/search";
import {
  rebakeTheme,
  applyScrubMode,
  invalidatePipeline,
  paintAllVisibleThumbs,
} from "./engine/theme";
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
  setScrubbing,
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
  disposeScratch();
}

(globalThis as unknown as { __pdfDestroy?: () => Promise<void> }).__pdfDestroy = destroy;

async function refreshTheme(): Promise<void> {
  invalidatePipeline();
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

async function setScrubMode(on: boolean): Promise<void> {
  if (on) {
    // Produce unbaked raws BEFORE the CSS filter class goes on, otherwise
    // the first drag frame double-filters the baked Dark/Dim raster.
    setScrubbing(true);
    await preparePagesForScrub();
  }
  await applyScrubMode(on);
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
  releaseAllSurfaces,
  open,
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
  setHighlightMode,
  clearHighlights,
  refreshTheme,
  setScrubMode,
  takePendingFile,
} satisfies PDFReaderApi;
