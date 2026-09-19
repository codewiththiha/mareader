// Browser policy above the admission lane. Geometry stays in the virtualizer;
// these distances only decide which of its mounted pages deserve pixels.
import { ID_PREFIX_HSCROLL } from "./dom-contract";
import { session, PAGE_MAX_PIXELS } from "./state";
import type { PageState } from "./types";
import { releaseCanvas } from "./canvas";
import { RasterScheduler, ScrollMotion, HARD_RASTER_BYTES } from "./raster-scheduler";

export const motion = new ScrollMotion();
export const rasterTelemetry = {
  fullRendersStarted: 0, fullRendersStartedDuringFling: 0,
  fullRendersCompletedOffscreen: 0, rasterPixelsProduced: 0,
};
export type RasterResidency = "None" | "Preview" | "Full" | "FullPlusRaw";
export function rasterResidency(st: PageState): RasterResidency {
  if (!st.canvas || !st.canvas.width || !st.canvas.height) return "None";
  if (!st.viewport) return "Preview";
  return st.rawCanvas && st.rawCanvas !== st.canvas ? "FullPlusRaw" : "Full";
}
// Raw target, replacement display, filter ImageData/worker buffer, scratch,
// blend output and a safety surface. Existing display/raw/snapshots are resident
// separately. This deliberately over-reserves identity renders.
export const RASTER_BYTES_PER_PIXEL = 24;
let scroller: HTMLElement | null = null;
let horizontal = false;
let settleTimer: ReturnType<typeof setTimeout> | undefined;
let idleTimer: ReturnType<typeof setTimeout> | undefined;

export function rasterResidentBytes(): number {
  const surfaces = new Set<{ width: number; height: number }>();
  const add = (surface: { width: number; height: number } | null) => {
    if (surface) surfaces.add(surface);
  };
  // Includes DOM preview canvases and zoom/scrub snapshots, not just PageState.
  document.querySelectorAll("canvas").forEach((canvas) => add(canvas as HTMLCanvasElement));
  for (const st of session.stateByCanvasId.values()) { add(st.canvas); add(st.rawCanvas); }
  for (const entry of session.thumbCache.values()) { add(entry.raw); add(entry.display); }
  let bytes = 0;
  for (const surface of surfaces) bytes += surface.width * surface.height * 4;
  return bytes;
}

export function pageIntent(st: PageState): { visible: boolean; distance: number; predicted: number; ahead: boolean } {
  const rect = st.host?.getBoundingClientRect?.();
  // The page-mode boot and smoke harness may not have laid out a host yet.
  if (!rect || !Number.isFinite(rect.bottom) || (!rect.width && !rect.height)) {
    return { visible: true, distance: 0, predicted: 0, ahead: true };
  }
  const bounds = scroller && st.host && scroller.contains(st.host) ? scroller.getBoundingClientRect() : undefined;
  const start = horizontal ? rect.left : rect.top;
  const end = horizontal ? rect.right : rect.bottom;
  const near = bounds ? (horizontal ? bounds.left : bounds.top) : 0;
  const far = bounds ? (horizontal ? bounds.right : bounds.bottom)
    : (horizontal ? globalThis.innerWidth : globalThis.innerHeight) || 800;
  const size = Math.max(1, far - near);
  const distance = Math.max(near - end, start - far, 0) / size;
  const center = (start + end - near - far) / 2;
  return {
    visible: end > near && start < far,
    distance,
    predicted: Math.abs(center - (motion.predicted - motion.offset)) / size,
    ahead: Math.sign(center) === motion.direction,
  };
}

export function pagePriority(st: PageState): number | null {
  if (st.dead || !st.canvas) return null;
  const intent = pageIntent(st);
  if (intent.distance > 0.75) return null;
  if (motion.phase === "Fling" || motion.phase === "Tracking") return -Infinity;
  return (intent.visible ? 1000 : intent.predicted < 0.5 ? 900 : intent.ahead ? 800 : 500)
    - intent.distance * 100 - intent.predicted * 10;
}

export function pagePixelLimit(): number {
  return Math.min(PAGE_MAX_PIXELS, (motion.phase === "Idle" ? 8 : 4) * 1024 * 1024);
}

function evictOptionalRasters(bytes: number): void {
  if (bytes <= 0) return;
  // Drop distant optional raws first. Never wipe a mounted visible canvas:
  // Blank/Zombie unmount the raster child and own full-surface eviction.
  const pages = [...session.stateByCanvasId].sort((a, b) => pageIntent(b[1]).distance - pageIntent(a[1]).distance);
  for (const [id, st] of pages) {
    if (rasterScheduler.has(id) || st.renderTask || !st.rawCanvas || st.rawCanvas === st.canvas || session.themeScrubActive) continue;
    bytes -= st.rawCanvas.width * st.rawCanvas.height * 4;
    releaseCanvas(st.rawCanvas);
    st.rawCanvas = null;
    if (bytes <= 0) return;
  }
  for (const [page, entry] of session.thumbCache) {
    if (entry.pending) continue;
    const before = rasterResidentBytes();
    session.thumbCache.delete(page);
    session.releaseThumbEntry(entry);
    bytes -= before - rasterResidentBytes();
    if (bytes <= 0) return;
  }
}

export const rasterScheduler = new RasterScheduler({
  resident: rasterResidentBytes,
  evict: evictOptionalRasters,
  concurrency: () => motion.phase === "Fling" || motion.phase === "Tracking" ? 0 : motion.phase === "Settling" ? 1 : 2,
  defer: (callback) => { requestAnimationFrame(callback); },
});

export function resetRasterResources(): void {
  rasterScheduler.reset();
  clearTimeout(settleTimer);
  clearTimeout(idleTimer);
  scroller = null;
  horizontal = false;
  motion.settle();
  motion.idle();
}

export function mayPrefetch(): boolean {
  const stats = rasterScheduler.stats();
  return motion.phase === "Idle" && stats.queued === 0 && stats.running === 0
    && stats.residentBytes + stats.reservedBytes < stats.softBytes;
}

export function mayCopyRaster(bytes: number): boolean {
  const stats = rasterScheduler.stats();
  return stats.residentBytes + stats.reservedBytes + bytes <= HARD_RASTER_BYTES;
}

// Capture scroll once for both axes, but ignore sidebar/library scrolling.
// No standing RAF loop: scroll bursts and two settle timers are the only work.
if (typeof document.addEventListener === "function") {
  document.addEventListener("scroll", (event) => {
    const target = event.target as HTMLElement | null;
    if (!target || typeof target.contains !== "function") return;
    const page = [...session.stateByCanvasId].find(([, st]) => st.canvas && target.contains(st.canvas));
    if (!page) return;
    const x = target.scrollLeft;
    const y = target.scrollTop;
    // The horizontal strip can also pan vertically when zoomed. Its main
    // axis is a host contract, not whichever absolute offset is larger.
    const isHorizontal = page[0].startsWith(`${ID_PREFIX_HSCROLL}-`);
    if (scroller !== target || horizontal !== isHorizontal) {
      motion.offset = isHorizontal ? x : y;
      motion.velocity = 0;
    }
    scroller = target;
    horizontal = isHorizontal;
    motion.update(horizontal ? x : y, performance.now(),
      horizontal ? target.clientWidth : target.clientHeight,
      horizontal ? target.scrollWidth : target.scrollHeight);
    rasterScheduler.replan();
    clearTimeout(settleTimer);
    clearTimeout(idleTimer);
    settleTimer = setTimeout(() => {
      motion.settle();
      rasterScheduler.replan();
      idleTimer = setTimeout(() => { motion.idle(); rasterScheduler.replan(); }, 80);
    }, 100);
  }, { capture: true, passive: true });
}

/** Opt-in development HUD: append ?rasterDebug=1 to the app URL. Process/GPU
 * memory is intentionally NOT inferred from canvas byte estimates. */
if (globalThis.location?.search.includes("rasterDebug=1")) {
  const hud = document.createElement("pre");
  hud.style.cssText = "position:fixed;bottom:12px;right:12px;z-index:2147483647;padding:12px;background:#101820e8;color:#d6f3ea;font:12px monospace;pointer-events:none";
  document.body.appendChild(hud);
  setInterval(() => {
    const stats = rasterScheduler.stats();
    const counts = { None: 0, Preview: 0, Full: 0, FullPlusRaw: 0 };
    for (const st of session.stateByCanvasId.values()) counts[rasterResidency(st)]++;
    hud.textContent = [
      `Raster: ${motion.phase}  ${motion.velocity.toFixed(2)} px/ms  dir ${motion.direction}`,
      `Predicted offset: ${Math.round(motion.predicted)}  nav ${motion.generation}`,
      `Resident ${(stats.residentBytes / 1048576).toFixed(1)} MiB + reserved ${(stats.reservedBytes / 1048576).toFixed(1)} MiB`,
      `Soft ${stats.softBytes / 1048576} / hard ${stats.hardBytes / 1048576} MiB`,
      `Queued ${stats.queued} / running ${stats.running} / dropped ${stats.queuedJobsDropped}`,
      `Full ${counts.Full} / raw pair ${counts.FullPlusRaw} / preview ${counts.Preview}`,
      `Fling starts ${rasterTelemetry.fullRendersStartedDuringFling} / offscreen completions ${rasterTelemetry.fullRendersCompletedOffscreen}`,
    ].join("\n");
  }, 250);
}
