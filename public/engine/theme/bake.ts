// Raster baking: apply the CSS filter chain + paper blend to raw page
// pixels on the CPU. Hardware `ctx.filter` is not byte-identical across
// WebKit/Blink and would regress dark-mode inversion (the reason this
// baker exists). Intermediates recycle the shared scratch canvas so a bake
// never pins a second full-page buffer after it returns.

import type { FilterMatrix, PipelineCache } from "../types";
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

function filterTokenToMatrix(tok: string): FilterMatrix | null {
  const m = /^([a-z-]+)\(([^)]*)\)$/.exec(String(tok).trim());
  if (!m || !m[1] || !m[2]) return null;
  const name = m[1];
  const arg = parseFloat(m[2]);
  if (!Number.isFinite(arg)) return null;
  switch (name) {
    case "invert": {
      const k = 1 - 2 * arg;
      return { m: [k, 0, 0, 0, k, 0, 0, 0, k], o: [arg, arg, arg] };
    }
    case "brightness":
      return { m: [arg, 0, 0, 0, arg, 0, 0, 0, arg], o: [0, 0, 0] };
    case "contrast": {
      const off = 0.5 * (1 - arg);
      return { m: [arg, 0, 0, 0, arg, 0, 0, 0, arg], o: [off, off, off] };
    }
    case "saturate": {
      const t = 1 - arg;
      const a = 0.213 * t;
      const b = 0.715 * t;
      const c = 0.072 * t;
      return {
        m: [a + arg, b, c, a, b + arg, c, a, b, c + arg],
        o: [0, 0, 0],
      };
    }
    case "sepia": {
      const S = [0.393, 0.769, 0.189, 0.349, 0.686, 0.168, 0.272, 0.534, 0.131];
      const out: number[] = [];
      for (let i = 0; i < 9; i += 1) {
        const ident = i === 0 || i === 4 || i === 8 ? 1 : 0;
        out.push((1 - arg) * ident + arg * (S[i] ?? 0));
      }
      return { m: out, o: [0, 0, 0] };
    }
    case "hue-rotate": {
      const th = (arg * Math.PI) / 180;
      const c = Math.cos(th);
      const s = Math.sin(th);
      return {
        m: [
          0.213 + 0.787 * c - 0.213 * s,
          0.715 - 0.715 * c - 0.715 * s,
          0.072 - 0.072 * c + 0.928 * s,
          0.213 - 0.213 * c + 0.143 * s,
          0.715 + 0.285 * c + 0.140 * s,
          0.072 - 0.072 * c - 0.283 * s,
          0.213 - 0.213 * c - 0.787 * s,
          0.715 - 0.715 * c + 0.715 * s,
          0.072 + 0.928 * c + 0.072 * s,
        ],
        o: [0, 0, 0],
      };
    }
    default:
      return null;
  }
}
function composeFilter(filterString: string): FilterMatrix {
  let m = [1, 0, 0, 0, 1, 0, 0, 0, 1];
  let o = [0, 0, 0];
  for (const tok of String(filterString).split(/\s+/)) {
    if (!tok) continue;
    const op = filterTokenToMatrix(tok);
    if (!op) continue;
    const nm: number[] = [];
    const no: number[] = [];
    for (let r = 0; r < 3; r += 1) {
      nm[r * 3] =
        op.m[r * 3] * m[0] + op.m[r * 3 + 1] * m[3] + op.m[r * 3 + 2] * m[6];
      nm[r * 3 + 1] =
        op.m[r * 3] * m[1] + op.m[r * 3 + 1] * m[4] + op.m[r * 3 + 2] * m[7];
      nm[r * 3 + 2] =
        op.m[r * 3] * m[2] + op.m[r * 3 + 1] * m[5] + op.m[r * 3 + 2] * m[8];
      no[r] =
        op.m[r * 3] * o[0] + op.m[r * 3 + 1] * o[1] + op.m[r * 3 + 2] * o[2] + op.o[r];
    }
    m = nm;
    o = no;
  }
  return { m, o };
}
function applyFilterPixels(
  src: HTMLCanvasElement,
  filterString: string
): HTMLCanvasElement {
  const { m, o } = composeFilter(filterString);
  const identity =
    m[0] === 1 && m[1] === 0 && m[2] === 0 &&
    m[3] === 0 && m[4] === 1 && m[5] === 0 &&
    m[6] === 0 && m[7] === 0 && m[8] === 1 &&
    o[0] === 0 && o[1] === 0 && o[2] === 0;
  if (identity) return src;

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

  const SCALE = 1 << 16;
  const luts: Int32Array[] = new Array(9);
  for (let i = 0; i < 9; i += 1) {
    const coef = m[i] ?? 0;
    const lut = new Int32Array(256);
    for (let v = 0; v < 256; v += 1) lut[v] = Math.round(coef * v * SCALE);
    luts[i] = lut;
  }
  const o0 = Math.round((o[0] ?? 0) * 255 * SCALE);
  const o1 = Math.round((o[1] ?? 0) * 255 * SCALE);
  const o2 = Math.round((o[2] ?? 0) * 255 * SCALE);
  const L0 = luts[0]!, L1 = luts[1]!, L2 = luts[2]!;
  const L3 = luts[3]!, L4 = luts[4]!, L5 = luts[5]!;
  const L6 = luts[6]!, L7 = luts[7]!, L8 = luts[8]!;
  const d = img.data;
  for (let i = 0; i < d.length; i += 4) {
    const r = d[i]!;
    const g = d[i + 1]!;
    const b = d[i + 2]!;
    d[i] = (L0[r] + L1[g] + L2[b] + o0) >> 16;
    d[i + 1] = (L3[r] + L4[g] + L5[b] + o1) >> 16;
    d[i + 2] = (L6[r] + L7[g] + L8[b] + o2) >> 16;
  }

  // Always write to a scratch copy. Mutating `src` in place destroyed the
  // unbaked thumbnail raw, so the next theme change double-filtered and
  // live thumbs could not be rebaked without a pdf.js re-render.
  const out = acquireScratch(w, h);
  const octx = out.getContext("2d", { alpha: false });
  if (!octx) return src;
  octx.putImageData(img, 0, 0);
  return out;
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
export function bakeRaster(
  src: HTMLCanvasElement,
  pipeline: PipelineCache
): HTMLCanvasElement {
  if (pipelineIsIdentity(pipeline)) return src;

  let filtered: HTMLCanvasElement = src;
  if (pipeline.filter !== "none") {
    filtered = applyFilterPixels(src, pipeline.filter);
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
export function bakeInto(
  dst: HTMLCanvasElement,
  src: HTMLCanvasElement,
  pipeline: PipelineCache,
  visibleTag?: RasterThemeTag,
): void {
  const baked = bakeRaster(src, pipeline);
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
