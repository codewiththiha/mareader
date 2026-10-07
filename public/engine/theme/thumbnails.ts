// Themed thumbnail display: the raw/display split per cache entry, the
// ImageBitmap display cache, and painting cached entries onto live DOM
// canvases.

import type { MaybeCanvas, ThumbEntry } from "../types";
import {
  acquirePooledCanvas,
  blitInto,
  isSharedScratch,
  releasePooledCanvas,
  ownedBy,
  releaseScratch,
  sessionEl,
  showBaked,
  showRaw,
} from "../canvas";
import type { EngineSession } from "../state";
import { bakeRaster, rasterToCanvas } from "./bake";
import { currentGen, readPipeline } from "./pipeline";

/** Release a thumbnail entry's display surface (ImageBitmap or pooled canvas),
 *  leaving the raw raster untouched. Called when an entry's themed display is
 *  stale and must be rebuilt from raw. */
function releaseDisplayOnly(entry: ThumbEntry | null): void {
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
/** One canvas the rail can see, and the page it shows. */
type ThumbTarget = { canvas: HTMLCanvasElement; page: number };

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
export async function ensureEntryCurrent(
  s: EngineSession,
  entry: ThumbEntry
): Promise<MaybeCanvas> {
  if (s.themeScrubActive) {
    return rasterWidth(entry.display) > 0 ? entry.display : null;
  }
  if (entry.gen === currentGen(s) && rasterWidth(entry.display) > 0) {
    return entry.display;
  }
  if (entry.pending) return await entry.pending;
  entry.pending = (async () => {
    const raw = thumbRaw(entry);
    if (!raw) return null;
    const pipeline = readPipeline(s);
    // The bake reads its source and writes its own surface, so the entry's raw
    // is baked FROM directly: an earlier baker filtered in place, and a copy
    // in front of it was what kept the raw intact. `rasterToCanvas` still
    // makes a canvas when the raw is an ImageBitmap, and that one is ours to
    // return to the pool.
    const { canvas: src, borrowed } = rasterToCanvas(
      raw as HTMLCanvasElement | ImageBitmap,
    );
    const baked = await bakeRaster(src, pipeline);
    // A filter-only bake hands back `src`: the display still needs a surface
    // of its own, because `src` belongs to the entry as its raw.
    const newDisplay =
      baked === src ? await snapshotRaster(src) : await cacheDisplay({ display: baked });
    if (borrowed) releasePooledCanvas(src);
    if (entry.display && entry.display !== entry.raw && entry.display !== newDisplay) {
      releaseDisplayOnly(entry);
    }
    entry.display = newDisplay;
    entry.gen = pipeline.gen;
    return entry.display;
  })();
  const result = await entry.pending;
  entry.pending = null;
  return result;
}
/** Every thumb canvas this session owes a paint to, with the page it shows:
 *  the lane's own registrations plus any `canvas.thumb-canvas` in the document
 *  this session owns. Enumerated once so the paint below and a theme re-bake
 *  (`rebakeTheme`) see exactly the same set — a card on screen never keeps the
 *  look before the change because a second walk of the DOM disagreed. */
function visibleThumbTargets(s: EngineSession): ThumbTarget[] {
  const targets: ThumbTarget[] = [];
  const seen = new Set<string>();
  for (const [canvasId, { page }] of s.thumbLive) {
    seen.add(canvasId);
    const live = sessionEl(s.sid, canvasId) as HTMLCanvasElement | null;
    if (live) targets.push({ canvas: live, page });
  }
  try {
    const nodes = document.querySelectorAll("canvas.thumb-canvas");
    for (let i = 0; i < nodes.length; i += 1) {
      const live = nodes[i] as HTMLCanvasElement;
      if (!live.id || seen.has(live.id)) continue;
      // Only THIS session's canvases: another pane's rail may share the
      // document, and its thumbnails are not this cache's to paint.
      if (!ownedBy(live, s.sid)) continue;
      const m = /^thumb-(\d+)$/.exec(live.id);
      if (!m || !m[1]) continue;
      targets.push({ canvas: live, page: parseInt(m[1], 10) });
    }
  } catch (_) {
    /* no document */
  }
  return targets;
}

/** The pages a visible thumb shows, deduplicated: what a look change must
 *  have baked before the rail may be painted, and nothing else. */
export function visibleThumbPages(s: EngineSession): number[] {
  const pages = new Set<number>();
  for (const { page } of visibleThumbTargets(s)) pages.add(page);
  return [...pages];
}

export function paintAllVisibleThumbs(s: EngineSession): void {
  for (const { canvas, page } of visibleThumbTargets(s)) {
    const entry = s.thumbCache.get(page);
    if (!entry) continue;
    paintCached(s, canvas, entry);
    s.thumbLive.set(canvas.id, { page });
  }
}
export function paintCached(
  s: EngineSession,
  dst: HTMLCanvasElement | null,
  entry: ThumbEntry | null
): { width: number; height: number } | null {
  const raw = s.themeScrubActive ? thumbRaw(entry) : null;
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

