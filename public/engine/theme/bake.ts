// Raster baking: CPU filter + blend (ctx.filter differs per engine).

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
    // Absolute URL: Tauri's custom protocol needs one, not a bare path.
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
      // Worker died: fail what is in flight, fall back to the inline kernel.
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

// Rasters this small cost less inline than a worker crossing.
const INLINE_BAKE_MAX_PIXELS = 1 << 16;

/** Terminate the worker once no session can ask for a bake. */
export function releaseBakeWorker(): void {
  if (!bakeWorker || pendingBakes.size > 0) return;
  try {
    bakeWorker.terminate();
  } catch (_) {
    /* already gone */
  }
  bakeWorker = undefined;
}

/** Off-thread filter: no main-thread GPU->CPU readback. */
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

  // A scratch copy: mutating `src` in place destroyed the unbaked thumb raw.
  const out = acquireScratch(w, h);
  const octx = out.getContext("2d", { alpha: false });
  if (!octx) return src;
  octx.putImageData(img, 0, 0);
  return out;
}

/** The filtered pixels of `src`, or null when the filter changes nothing. */
async function filterPixelsFor(
  src: HTMLCanvasElement,
  w: number,
  h: number,
  filterString: string,
): Promise<ImageData | null> {
  const worker = getBakeWorker();
  const offThread = !!worker && w * h > INLINE_BAKE_MAX_PIXELS;

  if (offThread) {
    let bitmap: ImageBitmap | null = null;
    try {
      bitmap = await createImageBitmap(src);
      const back = await workerApplyBitmap(bitmap, w, h, filterString);
      return new ImageData(back, w, h);
    } catch (_) {
      // Bitmap or worker failed: the inline kernel below still themes the page.
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

/** A bake's raster and the surface the baker may release (`null` = the
 *  caller's own). */
export type Baked = {
  painted: HTMLCanvasElement;
  owned: HTMLCanvasElement | null;
};

/** Filter `src` without painting, so a caller can drop the bake after an
 *  await. */
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

/** Fill paper then blend, sized first: never half a page. */
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

/** Land the bake on `dst`; the blend composites onto it directly. */
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
    // In-place render: destination is source, so the blend needs a stand-in.
    const copy = acquirePooledCanvas(painted.width, painted.height);
    if (blendOnto(copy, painted, pipeline)) blitBaked(dst, copy, visibleTag);
    releasePooledCanvas(copy);
  }
  if (visibleTag) dst.classList.remove(visibleTag);
  releaseBake(baked);
}

/** Filter and blend into a surface the thumbnail cache keeps, or `src` when
 *  nothing changes. */
export async function bakeRaster(
  src: HTMLCanvasElement,
  pipeline: PipelineCache,
): Promise<HTMLCanvasElement> {
  if (pipelineIsIdentity(pipeline)) return src;
  const baked = await bakeFiltered(src, pipeline);
  if (pipeline.blend === "normal") return baked.painted;
  const out = acquirePooledCanvas(src.width, src.height);
  if (!blendOnto(out, baked.painted, pipeline)) {
    // No blend context: return the filtered surface, recycle the pooled one.
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
