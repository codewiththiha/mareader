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
import { coverDataUrl, destroyTask, open, resolveOutline, takePendingFile } from "./engine/loader";
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
import {
  clearHighlights,
  extractPageText,
  setActiveMatch,
  setSearchContext,
} from "./engine/search";
import { rebakeTheme, setPipelineModeInternal, setScrubModeInternal } from "./engine/theme/scrub";
import { invalidatePipeline, isLivePipeline } from "./engine/theme/pipeline";
import { paintAllVisibleThumbs } from "./engine/theme/thumbnails";
import {
  clearLegacyPaperCache,
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

async function destroy(): Promise<void> {
  try {
    for (const st of session.stateByCanvasId.values()) {
      st.dead = true;
      try { st.renderTask && st.renderTask.cancel(); } catch (_) { /* ignore */ }
      try { st.textLayer && st.textLayer.cancel(); } catch (_) { /* ignore */ }
      if (st.queueHandle) {
        cancelAnimationFrame(st.queueHandle);
        st.queueHandle = 0;
      }
      session.releasePageSurfaces(st);
    }
    session.stateByCanvasId.clear();
    for (const task of session.thumbTasks.values()) {
      try { task.cancel(); } catch (_) { /* ignore */ }
    }
    session.thumbTasks.clear();
    session.thumbCancelled.clear();
    session.thumbLive.clear();
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
    disposeScratch();
  }
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
  if (session.themeScrubActive) return;
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

/** Reader-facing pipeline switch (Appearance ▸ Rendering). Queued with every
 * other theme mutation so a mode flip mid-scrub still ends in a consistent
 * raster state. */
function setLivePipelineMode(on: boolean): Promise<void> {
  return enqueueTheme(() => setPipelineModeInternal(on));
}

function stats(): Stats {
  return {
    pages: session.stateByCanvasId.size,
    thumbs: session.thumbCache.size,
    thumbLimit: THUMB_CACHE_MAX,
    thumbTasks: session.thumbTasks.size,
  };
}

function releaseAllSurfaces(): void {
  for (const st of session.stateByCanvasId.values()) {
    st.dead = true;
    try { st.renderTask && st.renderTask.cancel(); } catch (_) { /* ignore */ }
    try { st.textLayer && st.textLayer.cancel(); } catch (_) { /* ignore */ }
    if (st.queueHandle) {
      cancelAnimationFrame(st.queueHandle);
      st.queueHandle = 0;
    }
    session.releasePageSurfaces(st);
  }
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
clearLegacyPaperCache();

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
  setLivePipeline: setLivePipelineMode,
  isLivePipeline,
  setPaper,
  setPaperActive,
  clearLegacyPaperCache,
  takePaperFrame,
  samplePaperPage,
  sweep: () => {
    session.sweepPdf();
  },
  takePendingFile,
  prefetchThumb,
} satisfies PDFReaderApi;

// The engine contract is fixed by the Rust bridge: surface integrity beats
// extensibility, so freeze the object (has_pdf_reader only checks existence).
Object.freeze(globalThis.PDFReader);

// Boot in the engine's default mode. Live means the raw rasters go under the
// CSS pipeline immediately; the reader's persisted choice is applied by the
// app right after mount, through setLivePipeline.
if (isLivePipeline()) {
  void setScrubMode(true);
}
