// The ghost: one real page of the open document, rendered tiny, desaturated
// and stretched across every mounted-but-unpainted page as its placeholder.
// One small render (~4-8 KB of WebP, one blob URL) for the whole session,
// versus ~22 MB of canvas per full-resolution page — which is exactly what a
// fling used to churn.
//
// The render is keyed on the theme pipeline's generation: a repeat call with
// an unchanged appearance resolves the cached URL (the Rust side re-fires it
// on every appearance change and pays nothing when the pipeline did not
// move), and a render that started under one pipeline and lands under
// another discards its result rather than caching the wrong theme.

import {
  isSharedScratch,
  releasePooledCanvas,
  releaseScratch,
} from "./canvas";
import { bakeRaster } from "./theme/bake";
import { pipelineCache, pipelineIsIdentity, readPipeline } from "./theme/pipeline";
import { session } from "./state";

let cached: { url: string; gen: number } | null = null;
let inflight: { gen: number; promise: Promise<string | null> } | null = null;

/** Whether a ghost exists for the pipeline in force right now. */
export function ghostReady(): boolean {
  return !!cached && cached.gen === pipelineCache.gen;
}

/** Render the modal page at ~`heightPx` and resolve the ghost's blob URL.
 *  Resolves `null` when it cannot (no document, no context); the caller
 *  keeps what it has. */
export async function renderGhost(pageNo: number, heightPx: number): Promise<string | null> {
  if (!session.pdf) return null;
  const gen = pipelineCache.gen;
  if (cached && cached.gen === gen) return cached.url;
  if (inflight && inflight.gen === gen) return inflight.promise;
  const h = Math.max(1, Math.min(4096, Math.floor(heightPx) || 140));
  inflight = { gen, promise: doRenderGhost(pageNo, h, gen) };
  return inflight.promise;
}

/** Drop the cached blob (document teardown). */
export function resetGhost(): void {
  if (cached) URL.revokeObjectURL(cached.url);
  cached = null;
}

async function doRenderGhost(pageNo: number, h: number, gen: number): Promise<string | null> {
  let off: HTMLCanvasElement | null = null;
  try {
    const pg = await session.pdf!.getPage(pageNo);
    const base = pg.getViewport({ scale: 1 });
    const scale = base.height > 0 ? Math.min(1, h / base.height) : 1;
    const vp = pg.getViewport({ scale });
    const w = Math.max(1, Math.round(vp.width));
    const hh = Math.max(1, Math.round(vp.height));
    off = document.createElement("canvas");
    off.width = w;
    off.height = hh;
    const ctx = off.getContext("2d", { alpha: false });
    if (!ctx) return null;
    await pg.render({ canvasContext: ctx, viewport: vp }).promise;
    pg.cleanup();

    // Desaturate: reads as "a preview of a page", not the content.
    neutralize(ctx, w, hh);

    // Bake through the pipeline in force at START so the ghost matches the
    // pages it stands in for (sepia, dark, texture blends). The generation
    // guard below discards the result if the theme moved mid-flight.
    let display: HTMLCanvasElement = off;
    const pipe = readPipeline();
    if (!pipelineIsIdentity(pipe)) {
      const baked = await bakeRaster(off, pipe);
      if (baked !== off) display = baked;
    }
    if (gen !== pipelineCache.gen) {
      if (display !== off) releaseDisplay(display);
      return null;
    }
    const blob = await toBlob(display);
    if (display !== off) releaseDisplay(display);
    if (!blob) return null;
    if (cached) URL.revokeObjectURL(cached.url);
    const url = URL.createObjectURL(blob);
    cached = { url, gen };
    return url;
  } catch {
    return null;
  } finally {
    // A theme move during the render may have started a NEWER inflight —
    // only clear the slot if it is still ours.
    if (inflight && inflight.gen === gen) inflight = null;
    if (off) releaseOffscreen(off);
  }
}

/** Free a bake's intermediate the same way the render path does: the shared
 *  scratch goes back to the scratch, everything else to the pool. */
function releaseDisplay(display: HTMLCanvasElement): void {
  if (isSharedScratch(display)) {
    releaseScratch(display);
  } else {
    releasePooledCanvas(display);
  }
}

function releaseOffscreen(c: HTMLCanvasElement): void {
  c.width = 0;
  c.height = 0;
}

/** Luminance to all three channels: the sheet reads as a preview, not the
 *  document's content. */
function neutralize(ctx: CanvasRenderingContext2D, w: number, h: number): void {
  const img = ctx.getImageData(0, 0, w, h);
  const d = img.data;
  for (let i = 0; i < d.length; i += 4) {
    const g = (d[i]! * 0.299 + d[i + 1]! * 0.587 + d[i + 2]! * 0.114) | 0;
    d[i] = g;
    d[i + 1] = g;
    d[i + 2] = g;
  }
  ctx.putImageData(img, 0, 0);
}

function toBlob(c: HTMLCanvasElement): Promise<Blob | null> {
  return new Promise((res) => {
    try {
      c.toBlob((b) => res(b), "image/webp", 0.55);
    } catch (_) {
      res(null);
    }
  });
}
