// LRU thumbnail cache + blit / render.

import type { MaybeCanvas, ThumbEntry, ThumbResult } from "./types";
import { el, fail, failFrom, offscreenFor, releaseCanvas, showBaked, showRaw } from "./canvas";
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

import { THUMB_CACHE_MAX, session } from "./state";

export function cachePut(page: number, entry: ThumbEntry): void {
  if (session.thumbCache.has(page)) {
    const prev = session.thumbCache.get(page);
    session.thumbCache.delete(page);
    if (prev && prev !== entry) session.releaseThumbEntry(prev);
  }
  session.thumbCache.set(page, entry);
  while (session.thumbCache.size > THUMB_CACHE_MAX) {
    const oldest = session.thumbCache.keys().next();
    if (oldest.done || oldest.value === undefined) break;
    const oldEntry = session.thumbCache.get(oldest.value);
    session.thumbCache.delete(oldest.value);
    if (oldEntry && oldEntry !== entry) session.releaseThumbEntry(oldEntry);
  }
}

export function hasThumb(page: number, scale: number): boolean {
  const hit = session.thumbCache.get(page);
  return (
    !!hit &&
    Math.abs(hit.scale - scale) < 1e-9 &&
    (session.themeScrubActive || hit.gen === pipelineCache.gen || !!hit.raw)
  );
}

export function blitThumb(canvasId: string, page: number): boolean {
  const dst = el(canvasId) as HTMLCanvasElement | null;
  const entry = session.thumbCache.get(page);
  if (!dst || !entry) return false;
  const raw = session.themeScrubActive ? thumbRaw(entry) : null;
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
  session.thumbCancelled.delete(canvasId);

  // Cache hits are synchronous blits or a small display refresh; they do not
  // create pdf.js raster work and should never wait behind cold renders.
  if (hasThumb(page, scale)) {
    try {
      return await renderThumbInternal(canvasId, page, scale);
    } catch (e) {
      return failFrom(e);
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
        session.thumbCancelled.has(canvasId)
        || thumbGeneration.get(canvasId) !== generation
      ) {
        resolve(fail("cancelled", "Thumbnail render cancelled"));
        finish();
        return;
      }
      renderThumbInternal(canvasId, page, scale)
        .then(resolve)
        .catch((e: unknown) => {
          resolve(failFrom(e));
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
  if (!session.pdf) return fail("no_document", "No document open");

  const hit = session.thumbCache.get(page);
  if (hit && Math.abs(hit.scale - scale) < 1e-9) {
    if (session.themeScrubActive) {
      if (showRaw(canvas, thumbRaw(hit), "thumb-raw")) {
        cachePut(page, hit);
        session.thumbLive.set(canvasId, { page });
        return { ok: true, width: hit.cssW, height: hit.cssH, scale };
      }
    } else if (hit.gen === pipelineCache.gen) {
      const size = paintCached(canvas, hit);
      if (size) {
        cachePut(page, hit);
        session.thumbLive.set(canvasId, { page });
        return { ok: true, width: size.width, height: size.height, scale };
      }
    } else if (await ensureEntryCurrent(hit)) {
      const size = paintCached(canvas, hit);
      if (size) {
        cachePut(page, hit);
        session.thumbLive.set(canvasId, { page });
        return { ok: true, width: size.width, height: size.height, scale };
      }
    }
  }

  try { const t = session.thumbTasks.get(canvasId); if (t) t.cancel(); } catch (_) { /* ignore */ }
  session.thumbTasks.delete(canvasId);
  session.thumbCancelled.delete(canvasId);

  try {
    const pg = await session.pdf.getPage(page);
    const viewport = pg.getViewport({ scale });
    const out = 1;
    const cssW = Math.floor(viewport.width);
    const cssH = Math.floor(viewport.height);

    const made = offscreenFor(viewport, out);
    if (!made) return fail("no_context", "No 2d context");
    const { canvas: off, ctx } = made;
    const transform = out !== 1 ? [out, 0, 0, out, 0, 0] : null;

    const task = pg.render({ canvasContext: ctx, viewport, transform });
    session.thumbTasks.set(canvasId, task);
    try {
      await task.promise;
    } catch (e) {
      session.thumbTasks.delete(canvasId);
      releaseCanvas(off);
      try { pg.cleanup(); } catch (_) { /* ignore */ }
      if ((e as { name?: string }).name === "RenderingCancelledException") {
        return fail("cancelled", "Thumb render cancelled");
      }
      return failFrom(e);
    }
    session.thumbTasks.delete(canvasId);
    pg.cleanup();

    // Keep `off` as the unbaked raw for every later theme rebake. Never
    // alias raw === display and never release `off` here — cacheDisplay /
    // createImageBitmap used to zero the only unthemed copy, so a theme
    // change could not update visible thumbs until a full pdf.js re-render.
    const raw = off;
    let display: MaybeCanvas = session.themeScrubActive ? raw : await bakeRaster(raw, readPipeline());
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
      gen: session.themeScrubActive ? -1 : pipelineCache.gen,
      pending: null,
    };
    cachePut(page, entry);

    if (!session.thumbCancelled.has(canvasId)) {
      const live = el(canvasId) as HTMLCanvasElement | null;
      if (live) {
        session.thumbLive.set(canvasId, { page });
        paintCached(live, entry);
      }
    }
    session.thumbCancelled.delete(canvasId);

    return { ok: true, width: cssW, height: cssH, scale };
  } catch (e) {
    session.thumbTasks.delete(canvasId);
    return failFrom(e);
  }
}

export function cancelThumb(canvasId: string): void {
  // Invalidate a queued job as well as cancelling an active pdf.js task.
  nextThumbGeneration(canvasId);
  const task = session.thumbTasks.get(canvasId);
  if (task) {
    try { task.cancel(); } catch (_) { /* ignore */ }
    session.thumbTasks.delete(canvasId);
  }
  session.thumbCancelled.add(canvasId);
  session.thumbLive.delete(canvasId);
  releaseCanvas(el(canvasId) as HTMLCanvasElement | null);
}

/** Render a page into the cache with no DOM canvas (idle prefetch).
 *  A cell whose page is cache-warm asks `hasThumb` while it is still being
 *  built, mounts already loaded, and its first render call is a synchronous
 *  blit → zero skeleton, zero waiting. So render the pages AROUND the reader
 *  into the cache while idle; by the time the reader flings the grid to page
 *  N, pages N±k answer that probe true and every remount is instant. */
export async function prefetchThumb(page: number, scale: number): Promise<void> {
  if (!session.pdf) return;
  const hit = session.thumbCache.get(page);
  if (hit && Math.abs(hit.scale - scale) < 1e-9) return;
  try {
    const pg = await session.pdf.getPage(page);
    const viewport = pg.getViewport({ scale });
    const made = offscreenFor(viewport);
    if (!made) return;
    const { canvas: off, ctx } = made;
    const task = pg.render({ canvasContext: ctx, viewport });
    await task.promise;
    pg.cleanup();
    const raw = off;
    let display: MaybeCanvas = session.themeScrubActive ? raw : await bakeRaster(raw, readPipeline());
    if (display !== raw) display = await cacheDisplay({ display });
    cachePut(page, { raw, display, cssW: Math.floor(viewport.width),
                     cssH: Math.floor(viewport.height), scale,
                     gen: session.themeScrubActive ? -1 : pipelineCache.gen, pending: null });
  } catch (_) { /* prefetch is best-effort */ }
}
