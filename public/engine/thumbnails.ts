// LRU thumbnail cache + blit / render.

import type { MaybeCanvas, ThumbEntry, ThumbResult } from "./types";
import { el, errorInfo, fail, releaseCanvas, showBaked, showRaw } from "./canvas";
import { bakeRaster } from "./theme/bake";
import { readPipeline, pipelineCache } from "./theme/pipeline";
import {
  cacheDisplay,
  ensureEntryCurrent,
  paintCached,
  thumbRaw,
  thumbSource,
} from "./theme/thumbnails";
// A cold sidebar can mount a full thumbnail window at once. Limit pdf.js
// raster work, not clicks: queued jobs are invalidated on unmount and cached
// paths still paint immediately.
const THUMB_RENDER_LIMIT = 3;
let thumbActive = 0;
const thumbQueue: Array<() => void> = [];
const thumbGeneration = new Map<string, number>();

function nextThumbGeneration(canvasId: string): number {
  const next = (thumbGeneration.get(canvasId) ?? 0) + 1;
  thumbGeneration.set(canvasId, next);
  return next;
}

function pumpThumbQueue(): void {
  while (thumbActive < THUMB_RENDER_LIMIT && thumbQueue.length > 0) {
    const next = thumbQueue.shift();
    if (!next) return;
    thumbActive += 1;
    next();
  }
}

import {
  pdf,
  releaseThumbEntry,
  themeScrubActive,
  THUMB_CACHE_MAX,
  thumbCache,
  thumbCancelled,
  thumbLive,
  thumbTasks,
} from "./state";

export function cachePut(page: number, entry: ThumbEntry): void {
  if (thumbCache.has(page)) {
    const prev = thumbCache.get(page);
    thumbCache.delete(page);
    if (prev && prev !== entry) releaseThumbEntry(prev);
  }
  thumbCache.set(page, entry);
  while (thumbCache.size > THUMB_CACHE_MAX) {
    const oldest = thumbCache.keys().next();
    if (oldest.done || oldest.value === undefined) break;
    const oldEntry = thumbCache.get(oldest.value);
    thumbCache.delete(oldest.value);
    if (oldEntry && oldEntry !== entry) releaseThumbEntry(oldEntry);
  }
}

export function hasThumb(page: number, scale: number): boolean {
  const hit = thumbCache.get(page);
  return (
    !!hit &&
    Math.abs(hit.scale - scale) < 1e-9 &&
    (themeScrubActive || hit.gen === pipelineCache.gen || !!hit.raw)
  );
}

export function blitThumb(canvasId: string, page: number): boolean {
  const dst = el(canvasId) as HTMLCanvasElement | null;
  const entry = thumbCache.get(page);
  if (!dst || !entry) return false;
  const raw = themeScrubActive ? thumbRaw(entry) : null;
  const src = raw ?? thumbSource(entry);
  if (!src) return false;
  return raw
    ? showRaw(dst, raw, "thumb-raw")
    : showBaked(dst, src, "thumb-raw");
}

export async function renderThumb(
  canvasId: string,
  page: number,
  scale: number
): Promise<ThumbResult> {
  // A new mount supersedes any queued request for the same recycled canvas id.
  const generation = nextThumbGeneration(canvasId);
  thumbCancelled.delete(canvasId);

  // Cache hits are synchronous blits or a small display refresh; they do not
  // create pdf.js raster work and should never wait behind cold renders.
  if (hasThumb(page, scale)) {
    try {
      return await renderThumbInternal(canvasId, page, scale);
    } catch (e) {
      const info = errorInfo(e);
      return fail(info.name, info.message);
    }
  }

  return await new Promise<ThumbResult>((resolve) => {
    thumbQueue.push(() => {
      const finish = () => {
        thumbActive -= 1;
        pumpThumbQueue();
      };
      // The cell disappeared, or a newer mount re-used this id, before the
      // job reached the front of the queue. Drop it without touching pdf.js.
      if (
        thumbCancelled.has(canvasId)
        || thumbGeneration.get(canvasId) !== generation
      ) {
        resolve(fail("cancelled", "Thumbnail render cancelled"));
        finish();
        return;
      }
      renderThumbInternal(canvasId, page, scale)
        .then(resolve)
        .catch((e: unknown) => {
          const info = errorInfo(e);
          resolve(fail(info.name, info.message));
        })
        .finally(finish);
    });
    pumpThumbQueue();
  });
}

async function renderThumbInternal(
  canvasId: string,
  page: number,
  scale: number
): Promise<ThumbResult> {
  const canvas = el(canvasId) as HTMLCanvasElement | null;
  if (!canvas) return fail("no_canvas", "No canvas: " + canvasId);
  if (!pdf) return fail("no_document", "No document open");

  const hit = thumbCache.get(page);
  if (hit && Math.abs(hit.scale - scale) < 1e-9) {
    if (themeScrubActive) {
      if (showRaw(canvas, thumbRaw(hit), "thumb-raw")) {
        cachePut(page, hit);
        thumbLive.set(canvasId, { page });
        return { ok: true, width: hit.cssW, height: hit.cssH, scale, cached: true };
      }
    } else if (hit.gen === pipelineCache.gen) {
      const size = paintCached(canvas, hit);
      if (size) {
        cachePut(page, hit);
        thumbLive.set(canvasId, { page });
        return { ok: true, width: size.width, height: size.height, scale, cached: true };
      }
    } else if (await ensureEntryCurrent(hit)) {
      const size = paintCached(canvas, hit);
      if (size) {
        cachePut(page, hit);
        thumbLive.set(canvasId, { page });
        return { ok: true, width: size.width, height: size.height, scale, cached: false };
      }
    }
  }

  try { const t = thumbTasks.get(canvasId); if (t) t.cancel(); } catch (_) { /* ignore */ }
  thumbTasks.delete(canvasId);
  thumbCancelled.delete(canvasId);

  try {
    const pg = await pdf.getPage(page);
    const viewport = pg.getViewport({ scale });
    const out = 1;
    const cssW = Math.floor(viewport.width);
    const cssH = Math.floor(viewport.height);

    const off = document.createElement("canvas");
    off.width = Math.max(1, Math.floor(viewport.width * out));
    off.height = Math.max(1, Math.floor(viewport.height * out));
    const ctx = off.getContext("2d", { alpha: false });
    if (!ctx) {
      releaseCanvas(off);
      return fail("no_context", "No 2d context");
    }
    const transform = out !== 1 ? [out, 0, 0, out, 0, 0] : null;

    const task = pg.render({ canvasContext: ctx, viewport, transform });
    thumbTasks.set(canvasId, task);
    try {
      await task.promise;
    } catch (e) {
      thumbTasks.delete(canvasId);
      releaseCanvas(off);
      try { pg.cleanup(); } catch (_) { /* ignore */ }
      if ((e as { name?: string }).name === "RenderingCancelledException") {
        return fail("cancelled", "Thumb render cancelled");
      }
      const info = errorInfo(e);
      return fail(info.name, info.message);
    }
    thumbTasks.delete(canvasId);
    pg.cleanup();

    // Keep `off` as the unbaked raw for every later theme rebake. Never
    // alias raw === display and never release `off` here — cacheDisplay /
    // createImageBitmap used to zero the only unthemed copy, so a theme
    // change could not update visible thumbs until a full pdf.js re-render.
    const raw = off;
    let display: MaybeCanvas = themeScrubActive ? raw : bakeRaster(raw, readPipeline());
    if (display === raw) {
      if (typeof createImageBitmap === "function") {
        try {
          display = await createImageBitmap(raw);
        } catch (_) {
          display = raw;
        }
      }
    } else {
      display = await cacheDisplay({ display } as ThumbEntry);
    }
    const entry: ThumbEntry = {
      raw,
      display,
      cssW,
      cssH,
      scale,
      gen: themeScrubActive ? -1 : pipelineCache.gen,
      pending: null,
    };
    cachePut(page, entry);

    if (!thumbCancelled.has(canvasId)) {
      const live = el(canvasId) as HTMLCanvasElement | null;
      if (live) {
        thumbLive.set(canvasId, { page });
        paintCached(live, entry);
      }
    }
    thumbCancelled.delete(canvasId);

    return { ok: true, width: cssW, height: cssH, scale, cached: false };
  } catch (e) {
    thumbTasks.delete(canvasId);
    const info = errorInfo(e);
    return fail(info.name, info.message);
  }
}

export function cancelThumb(canvasId: string): void {
  // Invalidate a queued job as well as cancelling an active pdf.js task.
  nextThumbGeneration(canvasId);
  const task = thumbTasks.get(canvasId);
  if (task) {
    try { task.cancel(); } catch (_) { /* ignore */ }
    thumbTasks.delete(canvasId);
  }
  thumbCancelled.add(canvasId);
  thumbLive.delete(canvasId);
  releaseCanvas(el(canvasId) as HTMLCanvasElement | null);
}

/** Render a page into the cache with no DOM canvas (idle prefetch).
 *  A cache-hit cell mounts with `cached: true` → synchronous blit → zero
 *  skeleton, zero waiting. So render the pages AROUND the reader into the
 *  cache while idle; by the time the reader flings the grid to page N,
 *  pages N±k are cache-warm and every remount is an instant synchronous blit. */
export async function prefetchThumb(page: number, scale: number): Promise<void> {
  if (!pdf) return;
  const hit = thumbCache.get(page);
  if (hit && Math.abs(hit.scale - scale) < 1e-9) return;
  try {
    const pg = await pdf.getPage(page);
    const viewport = pg.getViewport({ scale });
    const off = document.createElement("canvas");
    off.width = Math.max(1, Math.floor(viewport.width));
    off.height = Math.max(1, Math.floor(viewport.height));
    const ctx = off.getContext("2d", { alpha: false });
    if (!ctx) { releaseCanvas(off); return; }
    const task = pg.render({ canvasContext: ctx, viewport });
    await task.promise;
    pg.cleanup();
    const raw = off;
    let display: MaybeCanvas = themeScrubActive ? raw : bakeRaster(raw, readPipeline());
    if (display !== raw) display = await cacheDisplay({ display });
    cachePut(page, { raw, display, cssW: Math.floor(viewport.width),
                     cssH: Math.floor(viewport.height), scale,
                     gen: themeScrubActive ? -1 : pipelineCache.gen, pending: null });
  } catch (_) { /* prefetch is best-effort */ }
}
