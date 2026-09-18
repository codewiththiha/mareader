// The raster ledger: what the engine is holding, and what it lets go first.
//
// A page count is the wrong unit for a PDF. One poster page can cost ten
// times what its neighbour does, so a document of a hundred pages is not
// necessarily cheaper than one of twenty, and a budget written in pages
// either starves an ordinary book or blows through a scanned one. The budget
// is therefore in bytes, and everything the engine holds is weighed the same
// way: a canvas backing store is `width * height * 4` whether it is a page,
// a retained raw or a sidebar thumbnail.
//
// Reclamation is tiered, cheapest concession first, and it never blanks a page
// the reader can see:
//
//   1. retained raws outside the full window — a raw exists so a tint scrub
//      can restore a page without re-rendering it, and a page three screens
//      away is not about to be scrubbed;
//   2. sidebar thumbnails, by importance rather than by age — the grid is a
//      cache in the strict sense, and every entry in it is re-derivable
//      without the reader noticing;
//   3. pdf.js's own worker caches, via the advisory cleanup the idle sweep
//      already fires.
//
// What it does NOT do is release a live page's canvas. Zeroing a backing store
// is how a surface is really freed, and doing that to a canvas the app has
// mounted would blank a page the virtualizer still considers painted — the
// reader would see a hole where a page was. The preview tier is bounded the
// other way round, by ADMISSION: `previewAdmissionOk` is what the scheduler
// asks before it starts a preview raster, so the tier cannot grow past its
// ceiling in the first place and there is nothing to take back afterwards.
// Choosing what tier to render at is the scheduler's job; this module only
// weighs the result.

import { releaseCanvas } from "./canvas";
import { currentBudget, currentMotion, directionalDistance, inWindow } from "./motion";
import { session } from "./state";
import type { PageState, ThumbEntry } from "./types";

/** Bytes per pixel of a canvas backing store: RGBA8, which is what every
 *  surface in here is. */
const BYTES_PER_PIXEL = 4;

/** How much a megabyte of surface costs an entry's importance score. The
 *  scale is what makes the two terms comparable: a 48MB poster page loses
 *  768 points, which outweighs being a couple of pages from the reader but
 *  not being on screen. */
const BYTES_PER_IMPORTANCE_POINT = 64 * 1024;

/** How close to the reader a page must be to be worth its raw. Outside the
 *  full window a retained raw is a scrub convenience for a page nobody is
 *  looking at. */
const RAW_KEEP_SCREEN_PAGES = 4;

/** The shortest gap between two motion-driven reclamations. A fling publishes
 *  a frame every 16ms; weighing the ledger that often buys nothing that the
 *  next weigh would not buy. */
const RECLAIM_THROTTLE_MS = 250;

let lastReclaimAt = 0;

/** A surface's backing-store size, or 0 for anything already released. */
function surfaceBytes(surface: { width: number; height: number } | null | undefined): number {
  if (!surface) return 0;
  const w = surface.width;
  const h = surface.height;
  if (!(w > 0) || !(h > 0)) return 0;
  return w * h * BYTES_PER_PIXEL;
}

/** What one live page state costs: its visible raster plus the unbaked raw it
 *  is holding for a scrub, when that raw is a separate surface. */
function pageBytes(st: PageState): number {
  const canvas = surfaceBytes(st.canvas);
  const raw = st.rawCanvas && st.rawCanvas !== st.canvas ? surfaceBytes(st.rawCanvas) : 0;
  return canvas + raw;
}

/** What one thumbnail entry costs: the unthemed raw and the baked display,
 *  which are two surfaces by design (see thumbnails.ts). */
function thumbBytes(entry: ThumbEntry): number {
  const display = surfaceBytes(entry.display as { width: number; height: number } | null);
  const raw = entry.raw && entry.raw !== entry.display
    ? surfaceBytes(entry.raw as { width: number; height: number } | null)
    : 0;
  return display + raw;
}

/** Everything the ledger counts, bytes. The scratch pad and the canvas pool
 *  are left out on purpose: both are bounded by construction (one scratch, six
 *  pooled canvases) and neither grows with the document or the movement. */
export function totalBytes(): number {
  let total = 0;
  for (const st of session.stateByCanvasId.values()) total += pageBytes(st);
  for (const entry of session.thumbCache.values()) total += thumbBytes(entry);
  return total;
}

/** What the preview tier is holding: bytes, and how many surfaces. */
function previewTotals(): { bytes: number; count: number } {
  let bytes = 0;
  let count = 0;
  for (const st of session.stateByCanvasId.values()) {
    if (st.dead || !st.preview) continue;
    bytes += pageBytes(st);
    count += 1;
  }
  return { bytes, count };
}

/** Whether the scheduler may start another preview raster.
 *
 * Both ceilings count: the byte one because a preview of a poster page is not
 * cheap just because it is a preview, and the page-count one because a
 * document of enormous pages would otherwise spend the whole budget on four
 * of them. The count is of surfaces the engine holds, so a page the app has
 * already replaced with a placeholder costs nothing here. */
export function previewAdmissionOk(): boolean {
  const budget = currentBudget();
  const held = previewTotals();
  return held.bytes < budget.previewBytes && held.count < budget.maxPreviewPages;
}

/** How much a page is worth right now: the reader's own answer (which window
 *  it is in, which way they are going, how near the destination it is) minus
 *  what holding it costs. Higher survives. */
function importance(page: number, bytes: number): number {
  const motion = currentMotion();
  let score = 0;
  if (inWindow(page, motion.full)) score += 10_000;
  else if (inWindow(page, motion.preview)) score += 2_000;
  if (motion.predictedPage > 0 && page === motion.predictedPage) score += 4_000;
  score -= directionalDistance(page, motion.predictedPage, motion.direction) * 250;
  score -= bytes / BYTES_PER_IMPORTANCE_POINT;
  return score;
}

/** Whether a live page's retained raw is still worth its surface. A scrub in
 *  progress or an open appearance menu keeps every raw — that is what they are
 *  for — and so does proximity to the reader. */
function rawIsWorthKeeping(st: PageState): boolean {
  if (!st.rawCanvas || st.rawCanvas === st.canvas) return true;
  if (session.themeScrubActive || session.appearanceMenuOpen) return true;
  if (session.scrubIsPlausible() && nearReader(st.page)) return true;
  return false;
}

/** Whether a page is close enough to the reader to be worth holding pixels
 *  for. Falls back to the full window when no motion has been published (the
 *  paginated modes), where "close" is whatever is mounted. */
function nearReader(page: number): boolean {
  const motion = currentMotion();
  if (inWindow(page, motion.full) || inWindow(page, motion.preview)) return true;
  if (!(motion.predictedPage > 0)) return true;
  return Math.abs(page - motion.predictedPage) <= RAW_KEEP_SCREEN_PAGES;
}

/** The thumbnail entry to evict: the lowest importance, not the oldest.
 *
 * Insertion order is a reasonable proxy for importance when the only thing
 * that moves is the sidebar's own scroll. It stops being one the moment the
 * reader is somewhere else in the document: the thumbnails behind them and the
 * ones near where they are heading are not equally worth keeping, and an
 * age-only rule evicts whichever the grid happened to paint first. */
export function thumbEvictionCandidate(except: number | null): number | null {
  let worstPage: number | null = null;
  let worstScore = Infinity;
  for (const [page, entry] of session.thumbCache) {
    if (except !== null && page === except) continue;
    const score = importance(page, thumbBytes(entry));
    if (score < worstScore) {
      worstScore = score;
      worstPage = page;
    }
  }
  return worstPage;
}

/** Bring the ledger back under budget, cheapest concession first.
 *
 * `throttled` is what a motion publication passes: it means "only if it has
 *  been a moment since the last weigh", so a fling's sixty frames a second do
 *  not each walk the ledger. A render completion passes false and is weighed
 *  immediately, because that is the moment the total actually changed. */
export function reclaim(throttled = false): void {
  const now = Date.now();
  if (throttled && now - lastReclaimAt < RECLAIM_THROTTLE_MS) return;
  lastReclaimAt = now;

  const budget = currentBudget();
  if (totalBytes() <= budget.maxBytes) return;

  // 1. Retained raws the reader is not near. Dropping one costs a re-render
  //    on the next theme change, which is exactly the trade the existing idle
  //    tail makes — this just makes it under pressure rather than on a timer.
  for (const st of session.stateByCanvasId.values()) {
    if (st.dead || rawIsWorthKeeping(st)) continue;
    releaseCanvas(st.rawCanvas);
    st.rawCanvas = null;
    if (totalBytes() <= budget.maxBytes) return;
  }

  // 2. Sidebar thumbnails, least important first.
  while (session.thumbCache.size > 0 && totalBytes() > budget.maxBytes) {
    const page = thumbEvictionCandidate(null);
    if (page === null) return;
    const entry = session.thumbCache.get(page);
    session.thumbCache.delete(page);
    session.releaseThumbEntry(entry);
  }

  // 3. Still over: ask pdf.js for its worker caches back. Advisory, and the
  //    same call the idle sweep makes — the ledger has nothing left of its own
  //    to give up that would not blank a page.
  if (totalBytes() > budget.maxBytes) session.sweepPdf();
}
