// LRU thumbnail cache + blit / render.

import type { MaybeCanvas, ThumbEntry, ThumbResult } from "./types";
import { el, offscreenFor, releaseCanvas, showBaked, showRaw } from "./canvas";
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
import { lifecycleEvent, THUMB_CACHE_MAX, session } from "./state";
// A cold sidebar can mount a full thumbnail window at once. Limit pdf.js
// raster work, not clicks: queued jobs are invalidated on unmount and cached
// paths still paint immediately.
const THUMB_RENDER_LIMIT = 3;
let thumbActive = 0;
const thumbQueue: Array<() => void> = [];
/** Pages with a prefetch in flight. A prefetch is lane work like any other
 *  (it queues, it renders, it lands in the cache), so the same epoch that
 *  invalidates cell renders invalidates it, and the set — like the
 *  generation counters — resets whole at document teardown. */
const prefetchInFlight = new Set<number>();
/** Per-canvas-id generation counters, invalidating queued jobs a newer mount
 *  of the same id superseded. Ids are per page (`thumb-{page}`), so the map
 *  grows with the pages a document's sidebar showed — deliberately NOT pruned
 *  per id at cancel time: the counters exist to outlive unmounts, and
 *  deleting an id's entry at cancel would let a still-queued job collide with
 *  a fresh mount's generation 1 and paint the wrong page into the recycled
 *  canvas. The map resets whole at document teardown instead
 *  (`resetThumbLane`), which is safe because the lane's epoch invalidates
 *  every queued job in the same breath. */
const thumbGeneration = new Map<string, number>();
/** Whether a document's lane is open for business. Teardown closes it: a
 *  request that straggles in after `resetThumbLane` belongs to the dead
 *  lane and must not reseed the generation bookkeeping the teardown just
 *  cleared (the drained gate reads this map as zero). */
let thumbLaneOpen = false;
/** Bumped at document teardown. Queued lane jobs capture it and resolve as
 *  cancelled when they reach the front of the queue under a newer epoch,
 *  instead of racing the next document's mounts for recycled canvas ids. */
let thumbLaneEpoch = 0;

/** Forget the lane's per-document bookkeeping: every queued job is
 *  invalidated wholesale (the epoch) and the generation counters go with the
 *  canvas ids they were issued for — the next document reissues those ids
 *  from 1, and counters surviving across documents would collide with it.
 *  Called from the engine's destroy. */
export function resetThumbLane(): void {
  thumbLaneEpoch += 1;
  thumbLaneOpen = false;
  thumbGeneration.clear();
  prefetchInFlight.clear();
  // Wake every await that is racing the epoch: teardown just moved it, so
  // anything waiting for "this prefetch's era ended" must stop now. The
  // cascade also drains the queue: each waiting job takes its stale-epoch
  // guard, resolves its caller with a drop, and pumps the next one.
  const waiters = epochWaiters.splice(0);
  for (const wake of waiters) wake();
  pumpThumbQueue();
}

/// A cancellable "the epoch moved past `epoch`" signal. A pdf.js task
/// created inside the destroy window — after the cancel sweep, before the
/// document nulls — sits on a worker that will never answer, and its
/// promise never settles on its own; the epoch is the truthful "give up"
/// signal every in-flight await can race against. Every subscriber gets an
/// unsubscribe, because a prefetch that settles normally (the usual case)
/// must remove its waiter rather than park a resolver here for the
/// document's lifetime.
const epochWaiters: Array<() => void> = [];

function epochMovedSignal(epoch: number): {
  promise: Promise<void>;
  unsubscribe: () => void;
} {
  if (epoch !== thumbLaneEpoch) {
    return { promise: Promise.resolve(), unsubscribe: () => {} };
  }
  let resolve!: () => void;
  const promise = new Promise<void>((r) => {
    resolve = r;
  });
  const waiter = () => resolve();
  epochWaiters.push(waiter);
  return {
    promise,
    unsubscribe: () => {
      const at = epochWaiters.indexOf(waiter);
      if (at >= 0) epochWaiters.splice(at, 1);
    },
  };
}

/// The thumbnail lane's gauges for the stats surface (queue depth, active
/// slots): the teardown baseline requires an EMPTY lane, not merely one
/// whose jobs are no longer running.
export function thumbLaneGauge(): { thumbQueue: number; thumbActive: number } {
  return { thumbQueue: thumbQueue.length, thumbActive: thumbActive };
}

/** The generation map's size for the stats surface: per-canvas bookkeeping
 *  the lane keeps until document teardown. The baseline measures it so a
 *  long scrolling session cannot grow it unseen. */
export function thumbGenerationSize(): number {
  return thumbGeneration.size;
}

/** A document's lane is open again: generation bookkeeping records anew. */
export function beginThumbLane(): void {
  thumbLaneOpen = true;
}

function nextThumbGeneration(canvasId: string): number {
  if (!thumbLaneOpen) return 0;
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
/** Insert a thumbnail entry into the cache, releasing any previous entry for
 *  the page and evicting the LRU entry if the cache is full. */
function cachePut(page: number, entry: ThumbEntry): void {
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
    const epoch = thumbLaneEpoch;
    thumbQueue.push(() => {
      const finish = () => {
        thumbActive -= 1;
        pumpThumbQueue();
      };
      // The cell disappeared, a newer mount re-used this id, or the document
      // was torn down while the job waited, before the job reached the front
      // of the queue. Drop it without touching pdf.js.
      if (
        epoch !== thumbLaneEpoch
        || session.thumbCancelled.has(canvasId)
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
    // alias raw === display and never release `off` here: cacheDisplay /
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
export async function prefetchThumb(page: number, scale: number): Promise<void> {
  if (!session.pdf) return;
  const hit = session.thumbCache.get(page);
  if (hit && Math.abs(hit.scale - scale) < 1e-9) return;
  if (prefetchInFlight.has(page)) return;
  const epoch = thumbLaneEpoch;
  prefetchInFlight.add(page);
  session.prefetchesStarted += 1;
  session.prefetchesActive += 1;
  lifecycleEvent("thumb_prefetch:start");
  try {
    await new Promise<void>((resolve) => {
      thumbQueue.push(() => {
        const finish = () => {
          thumbActive -= 1;
          pumpThumbQueue();
        };
        // The document was torn down (or replaced) while this prefetch
        // waited for a lane slot. Drop it without touching pdf.js — the
        // same guard a queued cell render gets.
        if (epoch !== thumbLaneEpoch || !session.pdf) {
          session.prefetchesDropped += 1;
          lifecycleEvent("thumb_prefetch:drop");
          resolve();
          finish();
          return;
        }
        prefetchThumbInternal(page, scale, epoch)
          .then((landed) => {
            if (landed) {
              session.prefetchesCompleted += 1;
              lifecycleEvent("thumb_prefetch:complete");
            } else {
              session.prefetchesDropped += 1;
              lifecycleEvent("thumb_prefetch:drop");
            }
          })
          .catch(() => {
            session.prefetchesDropped += 1;
            lifecycleEvent("thumb_prefetch:drop");
          })
          .finally(finish)
          .finally(resolve);
      });
      pumpThumbQueue();
    });
  } finally {
    prefetchInFlight.delete(page);
    session.prefetchesActive -= 1;
  }
}

/// A combined cancellable signal for "this prefetch's world ended": the
/// lane epoch moved (teardown or swap ran before this await started), or
/// the document's destroy began while the await was in flight. The
/// document-gone half covers the destroy window the epoch alone cannot
/// see: a prefetch enqueued into the NEW epoch, onto a worker whose death
/// is already underway, awaiting a promise it will never see settle.
function prefetchWorldEnded(epoch: number): {
  promise: Promise<void>;
  unsubscribe: () => void;
} {
  const epochSignal = epochMovedSignal(epoch);
  const goneSignal = session.documentGoneSignal();
  const promise = Promise.race([epochSignal.promise, goneSignal.promise]);
  let done = false;
  return {
    promise,
    unsubscribe: () => {
      if (done) return;
      done = true;
      epochSignal.unsubscribe();
      goneSignal.unsubscribe();
    },
  };
}

/** The lane-slot half of a prefetch: render offscreen, bake, cache. Resolves
 *  `true` only when the entry landed in THIS document's cache; every stale
 *  or failed path cleans up after itself and resolves `false`. The pdf.js
 *  task rides `session.thumbTasks` under a synthetic id, so `destroy`'s
 *  cancel-everything sweep reaches it and `stats().thumbTasks` counts it. */
async function prefetchThumbInternal(
  page: number,
  scale: number,
  epoch: number
): Promise<boolean> {
  const taskId = `prefetch-${page}`;
  const dying = prefetchWorldEnded(epoch);
  try {
    const pg = await Promise.race([
      session.pdf!.getPage(page),
      dying.promise.then(() => null),
    ]);
    if (!pg || epoch !== thumbLaneEpoch || !session.pdf) {
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
    session.thumbTasks.set(taskId, task);
    const rendering = prefetchWorldEnded(epoch);
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
    session.thumbTasks.delete(taskId);
    if (epochMoved) {
      // The teardown sweep had already run when this task was created, so
      // the cancel-everything pass never reached it — cancel it here, or it
      // would render into a canvas this prefetch is about to release.
      try { task.cancel(); } catch (_) { /* ignore */ }
    }
    if (!rendered || epochMoved || epoch !== thumbLaneEpoch || !session.pdf) {
      releaseCanvas(off);
      try { pg.cleanup(); } catch (_) { /* ignore */ }
      return false;
    }
    pg.cleanup();
    const raw = off;
    let display: MaybeCanvas = session.themeScrubActive ? raw : await bakeRaster(raw, readPipeline());
    if (display !== raw) display = await cacheDisplay({ display });
    // The epoch check AGAIN: a bake can wait on the theme queue, and a
    // document swap in that window must not file this book's colours into
    // the next document's cache.
    if (epoch !== thumbLaneEpoch || !session.pdf) {
      releaseCanvas(off);
      return false;
    }
    cachePut(page, { raw, display, cssW: Math.floor(viewport.width),
                     cssH: Math.floor(viewport.height), scale,
                     gen: session.themeScrubActive ? -1 : pipelineCache.gen, pending: null });
    return true;
  } catch (_) {
    session.thumbTasks.delete(taskId);
    return false;
  } finally {
    dying.unsubscribe();
  }
}
