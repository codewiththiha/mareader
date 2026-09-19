// Byte-weighted LRU previews. Cold sidebar renders and idle prewarm use the
// same resource lane as full pages; prewarm can never race a foreground burst.
import type { MaybeCanvas, PDFPageProxy, RenderTask, ThumbEntry, ThumbResult } from "./types";
import { el, releaseCanvas, showBaked, showRaw } from "./canvas";
import { fail, failFrom } from "./errors";
import { bakeRaster } from "./theme/bake";
import { readPipeline, pipelineCache } from "./theme/pipeline";
import { cacheDisplay, ensureEntryCurrent, paintCached, thumbRaw, thumbSource } from "./theme/thumbnails";
import { THUMB_CACHE_MAX, session } from "./state";
import { rasterSize, type RasterTicket } from "./raster-scheduler";
import { mayCopyRaster, mayPrefetch, motion, RASTER_BYTES_PER_PIXEL, rasterScheduler } from "./raster-resources";

export const THUMB_MAX_BYTES = 16 * 1024 * 1024;
const THUMB_MAX_PIXELS = 256 * 1024;
let epoch = 0;
const keys = new Map<string, Promise<ThumbResult>>();

export function resetThumbLane(): void {
  epoch++;
  for (const key of keys.keys()) rasterScheduler.cancel(key);
  keys.clear();
}

export function thumbResidentBytes(): number {
  const surfaces = new Set<NonNullable<MaybeCanvas>>();
  for (const entry of session.thumbCache.values()) {
    if (entry.raw) surfaces.add(entry.raw);
    if (entry.display) surfaces.add(entry.display);
  }
  let bytes = 0;
  for (const surface of surfaces) bytes += surface.width * surface.height * 4;
  return bytes;
}

function cachePut(page: number, entry: ThumbEntry): void {
  const previous = session.thumbCache.get(page);
  session.thumbCache.delete(page);
  if (previous && previous !== entry) session.releaseThumbEntry(previous);
  session.thumbCache.set(page, entry);
  for (const [oldPage, oldEntry] of session.thumbCache) {
    if (session.thumbCache.size <= THUMB_CACHE_MAX && thumbResidentBytes() <= THUMB_MAX_BYTES) break;
    if (oldEntry.pending) continue;
    session.thumbCache.delete(oldPage);
    session.releaseThumbEntry(oldEntry);
  }
}

export function hasThumb(page: number, scale: number): boolean {
  const hit = session.thumbCache.get(page);
  return !!hit && Math.abs(hit.scale - scale) < 1e-9 && !!thumbSource(hit)
    && (session.themeScrubActive || hit.gen === pipelineCache.gen || !!hit.raw);
}

export function blitThumb(canvasId: string, page: number): boolean {
  const dst = el(canvasId) as HTMLCanvasElement | null;
  const entry = session.thumbCache.get(page);
  if (!dst || !entry) return false;
  const raw = session.themeScrubActive ? thumbRaw(entry) : null;
  const src = raw ?? thumbSource(entry);
  if (!src || !mayCopyRaster(src.width * src.height * 4)) return false;
  return raw ? showRaw(dst, raw, "thumb-raw") : showBaked(dst, src, "thumb-raw");
}

function requestThumb(canvasId: string | null, page: number, scale: number): Promise<ThumbResult> {
  const doc = session.pdf;
  if (!doc) return Promise.resolve(fail("no_document", "No document open"));
  if (!Number.isFinite(scale) || scale <= 0) return Promise.resolve(fail("invalid_scale", "Invalid thumbnail scale"));
  const key = canvasId ? `thumb:${canvasId}` : `prefetch:${page}`;
  const atEpoch = epoch;
  let task: RenderTask | undefined;
  const request = rasterScheduler.request<ThumbResult>({
    key, bytes: THUMB_MAX_PIXELS * RASTER_BYTES_PER_PIXEL,
    priority: () => {
      if (epoch !== atEpoch || session.pdf !== doc) return null;
      if (!canvasId && motion.phase !== "Idle") return null;
      return canvasId ? 200 : 50;
    },
    cancel: () => { try { task?.cancel(); } catch (_) { /* already done */ } },
    cancelled: () => fail("cancelled", "Thumbnail cancelled"), failed: failFrom,
    run: async (ticket: RasterTicket): Promise<ThumbResult> => {
      const current = () => ticket.current() && epoch === atEpoch && session.pdf === doc;
      let pg: PDFPageProxy | undefined;
      let off: HTMLCanvasElement | undefined;
      let display: MaybeCanvas = null;
      let retained = false;
      try {
        if (!current()) return fail("cancelled", "Thumbnail cancelled");
        const hit = session.thumbCache.get(page);
        if (hit && Math.abs(hit.scale - scale) < 1e-9) {
          if (!session.themeScrubActive && hit.gen !== pipelineCache.gen) await ensureEntryCurrent(hit, true);
          if (!current()) return fail("cancelled", "Thumbnail cancelled");
          if (thumbSource(hit)) {
            cachePut(page, hit);
            if (canvasId) {
              paintCached(el(canvasId) as HTMLCanvasElement | null, hit, true);
              session.thumbLive.set(canvasId, { page });
            }
            return { ok: true, width: hit.cssW, height: hit.cssH, scale };
          }
        }
        pg = await doc.getPage(page);
        if (!current()) return fail("cancelled", "Thumbnail cancelled");
        const viewport = pg.getViewport({ scale });
        const size = rasterSize(viewport.width, viewport.height, 1, THUMB_MAX_PIXELS);
        off = document.createElement("canvas");
        off.width = size.width;
        off.height = size.height;
        const ctx = off.getContext("2d", { alpha: false });
        if (!ctx) return fail("no_context", "No thumbnail context");
        task = pg.render({ canvasContext: ctx, viewport,
          transform: [size.width / viewport.width, 0, 0, size.height / viewport.height, 0, 0] });
        if (canvasId) session.thumbTasks.set(canvasId, task);
        await task.promise;
        if (!current()) return fail("cancelled", "Thumbnail cancelled");
        const pipeline = readPipeline();
        const scrub = session.themeScrubActive;
        display = scrub ? off : await bakeRaster(off, pipeline);
        if (display !== off) display = await cacheDisplay({ display });
        if (!current()) return fail("cancelled", "Thumbnail cancelled");
        const entry: ThumbEntry = { raw: off, display,
          cssW: Math.floor(viewport.width), cssH: Math.floor(viewport.height), scale,
          gen: scrub ? -1 : pipeline.gen, pending: null };
        cachePut(page, entry);
        retained = true;
        if (canvasId) {
          session.thumbLive.set(canvasId, { page });
          paintCached(el(canvasId) as HTMLCanvasElement | null, entry, true);
        }
        return { ok: true, width: entry.cssW, height: entry.cssH, scale };
      } finally {
        if (canvasId && session.thumbTasks.get(canvasId) === task) session.thumbTasks.delete(canvasId);
        if (!retained) {
          session.releaseThumbEntry({ raw: off ?? null, display, cssW: 0, cssH: 0, scale, gen: 0, pending: null });
        }
        try { await pg?.cleanup(); } catch (_) { /* advisory */ }
      }
    },
  });
  // keys are bounded by live requests, not by the document's page count.
  keys.set(key, request);
  void request.finally(() => { if (keys.get(key) === request) keys.delete(key); });
  return request;
}

export function renderThumb(canvasId: string, page: number, scale: number): Promise<ThumbResult> {
  session.thumbCancelled.delete(canvasId);
  const hit = session.thumbCache.get(page);
  if (hit && hasThumb(page, scale) && (session.themeScrubActive || hit.gen === pipelineCache.gen)) {
    rasterScheduler.cancel(`thumb:${canvasId}`);
    if (mayCopyRaster((thumbSource(hit)?.width ?? 0) * (thumbSource(hit)?.height ?? 0) * 4)) {
      const size = paintCached(el(canvasId) as HTMLCanvasElement | null, hit);
      if (size) {
        cachePut(page, hit);
        session.thumbLive.set(canvasId, { page });
        return Promise.resolve({ ok: true, ...size, scale });
      }
    }
  }
  return requestThumb(canvasId, page, scale);
}

export function cancelThumb(canvasId: string): void {
  rasterScheduler.cancel(`thumb:${canvasId}`);
  session.thumbLive.delete(canvasId);
  releaseCanvas(el(canvasId) as HTMLCanvasElement | null);
}

export async function prefetchThumb(page: number, scale: number): Promise<void> {
  if (!mayPrefetch() || hasThumb(page, scale)) return;
  await requestThumb(null, page, scale);
}
