// Theme pipeline: read CSS variables, bake filter+blend into a raster.
//
// Pixel math stays CPU-side and deterministic. Hardware `ctx.filter` is not
// byte-identical across WebKit/Blink and would regress dark-mode inversion
// (the reason this baker exists). Intermediates recycle a shared scratch
// canvas so a bake never pins a second full-page buffer after it returns.

import type { FilterMatrix, PaperInfo, PipelineCache, ThumbEntry, MaybeCanvas } from "./types";
import {
  acquirePooledCanvas,
  acquireScratch,
  blitInto,
  el,
  isSharedScratch,
  releasePooledCanvas,
  releaseScratch,
} from "./canvas";
import {
  dropRawIfIdle,
  setScrubbing,
  scrubbing,
  stateByCanvasId,
  thumbCache,
  thumbLive,
} from "./state";

export const pipelineCache: PipelineCache = {
  token: null,
  filter: "none",
  blend: "normal",
  paperInfo: null,
  gen: 0,
};

export function invalidatePipeline(): void {
  pipelineCache.token = null;
  pipelineCache.gen += 1;
}

export function readPipeline(): PipelineCache {
  const root = document.documentElement;
  const token = root.getAttribute("style") || "";
  if (pipelineCache.token === token) return pipelineCache;
  let filter = "none";
  let blend = "normal";
  try {
    const cs = getComputedStyle(root);
    filter = (cs.getPropertyValue("--canvas-filter") || "none").trim() || "none";
    blend = (cs.getPropertyValue("--canvas-blend") || "normal").trim() || "normal";
  } catch (_) {
    /* identity */
  }
  pipelineCache.token = token;
  pipelineCache.filter = filter;
  pipelineCache.blend = blend;
  pipelineCache.paperInfo = null;
  pipelineCache.gen += 1;
  return pipelineCache;
}

export function paperInfo(pipeline: PipelineCache): PaperInfo {
  if (pipeline.paperInfo) return pipeline.paperInfo;
  const info: PaperInfo = { color: "#ffffff", rgb: [255, 255, 255] };
  try {
    const probe = document.createElement("div");
    probe.style.cssText = "display:none;background-color:var(--color-paper,#ffffff)";
    document.documentElement.appendChild(probe);
    const resolved = getComputedStyle(probe).backgroundColor;
    probe.remove();
    if (resolved && resolved !== "rgba(0, 0, 0, 0)") {
      info.color = resolved;
      const c = acquireScratch(1, 1);
      const ctx = c.getContext("2d");
      if (ctx) {
        ctx.fillStyle = resolved;
        ctx.fillRect(0, 0, 1, 1);
        const d = ctx.getImageData(0, 0, 1, 1).data;
        info.rgb = [d[0] ?? 255, d[1] ?? 255, d[2] ?? 255];
      }
      releaseScratch(c);
    }
  } catch (_) {
    /* white paper */
  }
  pipeline.paperInfo = info;
  return info;
}

export function pipelineIsIdentity(pipeline: PipelineCache): boolean {
  if (pipeline.filter !== "none") return false;
  if (pipeline.blend === "normal") return true;
  if (pipeline.blend === "multiply") {
    const rgb = paperInfo(pipeline).rgb;
    return rgb[0] === 255 && rgb[1] === 255 && rgb[2] === 255;
  }
  return false;
}

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

let filterLuts: Int32Array[] | null = null;

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

function rasterToCanvas(src: HTMLCanvasElement | ImageBitmap): {
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
  pipeline: PipelineCache
): void {
  const baked = bakeRaster(src, pipeline);
  if (baked !== dst) {
    blitInto(dst, baked);
    if (baked !== src) {
      if (isSharedScratch(baked)) releaseScratch(baked);
      else releasePooledCanvas(baked);
    }
  }
}

export function releaseDisplayOnly(entry: ThumbEntry | null): void {
  if (!entry) return;
  try {
    if (entry.display && typeof (entry.display as ImageBitmap).close === "function") {
      (entry.display as ImageBitmap).close();
    }
  } catch (_) {
    /* already closed */
  }
  if (entry.display && entry.display !== entry.raw) {
    releasePooledCanvas(entry.display as HTMLCanvasElement);
  }
  entry.display = null;
}

export async function cacheDisplay(entry: Pick<ThumbEntry, "display">): Promise<MaybeCanvas> {
  const off = entry.display;
  if (!off || typeof createImageBitmap !== "function") return off;
  try {
    const bitmap = await createImageBitmap(off as ImageBitmap);
    if (isSharedScratch(off as HTMLCanvasElement)) releaseScratch(off as HTMLCanvasElement);
    else releasePooledCanvas(off as HTMLCanvasElement);
    return bitmap;
  } catch (_) {
    return off;
  }
}

export function thumbSource(entry: ThumbEntry | null | undefined): MaybeCanvas {
  if (!entry) return null;
  if (entry.display && (entry.display as ImageBitmap).width > 0) return entry.display;
  if (entry.raw && (entry.raw as ImageBitmap).width > 0) return entry.raw;
  return null;
}

function rasterWidth(src: MaybeCanvas): number {
  return src ? ((src as ImageBitmap).width || 0) : 0;
}

export function thumbRaw(entry: ThumbEntry | null | undefined): MaybeCanvas {
  // Never fall back to `display`: that raster is already themed, and baking
  // it again double-applies invert/blend.
  if (!entry) return null;
  if (entry.raw && rasterWidth(entry.raw) > 0) return entry.raw;
  return null;
}

async function snapshotRaster(src: HTMLCanvasElement): Promise<MaybeCanvas> {
  if (typeof createImageBitmap === "function") {
    try {
      return await createImageBitmap(src);
    } catch (_) {
      /* fall through */
    }
  }
  const clone = acquirePooledCanvas(src.width, src.height);
  blitInto(clone, src);
  return clone;
}

export async function ensureEntryCurrent(entry: ThumbEntry): Promise<MaybeCanvas> {
  if (scrubbing) {
    return rasterWidth(entry.display) > 0 ? entry.display : null;
  }
  if (entry.gen === pipelineCache.gen && rasterWidth(entry.display) > 0) {
    return entry.display;
  }
  if (entry.pending) return await entry.pending;
  entry.pending = (async () => {
    const raw = thumbRaw(entry);
    if (!raw) return null;
    const pipeline = readPipeline();
    const { canvas: src, borrowed } = rasterToCanvas(
      raw as HTMLCanvasElement | ImageBitmap,
    );
    let work = src;
    let owned = borrowed;
    if (!borrowed) {
      work = acquirePooledCanvas(src.width, src.height);
      blitInto(work, src);
      owned = true;
    }
    const baked = bakeRaster(work, pipeline);
    let newDisplay: MaybeCanvas;
    if (baked === work) {
      newDisplay = await snapshotRaster(work);
      if (owned) releasePooledCanvas(work);
    } else {
      if (owned) releasePooledCanvas(work);
      newDisplay = await cacheDisplay({ display: baked });
    }
    if (entry.display && entry.display !== entry.raw && entry.display !== newDisplay) {
      releaseDisplayOnly(entry);
    }
    entry.display = newDisplay;
    entry.gen = pipelineCache.gen;
    return entry.display;
  })();
  const result = await entry.pending;
  entry.pending = null;
  return result;
}

export async function rebakeTheme(): Promise<void> {
  try {
    await refreshThemeInternal();
  } catch (e) {
    const msg = (e as { message?: string })?.message ?? e;
    console.warn("[pdfEngine] refreshTheme failed:", msg);
  }
}

/** Blit every cached thumb onto its live DOM canvas (and any stray `.thumb-canvas`). */
export function paintAllVisibleThumbs(): void {
  const seen = new Set<string>();
  for (const [canvasId, { page }] of thumbLive) {
    seen.add(canvasId);
    const entry = thumbCache.get(page);
    const live = el(canvasId) as HTMLCanvasElement | null;
    if (entry && live) paintCached(live, entry);
  }
  try {
    const nodes = document.querySelectorAll("canvas.thumb-canvas");
    for (let i = 0; i < nodes.length; i += 1) {
      const live = nodes[i] as HTMLCanvasElement;
      if (!live.id || seen.has(live.id)) continue;
      const m = /^thumb-(\d+)$/.exec(live.id);
      if (!m || !m[1]) continue;
      const page = parseInt(m[1], 10);
      const entry = thumbCache.get(page);
      if (!entry) continue;
      paintCached(live, entry);
      thumbLive.set(live.id, { page });
    }
  } catch (_) {
    /* no document */
  }
}

async function refreshThemeInternal(): Promise<void> {
  if (scrubbing) return;
  const pipeline = readPipeline();

  for (const st of stateByCanvasId.values()) {
    // Only re-bake from a DISTINCT raw raster. If raw === live canvas the
    // pixels may already be themed; baking again double-filters.
    if (st.dead || !st.canvas || !st.rawCanvas || st.rawCanvas === st.canvas) continue;
    bakeInto(st.canvas, st.rawCanvas, pipeline);
    dropRawIfIdle(st);
  }

  for (const entry of thumbCache.values()) {
    await ensureEntryCurrent(entry);
    if (scrubbing) return;
  }

  paintAllVisibleThumbs();
}

export function paintCached(
  dst: HTMLCanvasElement | null,
  entry: ThumbEntry | null
): { width: number; height: number } | null {
  const src = thumbSource(entry);
  if (!dst || !src) return null;
  const srcW = (src as ImageBitmap).width;
  const srcH = (src as ImageBitmap).height;
  dst.width = srcW;
  dst.height = srcH;
  const ctx = dst.getContext("2d", { alpha: false });
  if (!ctx) return null;
  ctx.drawImage(src as CanvasImageSource, 0, 0);
  return { width: entry!.cssW, height: entry!.cssH };
}

export async function applyScrubMode(on: boolean): Promise<void> {
  try {
    await setScrubModeInternal(on);
  } catch (e) {
    const msg = (e as { message?: string })?.message ?? e;
    console.warn("[pdfEngine] setScrubMode failed:", msg);
  }
}

async function setScrubModeInternal(on: boolean): Promise<void> {
  if (on) {
    setScrubbing(true);
    // Restore UNBAKED pixels first. Adding the CSS class while the canvas
    // still holds a baked Dark/Dim raster double-applies invert/brightness
    // (the "flash to light" / "goes dimmer" glitch).
    for (const st of stateByCanvasId.values()) {
      if (st.dead || !st.canvas) continue;
      const raw = st.rawCanvas;
      if (raw && raw !== st.canvas) blitInto(st.canvas, raw);
    }
    for (const [canvasId, { page }] of thumbLive) {
      const entry = thumbCache.get(page);
      const live = el(canvasId) as HTMLCanvasElement | null;
      const raw = entry ? thumbRaw(entry) : null;
      if (live && raw) blitInto(live, raw);
    }
    document.documentElement.classList.add("appearance-scrubbing");
    return;
  }

  if (!scrubbing) return;
  setScrubbing(false);

  const pipeline = readPipeline();
  for (const [canvasId, { page }] of thumbLive) {
    const entry = thumbCache.get(page);
    if (!entry) continue;
    if (!(await ensureEntryCurrent(entry))) continue;
    if (scrubbing) return;
  }
  for (const st of stateByCanvasId.values()) {
    if (st.dead || !st.canvas) continue;
    const raw = st.rawCanvas;
    if (!raw) continue;
    bakeInto(st.canvas, raw, pipeline);
    dropRawIfIdle(st);
  }
  paintAllVisibleThumbs();
  document.documentElement.classList.remove("appearance-scrubbing");
}
