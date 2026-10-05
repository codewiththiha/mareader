// Raster baking: apply the CSS filter chain + paper blend to raw page pixels
// on the CPU. Hardware ctx.filter is not byte-identical across WebKit/Blink
// and would regress dark-mode inversion (the reason this baker exists).
// Intermediates recycle the shared scratch canvas so a bake never pins a
// second full-page buffer after it returns, and the paint lands ON the visible
// canvas: a themed page costs one read, one paper fill and one composite, not
// a third full-page surface plus the blit into the page.
// The per-pixel loop runs in a Worker (theme/bake.worker.ts, bundled to
// public/bake.worker.js) when the webview has one — a 4K page is ~8M
// iterations. Without a Worker (the Node smoke harness, exotic webviews) the
// SAME kernel runs inline via ./filterKernel, so both paths are
// byte-identical by construction and the fallback is the tested reference.

import type { PipelineCache } from "../types";
import {
  acquirePooledCanvas,
  acquireScratch,
  blitInto,
  isSharedScratch,
  releasePooledCanvas,
  releaseScratch,
  showBaked,
  type RasterThemeTag,
} from "../canvas";
import { pipelineIsIdentity } from "./pipeline";
import { paperInfo } from "./paper";
import { applyFilterToData, isIdentityFilter } from "./filterKernel";

type BakeResponse = {
  id: number;
  buffer?: ArrayBuffer;
  error?: string;
};

let bakeWorker: Worker | null | undefined;
let bakeWorkerFailed = false;
let bakeSeq = 0;
const pendingBakes = new Map<
  number,
  { resolve: (d: Uint8ClampedArray) => void; reject: (e: unknown) => void }
>();

function getBakeWorker(): Worker | null {
  if (bakeWorker !== undefined) return bakeWorker;
  bakeWorker = null;
  if (bakeWorkerFailed) return null;
  try {
    if (typeof Worker === "undefined") return null;
    // Absolute URL, resolved against the app origin like the pdf.js worker
    // src: Tauri's custom protocol needs an absolute target, not a bare path.
    const url = new URL(
      "/bake.worker.js",
      globalThis.location?.href || "http://localhost/",
    ).href;
    const worker = new Worker(url);
    worker.onmessage = (ev: MessageEvent) => {
      const resp = ev.data as BakeResponse;
      const pending = pendingBakes.get(resp.id);
      if (!pending) return;
      pendingBakes.delete(resp.id);
      if (resp.error) {
        pending.reject(new Error(resp.error));
      } else if (resp.buffer) {
        pending.resolve(new Uint8ClampedArray(resp.buffer));
      } else {
        pending.reject(new Error("bake worker: empty reply"));
      }
    };
    worker.onerror = () => {
      // The worker died mid-bake: fail what is in flight and permanently
      // fall back to the inline kernel (keep the page rendering).
      bakeWorkerFailed = true;
      const bakes = [...pendingBakes.values()];
      pendingBakes.clear();
      for (const p of bakes) p.reject(new Error("bake worker failed"));
      try {
        worker.terminate();
      } catch (_) {
        /* already gone */
      }
      bakeWorker = null;
    };
    bakeWorker = worker;
  } catch (_) {
    bakeWorker = null;
  }
  return bakeWorker;
}

// The pixel count at or below which the filter runs on the main thread. The
// worker exists because a full page is millions of per-pixel iterations and
// the readback it needs is the stall the worker removes; a thumbnail is ~30k
// pixels, where the crossing — a bitmap copy, two messages, a readback in the
// worker — costs several times what the filter does, and a rail-wide re-bake
// is dozens of crossings. Both halves run the SAME kernel (`filterKernel` is
// the reference the Node harness checks against), so which path ran never
// shows in the pixels.
const INLINE_BAKE_MAX_PIXELS = 1 << 16;

/** Terminate the bake worker once nothing can ask it for work: the realm's
 *  last document session is gone. The worker is stateless (every buffer is
 *  transferred in and back out), so this releases only its thread and
 *  isolate — memory a reader realm left behind the shelf would otherwise
 *  keep with no document open — and the next bake starts a fresh one
 *  (`getBakeWorker`). A bake still in flight keeps it: its reply is owed. */
export function releaseBakeWorker(): void {
  if (!bakeWorker || pendingBakes.size > 0) return;
  try {
    bakeWorker.terminate();
  } catch (_) {
    /* already gone */
  }
  bakeWorker = undefined;
}

/** Run the pixel loop in the worker, transferring the buffer. The caller's
 *  `data` is detached on return — it must be discarded, not reused. */
function workerApply(
  data: Uint8ClampedArray,
  w: number,
  h: number,
  filter: string,
): Promise<Uint8ClampedArray> {
  return new Promise((resolve, reject) => {
    const worker = getBakeWorker();
    if (!worker) {
      reject(new Error("no bake worker"));
      return;
    }
    const id = ++bakeSeq;
    pendingBakes.set(id, { resolve, reject });
    try {
      worker.postMessage({ id, w, h, filter, buffer: data.buffer }, [data.buffer]);
    } catch (e) {
      pendingBakes.delete(id);
      reject(e);
    }
  });
}

/** Run the pixel loop in the worker WITHOUT a main-thread pixel readback:
 *  the raster crosses as an ImageBitmap (an off-thread copy, no
 *  synchronous GPU→CPU sync on this thread), and the worker reads, filters
 *  and returns the pixels from its own canvas. */
function workerApplyBitmap(
  bitmap: ImageBitmap,
  w: number,
  h: number,
  filter: string,
): Promise<Uint8ClampedArray> {
  return new Promise((resolve, reject) => {
    const worker = getBakeWorker();
    if (!worker) {
      reject(new Error("no bake worker"));
      return;
    }
    const id = ++bakeSeq;
    pendingBakes.set(id, { resolve, reject });
    try {
      worker.postMessage({ id, w, h, filter, bitmap }, [bitmap]);
    } catch (e) {
      pendingBakes.delete(id);
      reject(e);
    }
  });
}

async function applyFilterPixels(
  src: HTMLCanvasElement,
  filterString: string,
): Promise<HTMLCanvasElement | null> {
  if (isIdentityFilter(filterString)) return src;

  const w = src.width;
  const h = src.height;
  if (!(w > 0) || !(h > 0)) return src;

  const img = await filterPixelsFor(src, w, h, filterString);
  if (!img) return src;

  // Always write to a scratch copy: mutating `src` in place destroyed the
  // unbaked thumbnail raw, so the next theme change double-filtered and live
  // thumbs could not be rebaked without a pdf.js re-render.
  const out = acquireScratch(w, h);
  const octx = out.getContext("2d", { alpha: false });
  if (!octx) return src;
  octx.putImageData(img, 0, 0);
  return out;
}

/** The FILTERED pixels of `src`, or null when nothing changes: the pixels
 *  cannot be read at all, or the inline kernel found the filter an identity
 *  on them. Three readback paths, most off-thread first, and a raster too
 *  small for the crossing to pay for itself (INLINE_BAKE_MAX_PIXELS) takes the
 *  last one directly:
 *  1. the worker reads them from a transferred ImageBitmap — the main
 *     thread pays no synchronous GPU->CPU sync, which a getImageData there
 *     would force once per page (the visible hitch of a multi-pane theme
 *     change);
 *  2. a worker exists but createImageBitmap does not: the legacy buffer
 *     transfer (main-thread readback, worker-side loop);
 *  3. no worker, or a raster the worker would cost more than it saves: the
 *     inline kernel (the Node harness's tested reference). */
async function filterPixelsFor(
  src: HTMLCanvasElement,
  w: number,
  h: number,
  filterString: string,
): Promise<ImageData | null> {
  const worker = getBakeWorker();
  const offThread = !!worker && w * h > INLINE_BAKE_MAX_PIXELS;

  if (offThread && typeof createImageBitmap === "function") {
    let bitmap: ImageBitmap | null = null;
    try {
      bitmap = await createImageBitmap(src);
      const back = await workerApplyBitmap(bitmap, w, h, filterString);
      return new ImageData(back, w, h);
    } catch (_) {
      // The bitmap or the worker failed mid-flight: fall through to the
      // readback below — the page still gets its theme.
      try { bitmap?.close(); } catch (_) { /* already closed */ }
    }
  }

  let img: ImageData | null | undefined;
  try {
    const sctx = src.getContext("2d");
    img = sctx && sctx.getImageData(0, 0, w, h);
  } catch (_) {
    return null;
  }
  if (!img) return null;

  if (offThread) {
    try {
      // `img.data` is detached by the transfer: the reply gets a fresh
      // ImageData.
      const back = await workerApply(img.data, w, h, filterString);
      return new ImageData(back, w, h);
    } catch (_) {
      // The worker vanished mid-flight (its onerror already rejected this
      // promise) and the transferred pixels are gone: degrade to the
      // unfiltered raster for this frame only — the page still renders.
      // The failure is permanent (bakeWorkerFailed), so the next bake goes
      // through the inline kernel.
      return null;
    }
  }

  return applyFilterToData(img.data, w, h, filterString) ? img : null;
}

export function rasterToCanvas(src: HTMLCanvasElement | ImageBitmap): {
  canvas: HTMLCanvasElement;
  borrowed: boolean;
} {
  if (typeof (src as HTMLCanvasElement).getContext === "function") {
    return { canvas: src as HTMLCanvasElement, borrowed: false };
  }
  const c = acquirePooledCanvas(
    (src as ImageBitmap).width,
    (src as ImageBitmap).height,
  );
  const ctx = c.getContext("2d", { alpha: false });
  if (ctx) ctx.drawImage(src as CanvasImageSource, 0, 0);
  return { canvas: c, borrowed: true };
}

/** A bake's filtered raster, plus the surface the baker owns for it. `owned`
 *  is `null` when the filter left the caller's own pixels alone, so no caller
 *  has to guess which canvas it is allowed to release. */
export type Baked = {
  painted: HTMLCanvasElement;
  owned: HTMLCanvasElement | null;
};

/** The bake's async half: `src` through the pipeline's filter chain, on a
 *  surface of the baker's choosing (the shared scratch, or a pooled canvas
 *  while the scratch is mid-bake). Deliberately split from the paint: a caller
 *  that must re-decide after an await — the page render checks its theme
 *  generation, which can move while the worker is busy — can drop the bake
 *  before any visible canvas has been touched. */
export async function bakeFiltered(
  src: HTMLCanvasElement,
  pipeline: PipelineCache,
): Promise<Baked> {
  if (pipelineIsIdentity(pipeline) || pipeline.filter === "none") {
    return { painted: src, owned: null };
  }
  const filtered = (await applyFilterPixels(src, pipeline.filter)) ?? src;
  return { painted: filtered, owned: filtered === src ? null : filtered };
}

/** Return a bake's intermediate to whichever recycler owns it. */
export function releaseBake(baked: Baked): void {
  const owned = baked.owned;
  if (!owned) return;
  if (isSharedScratch(owned)) releaseScratch(owned);
  else releasePooledCanvas(owned);
}

/** Fill `surface` with the pipeline's paper and composite `painted` over it
 *  with the pipeline's blend, sized to the raster first: the clear and the
 *  draw are one uninterrupted turn, so a destination never paints half a
 *  page. `source-over` then the blend is the order the baker has always used,
 *  which is what keeps a baked page and the CSS-composited backdrop equal. */
function blendOnto(
  surface: HTMLCanvasElement,
  painted: HTMLCanvasElement,
  pipeline: PipelineCache,
): boolean {
  if (surface.width !== painted.width || surface.height !== painted.height) {
    surface.width = painted.width;
    surface.height = painted.height;
  }
  const ctx = surface.getContext("2d", { alpha: false });
  if (!ctx) return false;
  ctx.globalCompositeOperation = "source-over";
  ctx.fillStyle = paperInfo(pipeline).color;
  ctx.fillRect(0, 0, surface.width, surface.height);
  ctx.globalCompositeOperation = pipeline.blend as GlobalCompositeOperation;
  ctx.drawImage(painted, 0, 0);
  ctx.globalCompositeOperation = "source-over";
  return true;
}

/** Swap `painted` in as `dst`'s pixels, keeping its raw marker straight. */
function blitBaked(
  dst: HTMLCanvasElement,
  painted: HTMLCanvasElement,
  visibleTag?: RasterThemeTag,
): void {
  if (visibleTag) showBaked(dst, painted, visibleTag);
  else blitInto(dst, painted);
}

/** Land a finished bake on the VISIBLE canvas. The blend is composited onto
 *  `dst` itself rather than into a second full-page surface that is then
 *  blitted across, so a theme change costs one fill and one draw per page.
 *  The bake's intermediate is recycled here; `src` stays the caller's. */
export function paintBaked(
  dst: HTMLCanvasElement,
  baked: Baked,
  pipeline: PipelineCache,
  visibleTag?: RasterThemeTag,
): void {
  const painted = baked.painted;
  const blend = !pipelineIsIdentity(pipeline) && pipeline.blend !== "normal";
  if (painted !== dst) {
    if (!blend || !blendOnto(dst, painted, pipeline)) blitBaked(dst, painted, visibleTag);
  } else if (blend) {
    // The render drew IN PLACE, so the destination is also the source and a
    // blend onto it would erase the pixels it is about to read. This one case
    // still pays for a stand-in surface.
    const copy = acquirePooledCanvas(painted.width, painted.height);
    if (blendOnto(copy, painted, pipeline)) blitBaked(dst, copy, visibleTag);
    releasePooledCanvas(copy);
  }
  if (visibleTag) dst.classList.remove(visibleTag);
  releaseBake(baked);
}

/** The standalone bake: `src` through filter AND blend into a surface the
 *  caller owns — or `src` itself when the pipeline leaves its pixels alone.
 *  For a result that outlives one paint: the thumbnail cache keeps its baked
 *  display next to the raw it was baked from. */
export async function bakeRaster(
  src: HTMLCanvasElement,
  pipeline: PipelineCache,
): Promise<HTMLCanvasElement> {
  if (pipelineIsIdentity(pipeline)) return src;
  const baked = await bakeFiltered(src, pipeline);
  if (pipeline.blend === "normal") return baked.painted;
  const out = acquirePooledCanvas(src.width, src.height);
  if (!blendOnto(out, baked.painted, pipeline)) {
    // No context to blend with: hand back the filtered surface and give the
    // pooled one straight back, so a failed bake cannot strand a page-sized
    // canvas outside every recycler.
    releasePooledCanvas(out);
    releaseBake(baked);
    return baked.painted;
  }
  releaseBake(baked);
  return out;
}

/** Bake `src` and land it on `dst` in one call (see `paintBaked`). */
export async function bakeInto(
  dst: HTMLCanvasElement,
  src: HTMLCanvasElement,
  pipeline: PipelineCache,
  visibleTag?: RasterThemeTag,
): Promise<void> {
  paintBaked(dst, await bakeFiltered(src, pipeline), pipeline, visibleTag);
}

// only the changed file was rewritten
