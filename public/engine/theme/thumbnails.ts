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
import { mayCopyRaster, motion, RASTER_BYTES_PER_PIXEL, rasterScheduler } from "../raster-resources";
import { session } from "../state";
import { bakeRaster, rasterToCanvas } from "./bake";
import { pipelineCache, readPipeline } from "./pipeline";

/** Release a thumbnail entry's display surface (ImageBitmap or pooled canvas),
 *  leaving the raw raster untouched. Called when an entry's themed display is
 *  stale and must be rebuilt from raw. */
function releaseDisplayOnly(entry: ThumbEntry | null): void {
  if (!entry) return;
  try {
    if (entry.display && entry.display !== entry.raw && typeof (entry.display as ImageBitmap).close === "function") {
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
  if (!off) return off;
  if (typeof createImageBitmap === "function") {
    try {
      const bitmap = await createImageBitmap(off as ImageBitmap);
      if (isSharedScratch(off as HTMLCanvasElement)) releaseScratch(off as HTMLCanvasElement);
      else releasePooledCanvas(off as HTMLCanvasElement);
      return bitmap;
    } catch (_) { /* canvas fallback */ }
  }
  // A cache entry must never own the shared scratch. The next bake would
  // overwrite it (and its reservation has already ended).
  if (isSharedScratch(off as HTMLCanvasElement)) {
    const copy = acquirePooledCanvas(off.width, off.height);
    blitInto(copy, off);
    releaseScratch(off as HTMLCanvasElement);
    return copy;
  }
  return off;
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
let entrySequence = 0;
const entryKeys = new WeakMap<ThumbEntry, string>();
export async function ensureEntryCurrent(entry: ThumbEntry, admitted = false): Promise<MaybeCanvas> {
  if (admitted) return updateEntry(entry);
  if (entry.gen === pipelineCache.gen && rasterWidth(entry.display) > 0) return entry.display;
  let key = entryKeys.get(entry);
  if (!key) { key = `thumb-bake:${++entrySequence}`; entryKeys.set(entry, key); }
  const raw = thumbRaw(entry);
  if (!raw) return null;
  const doc = session.pdf;
  return rasterScheduler.request<MaybeCanvas>({
    key, bytes: raw.width * raw.height * RASTER_BYTES_PER_PIXEL,
    priority: () => session.pdf !== doc || entry.raw !== raw ? null : motion.phase === "Idle" ? 150 : -Infinity,
    cancel: () => {}, cancelled: () => null, failed: () => null,
    run: async (ticket) => ticket.current() && entry.raw === raw ? updateEntry(entry) : null,
  });
}
async function updateEntry(entry: ThumbEntry): Promise<MaybeCanvas> {
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
    let baked: HTMLCanvasElement | undefined;
    let newDisplay: MaybeCanvas = null;
    let retained = false;
    try {
      baked = await bakeRaster(work, pipeline);
      newDisplay = baked === work ? await snapshotRaster(work) : await cacheDisplay({ display: baked });
      // cacheDisplay transferred/released the intermediate (or handed its
      // ownership to newDisplay). Never release that shared scratch twice.
      baked = undefined;
      if (entry.raw !== raw) return null;
      if (entry.display && entry.display !== entry.raw && entry.display !== newDisplay) releaseDisplayOnly(entry);
      entry.display = newDisplay;
      entry.gen = pipeline.gen;
      retained = true;
      return entry.display;
    } finally {
      if (owned) releasePooledCanvas(work);
      if (baked && baked !== work) {
        if (isSharedScratch(baked)) releaseScratch(baked);
        else releasePooledCanvas(baked);
      }
      if (!retained && newDisplay) session.releaseThumbEntry({ ...entry, raw: null, display: newDisplay });
    }
  })();
  try { return await entry.pending; } finally { entry.pending = null; }
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
  entry: ThumbEntry | null,
  admitted = false
): { width: number; height: number } | null {
  const raw = session.themeScrubActive ? thumbRaw(entry) : null;
  const src = raw ?? thumbSource(entry);
  if (!dst || !src || (!admitted && !mayCopyRaster(Math.max(0, src.width * src.height - dst.width * dst.height) * 4))) return null;
  // Raster + tag are swapped by one synchronous primitive. Missing cache
  // data deliberately leaves the previous canvas and its matching tag alone.
  const shown = raw
    ? showRaw(dst, raw, "thumb-raw")
    : showBaked(dst, src, "thumb-raw");
  if (!shown) return null;
  return { width: entry!.cssW, height: entry!.cssH };
}
