// Themed thumbnail display: the raw/display split per cache entry, the
// ImageBitmap display cache, and painting cached entries onto live DOM
// canvases.

import type { MaybeCanvas, ThumbEntry } from "../types";
import {
  acquirePooledCanvas,
  blitInto,
  el,
  isSharedScratch,
  releasePooledCanvas,
  releaseScratch,
  showBaked,
  showRaw,
} from "../canvas";
import { session } from "../state";
import { bakeRaster, rasterToCanvas } from "./bake";
import { pipelineCache, readPipeline } from "./pipeline";

export function releaseDisplayOnly(entry: ThumbEntry | null): void {
  if (!entry) return;
  try {
    if (entry.display && typeof (entry.display as ImageBitmap).close === "function") {
      (entry.display as ImageBitmap).close();
    }
  } catch (_) {
    /* already closed */
  }
  if (entry.display && entry.display !== entry.raw) {
    releasePooledCanvas(entry.display as HTMLCanvasElement);
  }
  entry.display = null;
}
export async function cacheDisplay(entry: Pick<ThumbEntry, "display">): Promise<MaybeCanvas> {
  const off = entry.display;
  if (!off || typeof createImageBitmap !== "function") return off;
  try {
    const bitmap = await createImageBitmap(off as ImageBitmap);
    if (isSharedScratch(off as HTMLCanvasElement)) releaseScratch(off as HTMLCanvasElement);
    else releasePooledCanvas(off as HTMLCanvasElement);
    return bitmap;
  } catch (_) {
    return off;
  }
}
export function thumbSource(entry: ThumbEntry | null | undefined): MaybeCanvas {
  if (!entry) return null;
  if (entry.display && (entry.display as ImageBitmap).width > 0) return entry.display;
  if (entry.raw && (entry.raw as ImageBitmap).width > 0) return entry.raw;
  return null;
}
function rasterWidth(src: MaybeCanvas): number {
  return src ? ((src as ImageBitmap).width || 0) : 0;
}
export function thumbRaw(entry: ThumbEntry | null | undefined): MaybeCanvas {
  // Never fall back to `display`: that raster is already themed, and baking
  // it again double-applies invert/blend.
  if (!entry) return null;
  if (entry.raw && rasterWidth(entry.raw) > 0) return entry.raw;
  return null;
}
async function snapshotRaster(src: HTMLCanvasElement): Promise<MaybeCanvas> {
  if (typeof createImageBitmap === "function") {
    try {
      return await createImageBitmap(src);
    } catch (_) {
      /* fall through */
    }
  }
  const clone = acquirePooledCanvas(src.width, src.height);
  blitInto(clone, src);
  return clone;
}
export async function ensureEntryCurrent(entry: ThumbEntry): Promise<MaybeCanvas> {
  if (session.themeScrubActive) {
    return rasterWidth(entry.display) > 0 ? entry.display : null;
  }
  if (entry.gen === pipelineCache.gen && rasterWidth(entry.display) > 0) {
    return entry.display;
  }
  if (entry.pending) return await entry.pending;
  entry.pending = (async () => {
    const raw = thumbRaw(entry);
    if (!raw) return null;
    const pipeline = readPipeline();
    const { canvas: src, borrowed } = rasterToCanvas(
      raw as HTMLCanvasElement | ImageBitmap,
    );
    let work = src;
    let owned = borrowed;
    if (!borrowed) {
      work = acquirePooledCanvas(src.width, src.height);
      blitInto(work, src);
      owned = true;
    }
    const baked = await bakeRaster(work, pipeline);
    let newDisplay: MaybeCanvas;
    if (baked === work) {
      newDisplay = await snapshotRaster(work);
      if (owned) releasePooledCanvas(work);
    } else {
      if (owned) releasePooledCanvas(work);
      newDisplay = await cacheDisplay({ display: baked });
    }
    if (entry.display && entry.display !== entry.raw && entry.display !== newDisplay) {
      releaseDisplayOnly(entry);
    }
    entry.display = newDisplay;
    entry.gen = pipelineCache.gen;
    return entry.display;
  })();
  const result = await entry.pending;
  entry.pending = null;
  return result;
}
export function paintAllVisibleThumbs(): void {
  const seen = new Set<string>();
  for (const [canvasId, { page }] of session.thumbLive) {
    seen.add(canvasId);
    const entry = session.thumbCache.get(page);
    const live = el(canvasId) as HTMLCanvasElement | null;
    if (entry && live) paintCached(live, entry);
  }
  try {
    const nodes = document.querySelectorAll("canvas.thumb-canvas");
    for (let i = 0; i < nodes.length; i += 1) {
      const live = nodes[i] as HTMLCanvasElement;
      if (!live.id || seen.has(live.id)) continue;
      const m = /^thumb-(\d+)$/.exec(live.id);
      if (!m || !m[1]) continue;
      const page = parseInt(m[1], 10);
      const entry = session.thumbCache.get(page);
      if (!entry) continue;
      paintCached(live, entry);
      session.thumbLive.set(live.id, { page });
    }
  } catch (_) {
    /* no document */
  }
}
export function paintCached(
  dst: HTMLCanvasElement | null,
  entry: ThumbEntry | null
): { width: number; height: number } | null {
  const raw = session.themeScrubActive ? thumbRaw(entry) : null;
  const src = raw ?? thumbSource(entry);
  if (!dst || !src) return null;
  // Raster + tag are swapped by one synchronous primitive. Missing cache
  // data deliberately leaves the previous canvas and its matching tag alone.
  const shown = raw
    ? showRaw(dst, raw, "thumb-raw")
    : showBaked(dst, src, "thumb-raw");
  if (!shown) return null;
  return { width: entry!.cssW, height: entry!.cssH };
}
