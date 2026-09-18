// The page render scheduler: one priority queue in front of pdf.js.
//
// The lane it replaces was a FIFO with a depth of two. FIFO is the wrong
// order for a scroll: the page requested first is the page the reader was
// looking at when they started moving, and by the time a lane frees up it is
// the page they have left. A fling therefore spends both lanes on pages that
// are already off screen while the page the reader is arriving at waits
// behind them — which is exactly the blank-page-on-arrival this exists to
// remove.
//
// Three things decide the order, in this order of importance:
//
//   1. WHERE the page is. Inside the full window beats inside the preview
//      window beats outside both, and the predicted destination outranks its
//      neighbours even when no window reaches it.
//   2. WHICH WAY the reader is going. A page behind them costs three times
//      the distance a page ahead does (motion.ts, directionalDistance).
//   3. WHAT it costs. An enormous page — a poster, a fold-out scan — is
//      demoted unless the reader is looking at it, so one 12-megapixel plate
//      cannot hold the lane while three ordinary pages wait.
//
// And one thing decides whether a job runs at all: a request the reader has
// already outrun is dropped before it touches pdf.js. Cancelling in-flight
// work is pdf.js's job (`renderTask.cancel`, which renderer.ts still does);
// dropping queued work is free, and during a fling it is most of the win.

import { currentMotion, directionalDistance, inWindow, motionGeneration, motionIsCurrent } from "./motion";
import { previewAdmissionOk } from "./memory";
import { fail } from "./errors";
import type { RenderResult } from "./types";

/** What a queued render is owed. The tier the virtualizer assigned the page,
 *  carried here because a preview and a full render of the same page are not
 *  interchangeable: the preview is cheaper, and the full one supersedes it. */
export type Quality = "full" | "preview";

/** One render, as the caller describes it. `run` does the work and resolves
 *  the caller's promise; `abandon` resolves it as cancelled without any work
 *  having been done. The scheduler owns neither. */
export type RenderRequest = {
  /** The canvas id: the queue's identity. One job per canvas, ever — a second
   *  request for the same canvas supersedes the first. */
  key: string;
  page: number;
  quality: Quality;
  /** Estimated output pixels, for the cost term. 0 means unknown. */
  pixels: number;
  run: () => Promise<RenderResult>;
  abandon: (error: RenderResult) => void;
};

type Entry = {
  key: string;
  page: number;
  quality: Quality;
  pixels: number;
  priority: number;
  /** The delay timer, while the request is waiting out the phase's pacing. */
  timer: ReturnType<typeof setTimeout> | null;
  /** Whether the entry is in `ready` (as opposed to waiting on its timer). */
  queued: boolean;
  run: () => Promise<RenderResult>;
  abandon: (error: RenderResult) => void;
};

/** The lane depth the engine falls back to: what a settled reader gets, and
 *  what a mode that publishes no motion gets. Two full-page rasters in flight
 *  is the point past which they start fighting for the same worker rather
 *  than overlapping with it. */
const DEFAULT_LANES = 2;
/** The ceiling, whatever a policy asks for. Beyond three, the rasters contend
 *  for CPU, memory bandwidth and the pdf.js worker, and time-to-first-page
 *  gets worse rather than better. */
const MAX_LANES = 3;
/** A page above this many output pixels is expensive enough to wait behind
 *  cheaper ones — unless it is the page the reader is looking at. */
const HUGE_PAGE_PIXELS = 6_000_000;

const FULL_WINDOW_SCORE = 10_000;
const PREVIEW_WINDOW_SCORE = 2_000;
const PREDICTED_SCORE = 4_000;
const QUALITY_FULL_SCORE = 500;
const QUALITY_PREVIEW_SCORE = 250;
const DISTANCE_PENALTY = 100;
const HUGE_PAGE_PENALTY = 500;
const pending = new Map<string, Entry>();
let ready: Entry[] = [];
let active = 0;
let epoch = 0;

const counters = {
  requested: 0,
  ran: 0,
  superseded: 0,
  dropped: 0,
  /** Estimated output pixels of requests that never ran. */
  wastedPixels: 0,
};

/** How many lanes the current motion asks for. */
function laneCount(): number {
  const asked = currentMotion().workers;
  if (!(asked > 0)) return DEFAULT_LANES;
  return Math.min(Math.max(Math.floor(asked), 1), MAX_LANES);
}

/** Score one request against the motion in hand. */
function score(page: number, quality: Quality, pixels: number): number {
  const motion = currentMotion();
  let priority = 0;
  if (inWindow(page, motion.full)) {
    priority += FULL_WINDOW_SCORE;
  } else if (inWindow(page, motion.preview)) {
    priority += PREVIEW_WINDOW_SCORE;
  }
  if (motion.predictedPage > 0 && page === motion.predictedPage) {
    priority += PREDICTED_SCORE;
  }
  priority += quality === "full" ? QUALITY_FULL_SCORE : QUALITY_PREVIEW_SCORE;
  priority -=
    directionalDistance(page, motion.predictedPage, motion.direction) * DISTANCE_PENALTY;
  // An expensive page waits behind cheap ones — but never when it is the page
  // under the reader's eyes, which is what the full window means.
  if (pixels > HUGE_PAGE_PIXELS && !inWindow(page, motion.full)) {
    priority -= HUGE_PAGE_PENALTY;
  }
  return priority;
}

/** Whether a queued request is still worth running.
 *
 * The windows are recomputed on every published frame, so "outside every
 * window the reader currently has" IS the staleness test — no separate
 * generation arithmetic, and no way for the two to disagree. It is only ever
 * asked of a FRESH moving frame: a publication older than the TTL (a zoom
 * holds scroll feedback off for the length of its tween) or a mode that
 * publishes no motion at all describes a reader who is no longer there, and
 * cancelling on the strength of it would drop renders the current one asked
 * for. A settled reader cancels nothing either — the settle is the moment the
 * preview ring is promoted, not the moment it is thrown away. */
function stillWanted(entry: Entry): boolean {
  if (!motionIsCurrent()) return true;
  const motion = currentMotion();
  if (motion.phase === "idle") return true;
  if (entry.page > 0 && entry.page === motion.predictedPage) return true;
  return inWindow(entry.page, motion.full) || inWindow(entry.page, motion.preview);
}

/** Insert into `ready` in priority order. The queue is short — bounded by the
 *  mounted window, so a dozen entries at most — which is why this is a linear
 *  insert rather than a heap: the heap's advantage starts at sizes this queue
 *  is not allowed to reach, and a sorted array keeps `pump` a `shift`. */
function insert(entry: Entry): void {
  let at = ready.length;
  while (at > 0 && ready[at - 1].priority < entry.priority) at -= 1;
  ready.splice(at, 0, entry);
}

/** Whether a job may take a lane now. A full raster always may; a preview
 *  waits while the preview tier is at its ceiling (memory.ts) — the tier is
 *  bounded by admission rather than by taking surfaces back afterwards,
 *  because the surfaces belong to canvases the app has mounted. */
function admissible(entry: Entry): boolean {
  return entry.quality === "full" || previewAdmissionOk();
}

function pump(): void {
  const lanes = laneCount();
  while (active < lanes) {
    // The highest-priority ADMISSIBLE job, not simply the head: a preview
    // waiting out its tier's ceiling must not block the full raster behind
    // it, which is the render the reader is actually waiting on.
    const at = ready.findIndex(admissible);
    if (at < 0) return;
    const entry = ready.splice(at, 1)[0];
    if (!entry) return;
    entry.queued = false;
    // Superseded while it waited, or the document was torn down under it.
    if (pending.get(entry.key) !== entry) continue;
    if (!stillWanted(entry)) {
      abandon(entry, "outrun");
      continue;
    }
    active += 1;
    entry
      .run()
      .catch(() => {
        /* the caller's promise is already resolved by run() */
      })
      .finally(() => {
        if (pending.get(entry.key) === entry) pending.delete(entry.key);
        counters.ran += 1;
        active -= 1;
        pump();
      });
  }
}

/** Resolve a request as cancelled and forget it. */
function abandon(entry: Entry, reason: "superseded" | "outrun" | "reset"): void {
  if (entry.timer) {
    clearTimeout(entry.timer);
    entry.timer = null;
  }
  if (entry.queued) {
    const at = ready.indexOf(entry);
    if (at >= 0) ready.splice(at, 1);
    entry.queued = false;
  }
  if (pending.get(entry.key) === entry) pending.delete(entry.key);
  if (reason === "superseded") counters.superseded += 1;
  else counters.dropped += 1;
  counters.wastedPixels += entry.pixels;
  entry.abandon(fail("cancelled", `Render ${reason}`));
}

/** Queue one render.
 *
 * A request for a canvas that already has one waiting supersedes it: the same
 * page asked for twice in a frame (a mount and a settle promotion, a zoom's
 * display and committed scale landing together) is one render, not two. The
 * superseded caller is resolved as cancelled rather than left hanging, which
 * is what makes "coalesced" observable to the component that asked. */
export function schedule(request: RenderRequest): void {
  counters.requested += 1;
  const existing = pending.get(request.key);
  if (existing) abandon(existing, "superseded");

  const motion = currentMotion();
  const entry: Entry = {
    key: request.key,
    page: request.page,
    quality: request.quality,
    pixels: request.pixels,
    priority: score(request.page, request.quality, request.pixels),
    timer: null,
    queued: false,
    run: request.run,
    abandon: request.abandon,
  };
  pending.set(entry.key, entry);

  // Pacing: a page mounted mid-fling waits out the phase's delay before it
  // joins the queue, and a request superseded inside that window never joins
  // it at all. That is the whole trick — the pages a throw flies past are
  // cancelled before they cost anything, and the one the reader lands on is
  // requested again by the settle, at the head of an empty queue.
  if (motion.delayMs > 0) {
    entry.timer = setTimeout(() => {
      entry.timer = null;
      if (pending.get(entry.key) !== entry) return;
      entry.queued = true;
      insert(entry);
      pump();
    }, motion.delayMs);
    return;
  }
  entry.queued = true;
  insert(entry);
  pump();
}

/** Drop a canvas's queued render without running it. Called when the page
 *  unmounts (`unregisterPage`) or the caller cancels explicitly; an in-flight
 *  render is pdf.js's to cancel, and renderer.ts does that separately. */
export function unschedule(key: string): void {
  const entry = pending.get(key);
  if (!entry) return;
  abandon(entry, "outrun");
}

/** Re-score the queue against fresh motion. Called on every publication:
 *  the windows move, so yesterday's order is wrong, and a page that was
 *  outside every window a frame ago may now be the destination. */
export function onMotionPublished(): void {
  if (ready.length === 0 && pending.size === 0) return;
  for (const entry of pending.values()) {
    entry.priority = score(entry.page, entry.quality, entry.pixels);
  }
  // Re-sort only what is waiting; a running job cannot be re-ordered.
  ready.sort((a, b) => b.priority - a.priority);
  // A movement can also make a queued request pointless — drop those before
  // spending a lane on them.
  const kept: Entry[] = [];
  const outrun: Entry[] = [];
  for (const entry of ready) (stillWanted(entry) ? kept : outrun).push(entry);
  if (outrun.length > 0) {
    ready = kept;
    for (const entry of outrun) abandon(entry, "outrun");
  }
  pump();
}

/** Forget every queued render: document teardown. In-flight renders are
 *  cancelled by the page teardown that runs alongside this. */
export function resetScheduler(): void {
  epoch += 1;
  for (const entry of [...pending.values()]) abandon(entry, "reset");
  ready = [];
  active = 0;
  pending.clear();
}

/** Queue depth, lane state and the counters a tuning session reads, for
 *  `stats()`. `savedPixels` is the number that says whether the pacing is
 *  earning its keep: the estimated output of every request that was
 *  superseded or dropped before it ran, so it is work the scheduler never
 *  started rather than work it threw away. */
export function schedulerStats(): {
  queued: number;
  active: number;
  lanes: number;
  generation: number;
  requested: number;
  ran: number;
  superseded: number;
  dropped: number;
  savedPixels: number;
  epoch: number;
} {
  return {
    queued: ready.length,
    active,
    lanes: laneCount(),
    generation: motionGeneration(),
    requested: counters.requested,
    ran: counters.ran,
    superseded: counters.superseded,
    dropped: counters.dropped,
    savedPixels: counters.wastedPixels,
    epoch,
  };
}
