// Raster baking: apply the CSS filter chain + paper blend to raw page
// pixels on the CPU. Hardware `ctx.filter` is not byte-identical across
// WebKit/Blink and would regress dark-mode inversion (the reason this
// baker exists). Intermediates recycle the shared scratch canvas so a bake
// never pins a second full-page buffer after it returns.
//
// The per-pixel loop itself runs in a Worker (public/engine/theme/
// bake.worker.ts, bundled to public/bake.worker.js) when the webview
// has one — a 4K page is ~8M iterations and they used to block the main
// thread every render. Without a Worker (the Node smoke harness, exotic
// webviews) the SAME kernel runs inline via ./filterKernel, so the two
// paths are byte-identical by construction and the fallback is the tested
// reference implementation.

import type { PipelineCache } from "../types";
import {
  acquirePooledCanvas,
  acquireScratch,
  blitInto,
  isSharedScratch,
  showBaked,
  type RasterThemeTag,
  releasePooledCanvas,
  releaseScratch,
} from "../canvas";
import { pipelineIsIdentity } from "./pipeline";
import { paperInfo } from "./paper";
import { applyFilterToData, composeFilter } from "./filterKernel";

// --- Worker routing --------------------------------------------------------

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

// --- Filter application ----------------------------------------------------

function isIdentityFilter(filterString: string): boolean {
  const { m, o } = composeFilter(filterString);
  return (
    m[0] === 1 && m[1] === 0 && m[2] === 0 &&
    m[3] === 0 && m[4] === 1 && m[5] === 0 &&
    m[6] === 0 && m[7] === 0 && m[8] === 1 &&
    o[0] === 0 && o[1] === 0 && o[2] === 0
  );
}

async function applyFilterPixels(
  src: HTMLCanvasElement,
  filterString: string,
): Promise<HTMLCanvasElement | null> {
  if (isIdentityFilter(filterString)) return src;

  const w = src.width;
  const h = src.height;
  const sctx = src.getContext("2d");
  let img: ImageData | null | undefined;
  try {
    img = sctx && sctx.getImageData(0, 0, w, h);
  } catch (_) {
    return src;
  }
  if (!img) return src;

  let changed = false;
  const worker = getBakeWorker();
  if (worker) {
    try {
      const back = await workerApply(img.data, w, h, filterString);
      // `img.data` was transferred: build a fresh ImageData over the reply.
      img = new ImageData(back, w, h);
      changed = true;
    } catch (_) {
      // The worker vanished mid-flight (its onerror already rejected this
      // promise): the transferred pixels are gone, so the bake degrades to
      // the unfiltered raster for this frame only — the page still renders,
      // and the worker failure is permanent (bakeWorkerFailed) so the next
      // bake goes through the inline kernel.
      return src;
    }
  } else {
    changed = applyFilterToData(img.data, w, h, filterString);
  }

  if (!changed) return src;
  if (src.width === 0 || src.height === 0) return src;

  // Always write to a scratch copy. Mutating `src` in place destroyed the
  // unbaked thumbnail raw, so the next theme change double-filtered and
  // live thumbs could not be rebaked without a pdf.js re-render.
  const out = acquireScratch(w, h);
  const octx = out.getContext("2d", { alpha: false });
  if (!octx) return src;
  octx.putImageData(img, 0, 0);
  return out;
}

// --- Raster helpers (unchanged semantics) -----------------------------------

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

export async function bakeRaster(
  src: HTMLCanvasElement,
  pipeline: PipelineCache,
): Promise<HTMLCanvasElement> {
  if (pipelineIsIdentity(pipeline)) return src;

  let filtered: HTMLCanvasElement = src;
  if (pipeline.filter !== "none") {
    filtered = (await applyFilterPixels(src, pipeline.filter)) ?? src;
  }
  if (pipeline.blend === "normal") return filtered;

  const out = acquirePooledCanvas(src.width, src.height);
  const octx = out.getContext("2d", { alpha: false });
  if (!octx) {
    return filtered;
  }
  octx.globalCompositeOperation = "source-over";
  octx.fillStyle = paperInfo(pipeline).color;
  octx.fillRect(0, 0, out.width, out.height);
  octx.globalCompositeOperation = pipeline.blend as GlobalCompositeOperation;
  octx.drawImage(filtered, 0, 0);
  octx.globalCompositeOperation = "source-over";
  if (filtered !== src && isSharedScratch(filtered)) {
    releaseScratch(filtered);
  } else if (filtered !== src) {
    releasePooledCanvas(filtered);
  }
  return out;
}

/** Paint a baked raster into `dst` and immediately free any intermediate. */
export async function bakeInto(
  dst: HTMLCanvasElement,
  src: HTMLCanvasElement,
  pipeline: PipelineCache,
  visibleTag?: RasterThemeTag,
): Promise<void> {
  const baked = await bakeRaster(src, pipeline);
  if (baked !== dst) {
    if (visibleTag) showBaked(dst, baked, visibleTag);
    else blitInto(dst, baked);
    if (baked !== src) {
      if (isSharedScratch(baked)) releaseScratch(baked);
      else releasePooledCanvas(baked);
    }
  }
  if (visibleTag) dst.classList.remove(visibleTag);
}
