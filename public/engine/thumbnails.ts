// LRU thumbnail cache + blit / render.

import type { MaybeCanvas, ThumbEntry, ThumbResult } from "./types";
import { offscreenFor, releaseCanvas, sessionEl } from "./canvas";
import { fail, failFrom } from "./errors";
import { bakeRaster } from "./theme/bake";
import { currentGen, readPipeline } from "./theme/pipeline";
import { cacheDisplay, ensureEntryCurrent, paintCached } from "./theme/thumbnails";
import { lifecycleEvent, THUMB_CACHE_MAX, worldEndedSignal } from "./state";
import type { EngineSession } from "./state";
// Cold sidebar: limit pdf.js work, not clicks.
const THUMB_RENDER_LIMIT = 3;

// The lane's state lives on the session.

// Tear the session's lane down: bump the epoch, drain the queue.
export function resetThumbLane(s: EngineSession): void {
  const lane = s.thumbLane;
  lane.epoch += 1;
  lane.open = false;
  lane.generation.clear();
  lane.prefetchInFlight.clear();
  const waiters = lane.epochWaiters.splice(0);
  for (const wake of waiters) wake();
  pumpThumbQueue(s);
}

// Stop this session's prefetches; live renders are untouched.
export function suspendPrefetches(s: EngineSession): void {
  const lane = s.thumbLane;
  lane.suspended = true;
  lane.era += 1;
  const waiters = lane.eraWaiters.splice(0);
  for (const wake of waiters) wake();
  pumpThumbQueue(s);
}

export function resumePrefetches(s: EngineSession): void {
  s.thumbLane.suspended = false;
}

type WorldEnded = { promise: Promise<void>; unsubscribe: () => void };

/// The session's era moved (a suspend) — the world a prefetch was born in.
function eraMovedSignal(s: EngineSession, era: number): WorldEnded {
  const lane = s.thumbLane;
  return worldEndedSignal(lane.eraWaiters, era !== lane.era);
}

/// The session's lane epoch moved (a teardown or document swap).
function epochMovedSignal(s: EngineSession, epoch: number): WorldEnded {
  const lane = s.thumbLane;
  return worldEndedSignal(lane.epochWaiters, epoch !== lane.epoch);
}

export function thumbLaneGauge(s: EngineSession): { thumbQueue: number; thumbActive: number } {
  return { thumbQueue: s.thumbLane.queue.length, thumbActive: s.thumbLane.active };
}

// The generation map's size for stats.
export function thumbGenerationSize(s: EngineSession): number {
  return s.thumbLane.generation.size;
}

/** A document's lane is open again: generation bookkeeping records anew. */
export function beginThumbLane(s: EngineSession): void {
  s.thumbLane.open = true;
}

function nextThumbGeneration(s: EngineSession, canvasId: string): number {
  const lane = s.thumbLane;
  if (!lane.open) return 0;
  const next = (lane.generation.get(canvasId) ?? 0) + 1;
  lane.generation.set(canvasId, next);
  return next;
}

function pumpThumbQueue(s: EngineSession): void {
  const lane = s.thumbLane;
  while (lane.active < THUMB_RENDER_LIMIT && lane.queue.length > 0) {
    const next = lane.queue.shift();
    if (!next) return;
    lane.active += 1;
    next();
  }
}
// Insert a thumbnail entry, releasing the previous and the LRU.
function cachePut(s: EngineSession, page: number, entry: ThumbEntry): void {
  if (s.thumbCache.has(page)) {
    const prev = s.thumbCache.get(page);
    s.thumbCache.delete(page);
    if (prev && prev !== entry) s.releaseThumbEntry(prev);
  }
  s.thumbCache.set(page, entry);
  while (s.thumbCache.size > THUMB_CACHE_MAX) {
    const oldest = s.thumbCache.keys().next();
    if (oldest.done || oldest.value === undefined) break;
    const oldEntry = s.thumbCache.get(oldest.value);
    s.thumbCache.delete(oldest.value);
    if (oldEntry && oldEntry !== entry) s.releaseThumbEntry(oldEntry);
  }
}

export function hasThumb(s: EngineSession, page: number, scale: number): boolean {
  const hit = s.thumbCache.get(page);
  if (!hit || Math.abs(hit.scale - scale) >= 1e-9) return false;
  // Under a scrub the raw IS the picture; otherwise only current answers.
  return s.themeScrubActive || hit.gen === currentGen(s);
}

// The canvas `canvasId` names FOR THIS SESSION.
function targetCanvas(s: EngineSession, canvasId: string): HTMLCanvasElement | null {
  const pageState = s.stateByCanvasId.get(canvasId);
  if (pageState && pageState.pinned) return pageState.dead ? null : pageState.canvas;
  return sessionEl(s.sid, canvasId) as HTMLCanvasElement | null;
}

export async function renderThumb(
  s: EngineSession,
  canvasId: string,
  page: number,
  scale: number
): Promise<ThumbResult> {
  // A new mount supersedes any queued request for the same recycled canvas id.
  const generation = nextThumbGeneration(s, canvasId);
  s.thumbCancelled.delete(canvasId);

  // Cache hits are blits; they never wait behind cold renders.
  if (hasThumb(s, page, scale)) {
    try {
      return await renderThumbInternal(s, canvasId, page, scale);
    } catch (e) {
      return failFrom(e);
    }
  }

  return await new Promise<ThumbResult>((resolve) => {
    const lane = s.thumbLane;
    const epoch = lane.epoch;
    lane.queue.push(() => {
      const finish = () => {
        lane.active -= 1;
        pumpThumbQueue(s);
      };
      // The cell went, the id was reused, or the document died: drop it.
      if (
        epoch !== lane.epoch
        || s.thumbCancelled.has(canvasId)
        || lane.generation.get(canvasId) !== generation
      ) {
        resolve(fail("cancelled", "Thumbnail render cancelled"));
        finish();
        return;
      }
      renderThumbInternal(s, canvasId, page, scale)
        .then(resolve)
        .catch((e: unknown) => {
          resolve(failFrom(e));
        })
        .finally(finish);
    });
    pumpThumbQueue(s);
  });
}

async function renderThumbInternal(
  s: EngineSession,
  canvasId: string,
  page: number,
  scale: number
): Promise<ThumbResult> {
  const canvas = targetCanvas(s, canvasId);
  if (!canvas) return fail("no_canvas", "No canvas: " + canvasId);
  if (!s.pdf) return fail("no_document", "No document open");

  const hit = s.thumbCache.get(page);
  if (hit && Math.abs(hit.scale - scale) < 1e-9) {
    // Whether the entry may paint, or must re-bake from its raw.
    const current = s.themeScrubActive || hit.gen === currentGen(s);
    if (current || (await ensureEntryCurrent(s, hit))) {
      const size = paintCached(s, canvas, hit);
      if (size) {
        cachePut(s, page, hit);
        s.thumbLive.set(canvasId, { page });
        return { ok: true, width: size.width, height: size.height, scale };
      }
    }
  }

  try { const t = s.thumbTasks.get(canvasId); if (t) t.cancel(); } catch (_) { /* ignore */ }
  s.thumbTasks.delete(canvasId);
  s.thumbCancelled.delete(canvasId);

  try {
    const pg = await s.pdf.getPage(page);
    const viewport = pg.getViewport({ scale });
    const cssW = Math.floor(viewport.width);
    const cssH = Math.floor(viewport.height);

    const made = offscreenFor(viewport);
    if (!made) return fail("no_context", "No 2d context");
    const { canvas: off, ctx } = made;
    const task = pg.render({ canvasContext: ctx, viewport });
    s.thumbTasks.set(canvasId, task);
    try {
      await task.promise;
    } catch (e) {
      s.thumbTasks.delete(canvasId);
      releaseCanvas(off);
      try { pg.cleanup(); } catch (_) { /* ignore */ }
      if ((e as { name?: string }).name === "RenderingCancelledException") {
        return fail("cancelled", "Thumb render cancelled");
      }
      return failFrom(e);
    }
    s.thumbTasks.delete(canvasId);
    pg.cleanup();

    // Keep `off` as the unbaked raw for later rebakes.
    const raw = off;
    const pipeline = s.themeScrubActive ? null : readPipeline(s);
    let display: MaybeCanvas = pipeline ? await bakeRaster(raw, pipeline) : raw;
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
      gen: pipeline ? pipeline.gen : -1,
      pending: null,
    };
    cachePut(s, page, entry);

    if (!s.thumbCancelled.has(canvasId)) {
      const live = targetCanvas(s, canvasId);
      if (live) {
        s.thumbLive.set(canvasId, { page });
        paintCached(s, live, entry);
      }
    }
    s.thumbCancelled.delete(canvasId);

    return { ok: true, width: cssW, height: cssH, scale };
  } catch (e) {
    s.thumbTasks.delete(canvasId);
    return failFrom(e);
  }
}

export function cancelThumb(s: EngineSession, canvasId: string): void {
  // Invalidate a queued job as well as cancelling an active pdf.js task.
  nextThumbGeneration(s, canvasId);
  const task = s.thumbTasks.get(canvasId);
  if (task) {
    try { task.cancel(); } catch (_) { /* ignore */ }
    s.thumbTasks.delete(canvasId);
  }
  s.thumbCancelled.add(canvasId);
  s.thumbLive.delete(canvasId);
  releaseCanvas(targetCanvas(s, canvasId));
}

// Render a page into the cache with no DOM canvas (idle prefetch).
export async function prefetchThumb(s: EngineSession, page: number, scale: number): Promise<void> {
  if (!s.pdf) return;
  const hit = s.thumbCache.get(page);
  if (hit && Math.abs(hit.scale - scale) < 1e-9) return;
  const lane = s.thumbLane;
  if (lane.prefetchInFlight.has(page)) return;
  if (lane.suspended) return;
  const epoch = lane.epoch;
  const era = lane.era;
  lane.prefetchInFlight.add(page);
  s.prefetchesStarted += 1;
  s.prefetchesActive += 1;
  lifecycleEvent("thumb_prefetch:start");
  try {
    await new Promise<void>((resolve) => {
      lane.queue.push(() => {
        const finish = () => {
          lane.active -= 1;
          pumpThumbQueue(s);
        };
        // The document was torn down while this prefetch waited: drop it.
        if (epoch !== lane.epoch || era !== lane.era || !s.pdf || s.disposed) {
          s.prefetchesDropped += 1;
          lifecycleEvent("thumb_prefetch:drop");
          resolve();
          finish();
          return;
        }
        prefetchThumbInternal(s, page, scale, epoch, era)
          .then((landed) => {
            if (landed) {
              s.prefetchesCompleted += 1;
              lifecycleEvent("thumb_prefetch:complete");
            } else {
              s.prefetchesDropped += 1;
              lifecycleEvent("thumb_prefetch:drop");
            }
          })
          .catch(() => {
            s.prefetchesDropped += 1;
            lifecycleEvent("thumb_prefetch:drop");
          })
          .finally(finish)
          .finally(resolve);
      });
      pumpThumbQueue(s);
    });
  } finally {
    lane.prefetchInFlight.delete(page);
    s.prefetchesActive -= 1;
  }
}

/// A cancellable signal for "this prefetch's world ended".
function prefetchWorldEnded(s: EngineSession, epoch: number, era: number): {
  promise: Promise<void>;
  unsubscribe: () => void;
} {
  const epochSignal = epochMovedSignal(s, epoch);
  const eraSignal = eraMovedSignal(s, era);
  const goneSignal = s.documentGoneSignal();
  const promise = Promise.race([epochSignal.promise, eraSignal.promise, goneSignal.promise]);
  let done = false;
  return {
    promise,
    unsubscribe: () => {
      if (done) return;
      done = true;
      epochSignal.unsubscribe();
      eraSignal.unsubscribe();
      goneSignal.unsubscribe();
    },
  };
}

// The lane-slot half of a prefetch: render, bake, cache.
async function prefetchThumbInternal(
  s: EngineSession,
  page: number,
  scale: number,
  epoch: number,
  era: number
): Promise<boolean> {
  const taskId = `prefetch-${page}`;
  const stale = () =>
    epoch !== s.thumbLane.epoch || era !== s.thumbLane.era || !s.pdf || s.disposed;
  const dying = prefetchWorldEnded(s, epoch, era);
  try {
    const pg = await Promise.race([
      s.pdf!.getPage(page),
      dying.promise.then(() => null),
    ]);
    if (!pg || stale()) {
      try { pg?.cleanup(); } catch (_) { /* ignore */ }
      return false;
    }
    const viewport = pg.getViewport({ scale });
    const made = offscreenFor(viewport);
    if (!made) {
      try { pg.cleanup(); } catch (_) { /* ignore */ }
      return false;
    }
    const { canvas: off, ctx } = made;
    const task = pg.render({ canvasContext: ctx, viewport });
    s.thumbTasks.set(taskId, task);
    const rendering = prefetchWorldEnded(s, epoch, era);
    let rendered = true;
    let epochMoved = false;
    try {
      await Promise.race([task.promise, rendering.promise.then(() => {
        epochMoved = true;
      })]);
    } catch (_) {
      rendered = false; // cancelled by teardown, or a failed raster — best-effort either way
    } finally {
      // The usual path: remove this prefetch's waiters.
      rendering.unsubscribe();
    }
    s.thumbTasks.delete(taskId);
    if (epochMoved) {
      // The teardown sweep never saw this task: cancel it here.
      try { task.cancel(); } catch (_) { /* ignore */ }
    }
    if (!rendered || epochMoved || stale()) {
      releaseCanvas(off);
      try { pg.cleanup(); } catch (_) { /* ignore */ }
      return false;
    }
    pg.cleanup();
    const raw = off;
    const pipeline = s.themeScrubActive ? null : readPipeline(s);
    let display: MaybeCanvas = pipeline ? await bakeRaster(raw, pipeline) : raw;
    if (display !== raw) display = await cacheDisplay({ display });
    // The epoch check again: a bake can wait on the theme queue.
    if (stale()) {
      releaseCanvas(off);
      return false;
    }
    cachePut(s, page, { raw, display, cssW: Math.floor(viewport.width),
                     cssH: Math.floor(viewport.height), scale,
                     gen: pipeline ? pipeline.gen : -1, pending: null });
    return true;
  } catch (_) {
    s.thumbTasks.delete(taskId);
    return false;
  } finally {
    dying.unsubscribe();
  }
}
