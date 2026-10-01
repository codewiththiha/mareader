// The warm page cache: a downscaled copy of each page the reader actually
// LOOKED AT, kept after its surface unmounts so a scroll back paints a
// near-crisp page on the very first frame instead of a blank card (or an
// upscaled 0.25 thumbnail) while the full raster waits for the fling gate
// and the render lane.
//
// Capture is commitment-gated (rules.md §2/§3): only a page whose full
// render COMPLETED (it has a viewport) and is not mid-render or mid-scrub is
// copied, so a fling sweeping past unpainted pages copies nothing. The copy
// is the already-baked display pixels, tagged with the pipeline generation
// it was baked under; a lookup under a different generation drops the entry
// instead of painting stale colours.
//
// Bound and drain (rules.md §5): WARM_PAGE_MAX entries per session, each at
// most WARM_EDGE_PX on its long side (≤ ~1.6 MB RGBA for a Letter page, so
// ≤ ~16 MB per session). Drained by teardown (`destroySession`), quiesce,
// the session's idle sweep (SWEEP_IDLE_MS), pagehide, and a hidden window.
// Every entry is released through `releaseCanvas` (§6).

import { releaseCanvas, showBaked } from "./canvas";
import type { EngineSession } from "./state";
import type { PageState } from "./types";

/** Entries kept per session (LRU). */
export const WARM_PAGE_MAX = 10;
/** Long-side ceiling of a warm copy, device pixels. */
export const WARM_EDGE_PX = 720;

export type WarmPage = {
  canvas: HTMLCanvasElement;
  /** Pipeline generation the copied pixels were baked under. */
  gen: number;
};

/** Copy a settled page's pixels into the warm cache before its surface is
 *  released. No-op for pages that never finished a render. */
export function captureWarm(s: EngineSession, st: PageState): void {
  const src = st.canvas;
  if (!src || !st.viewport || st.renderTask) return;
  if (s.disposed || s.themeScrubActive || src.classList.contains("canvas-raw")) return;
  const w = src.width;
  const h = src.height;
  if (!(w > 1) || !(h > 1)) return;
  const k = Math.min(1, WARM_EDGE_PX / Math.max(w, h));
  const copy = document.createElement("canvas");
  copy.width = Math.max(1, Math.round(w * k));
  copy.height = Math.max(1, Math.round(h * k));
  const ctx = copy.getContext("2d", { alpha: false });
  if (!ctx) {
    releaseCanvas(copy);
    return;
  }
  try {
    ctx.imageSmoothingQuality = "high";
    ctx.drawImage(src, 0, 0, copy.width, copy.height);
  } catch (_) {
    releaseCanvas(copy);
    return;
  }
  putWarm(s, st.page, { canvas: copy, gen: s.themePipeline.gen });
}

function putWarm(s: EngineSession, page: number, entry: WarmPage): void {
  const prev = s.warmPages.get(page);
  if (prev) {
    s.warmPages.delete(page);
    releaseCanvas(prev.canvas);
  }
  s.warmPages.set(page, entry);
  while (s.warmPages.size > WARM_PAGE_MAX) {
    const oldest = s.warmPages.keys().next();
    if (oldest.done) break;
    const old = s.warmPages.get(oldest.value);
    s.warmPages.delete(oldest.value);
    if (old) releaseCanvas(old.canvas);
  }
}

/** Paint the warm copy of `page` into `dst`. False on a miss, a stale
 *  generation (dropped on the spot), or during a scrub — the scrub paints
 *  raw rasters only. A hit is refreshed in the LRU order. */
export function blitWarm(s: EngineSession, dst: HTMLCanvasElement | null, page: number): boolean {
  if (!dst || s.themeScrubActive) return false;
  const entry = s.warmPages.get(page);
  if (!entry) return false;
  s.warmPages.delete(page);
  if (entry.gen !== s.themePipeline.gen) {
    releaseCanvas(entry.canvas);
    return false;
  }
  s.warmPages.set(page, entry);
  return showBaked(dst, entry.canvas, "thumb-raw");
}

/** Release every warm copy of `s`. */
export function releaseWarm(s: EngineSession): void {
  for (const entry of s.warmPages.values()) releaseCanvas(entry.canvas);
  s.warmPages.clear();
}

/** Bytes the warm copies of `s` hold (stats gauge). */
export function warmBytes(s: EngineSession): number {
  let bytes = 0;
  for (const entry of s.warmPages.values()) bytes += entry.canvas.width * entry.canvas.height * 4;
  return bytes;
}
