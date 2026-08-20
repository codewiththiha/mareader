// LRU thumbnail cache + blit / render.

import type { MaybeCanvas, ThumbEntry, ThumbResult } from "./types";
import { blitInto, el, errorInfo, fail, releaseCanvas } from "./canvas";
import {
  bakeRaster,
  cacheDisplay,
  ensureEntryCurrent,
  paintCached,
  pipelineCache,
  readPipeline,
  thumbRaw,
  thumbSource,
} from "./theme";
import {
  pdf,
  releaseThumbEntry,
  scrubbing,
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
    (scrubbing || hit.gen === pipelineCache.gen || !!hit.raw)
  );
}

export function blitThumb(canvasId: string, page: number): boolean {
  const dst = el(canvasId) as HTMLCanvasElement | null;
  const entry = thumbCache.get(page);
  if (!dst || !entry) return false;
  const src = scrubbing ? thumbRaw(entry) : thumbSource(entry);
  if (!src) return false;
  if (dst.width <= 0 || dst.height <= 0) {
    dst.width = (src as ImageBitmap).width;
    dst.height = (src as ImageBitmap).height;
  }
  const ctx = dst.getContext("2d");
  if (!ctx) return false;
  try {
    ctx.drawImage(src as CanvasImageSource, 0, 0, dst.width, dst.height);
    return true;
  } catch (_) {
    return false;
  }
}

export async function renderThumb(
  canvasId: string,
  page: number,
  scale: number
): Promise<ThumbResult> {
  try {
    return await renderThumbInternal(canvasId, page, scale);
  } catch (e) {
    const info = errorInfo(e);
    return fail(info.name, info.message);
  }
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
    if (scrubbing) {
      if (blitInto(canvas, thumbRaw(hit))) {
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
    let display: MaybeCanvas = scrubbing ? raw : bakeRaster(raw, readPipeline());
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
      gen: scrubbing ? -1 : pipelineCache.gen,
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
  const task = thumbTasks.get(canvasId);
  if (task) {
    try { task.cancel(); } catch (_) { /* ignore */ }
    thumbTasks.delete(canvasId);
  }
  thumbCancelled.add(canvasId);
  thumbLive.delete(canvasId);
  releaseCanvas(el(canvasId) as HTMLCanvasElement | null);
}
