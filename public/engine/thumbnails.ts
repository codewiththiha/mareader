// LRU thumbnail cache + blit / render.

import type { MaybeCanvas, ThumbEntry, ThumbResult } from "./types";
import { offscreenFor, releaseCanvas, sessionEl, showBaked, showRaw } from "./canvas";
import { fail, failFrom } from "./errors";
import { bakeRaster } from "./theme/bake";
import { readPipeline, pipelineCache } from "./theme/pipeline";
import {
  cacheDisplay,
  ensureEntryCurrent,
  paintCached,
  thumbRaw,
  thumbSource,
} from "./theme/thumbnails";
import { lifecycleEvent, THUMB_CACHE_MAX } from "./state";
import type { EngineSession } from "./state";
// A cold sidebar can mount a full thumbnail window at once. Limit pdf.js
// raster work, not clicks: queued jobs are invalidated on unmount and cached
// paths still paint immediately.
const THUMB_RENDER_LIMIT = 3;

// The lane's state lives on the session (`EngineSession.thumbLane`): one
// pane's teardown bumps only ITS epoch, one pane's suspend only ITS era.

/** Tear the session's lane down: bump the epoch (every queued job and every
 *  parked prefetch await sees it), close generation bookkeeping, and pump
 *  so the queue drains through its own guards. */
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

/** Stop this session's speculative prefetches (its pane is suspended):
 *  queued prefetches drop, in-flight ones race the era signal. Live
 *  thumbnail renders are untouched. */
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

function eraMovedSignal(s: EngineSession, era: number): {
  promise: Promise<void>;
  unsubscribe: () => void;
} {
  const lane = s.thumbLane;
  if (era !== lane.era) {
    return { promise: Promise.resolve(), unsubscribe: () => {} };
  }
  let resolve!: () => void;
  const promise = new Promise<void>((r) => {
    resolve = r;
  });
  const waiter = () => resolve();
  lane.eraWaiters.push(waiter);
  return {
    promise,
    unsubscribe: () => {
      const at = lane.eraWaiters.indexOf(waiter);
      if (at >= 0) lane.eraWaiters.splice(at, 1);
    },
  };
}

function epochMovedSignal(s: EngineSession, epoch: number): {
  promise: Promise<void>;
  unsubscribe: () => void;
} {
  const lane = s.thumbLane;
  if (epoch !== lane.epoch) {
    return { promise: Promise.resolve(), unsubscribe: () => {} };
  }
  let resolve!: () => void;
  const promise = new Promise<void>((r) => {
    resolve = r;
  });
  const waiter = () => resolve();
  lane.epochWaiters.push(waiter);
  return {
    promise,
    unsubscribe: () => {
      const at = lane.epochWaiters.indexOf(waiter);
      if (at >= 0) lane.epochWaiters.splice(at, 1);
    },
  };
}

export function thumbLaneGauge(s: EngineSession): { thumbQueue: number; thumbActive: number } {
  return { thumbQueue: s.thumbLane.queue.length, thumbActive: s.thumbLane.active };
}

/** The generation map's size for the stats surface: per-canvas bookkeeping
 *  the lane keeps until document teardown. The baseline measures it so a
 *  long scrolling session cannot grow it unseen. */
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
/** Insert a thumbnail entry into the cache, releasing any previous entry for
 *  the page and evicting the LRU entry if the cache is full. */
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
  return (
    !!hit &&
    Math.abs(hit.scale - scale) < 1e-9 &&
    (s.themeScrubActive || hit.gen === pipelineCache.gen || !!hit.raw)
  );
}

/** The canvas `canvasId` names FOR THIS SESSION: a registered page's own
 *  (pinned) canvas when the id is a page's — the blurry first paint lands
 *  on page canvases too — otherwise the element with that id this session
 *  owns. Never another pane's twin. */
function targetCanvas(s: EngineSession, canvasId: string): HTMLCanvasElement | null {
  const pageState = s.stateByCanvasId.get(canvasId);
  if (pageState && pageState.pinned) return pageState.dead ? null : pageState.canvas;
  return sessionEl(s.sid, canvasId) as HTMLCanvasElement | null;
}

export function blitThumb(s: EngineSession, canvasId: string, page: number): boolean {
  const dst = targetCanvas(s, canvasId);
  const entry = s.thumbCache.get(page);
  if (!dst || !entry) return false;
  const raw = s.themeScrubActive ? thumbRaw(entry) : null;
  const src = raw ?? thumbSource(entry);
  if (!src) return false;
  return raw
    ? showRaw(dst, raw, "thumb-raw")
    : showBaked(dst, src, "thumb-raw");
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

  // Cache hits are synchronous blits or a small display refresh; they do not
  // create pdf.js raster work and should never wait behind cold renders.
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
      // The cell disappeared, a newer mount re-used this id, or the document
      // was torn down while the job waited, before the job reached the front
      // of the queue. Drop it without touching pdf.js.
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
    if (s.themeScrubActive) {
      if (showRaw(canvas, thumbRaw(hit), "thumb-raw")) {
        cachePut(s, page, hit);
        s.thumbLive.set(canvasId, { page });
        return { ok: true, width: hit.cssW, height: hit.cssH, scale };
      }
    } else if (hit.gen === pipelineCache.gen) {
      const size = paintCached(s, canvas, hit);
      if (size) {
        cachePut(s, page, hit);
        s.thumbLive.set(canvasId, { page });
        return { ok: true, width: size.width, height: size.height, scale };
      }
    } else if (await ensureEntryCurrent(s, hit)) {
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
    const out = 1;
    const cssW = Math.floor(viewport.width);
    const cssH = Math.floor(viewport.height);

    const made = offscreenFor(viewport, out);
    if (!made) return fail("no_context", "No 2d context");
    const { canvas: off, ctx } = made;
    const transform = out !== 1 ? [out, 0, 0, out, 0, 0] : null;

    const task = pg.render({ canvasContext: ctx, viewport, transform });
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

    // Keep `off` as the unbaked raw for every later theme rebake. Never
    // alias raw === display and never release `off` here: cacheDisplay /
    // createImageBitmap used to zero the only unthemed copy, so a theme
    // change could not update visible thumbs until a full pdf.js re-render.
    const raw = off;
    let display: MaybeCanvas = s.themeScrubActive ? raw : await bakeRaster(raw, readPipeline());
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
      gen: s.themeScrubActive ? -1 : pipelineCache.gen,
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

/** Render a page into the cache with no DOM canvas (idle prefetch). A
 *  cache-warm cell asks `hasThumb` while it is still being built, mounts
 *  already loaded, and its first render call is a synchronous blit — zero
 *  skeleton, zero waiting. Rendering the pages AROUND the reader while idle
 *  means every remount after a fling to page N answers that probe true.
 *
 *  Prefetch is FIRST-CLASS lane work, not a side channel: it waits in the
 *  same bounded queue as cell renders, its pdf.js task is registered under
 *  a `prefetch-<page>` id so a document teardown cancels it like any
 *  other, and the lane epoch is re-checked after every await — a prefetch
 *  started for one document can never land in the next one's cache. The
 *  lifecycle is visible in `stats()` (activePrefetches plus the
 *  started/completed/dropped trio), so the reader's disposal baseline can
 *  prove no prefetch work outlived the document. */
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
        // The document was torn down (or replaced) while this prefetch
        // waited for a lane slot. Drop it without touching pdf.js — the
        // same guard a queued cell render gets.
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

/// A combined cancellable signal for "this prefetch's world ended": the
/// lane epoch moved (teardown or swap ran before this await started), or
/// the document's destroy began while the await was in flight. The
/// document-gone half covers the destroy window the epoch alone cannot
/// see: a prefetch enqueued into the NEW epoch, onto a worker whose death
/// is already underway, awaiting a promise it will never see settle.
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

/** The lane-slot half of a prefetch: render offscreen, bake, cache. Resolves
 *  `true` only when the entry landed in THIS document's cache; every stale
 *  or failed path cleans up after itself and resolves `false`. The pdf.js
 *  task rides `s.thumbTasks` under a synthetic id, so `destroySession`'s
 *  cancel-everything sweep reaches it and `stats().thumbTasks` counts it. */
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
      // The usual path is a normal settle: remove this prefetch's waiters,
      // or each successful prefetch would leak two resolvers into the
      // cancellation arrays for the rest of the document's lifetime.
      rendering.unsubscribe();
    }
    s.thumbTasks.delete(taskId);
    if (epochMoved) {
      // The teardown sweep had already run when this task was created, so
      // the cancel-everything pass never reached it — cancel it here, or it
      // would render into a canvas this prefetch is about to release.
      try { task.cancel(); } catch (_) { /* ignore */ }
    }
    if (!rendered || epochMoved || stale()) {
      releaseCanvas(off);
      try { pg.cleanup(); } catch (_) { /* ignore */ }
      return false;
    }
    pg.cleanup();
    const raw = off;
    let display: MaybeCanvas = s.themeScrubActive ? raw : await bakeRaster(raw, readPipeline());
    if (display !== raw) display = await cacheDisplay({ display });
    // The epoch check AGAIN: a bake can wait on the theme queue, and a
    // document swap in that window must not file this book's colours into
    // the next document's cache.
    if (stale()) {
      releaseCanvas(off);
      return false;
    }
    cachePut(s, page, { raw, display, cssW: Math.floor(viewport.width),
                     cssH: Math.floor(viewport.height), scale,
                     gen: s.themeScrubActive ? -1 : pipelineCache.gen, pending: null });
    return true;
  } catch (_) {
    s.thumbTasks.delete(taskId);
    return false;
  } finally {
    dying.unsubscribe();
  }
}
