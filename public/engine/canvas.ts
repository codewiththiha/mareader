// Canvas backing-store helpers: a small pool plus one scratch pad.

import { SESSION_ATTR } from "./dom-contract";
import type { MaybeCanvas, Raster } from "./types";

/** Force the browser to drop a canvas backing store. */
export function releaseCanvas(canvas: MaybeCanvas): void {
  if (!canvas) return;
  const c = canvas as HTMLCanvasElement;
  try {
    // Browsers keep the GPU texture until both dimensions are 0.
    c.width = 0;
    c.height = 0;
    const ctx = typeof c.getContext === "function" ? c.getContext("2d") : null;
    if (ctx) ctx.clearRect(0, 0, 0, 0);
  } catch (_) {
    /* detached / already gone */
  }
}

const canvasPool: HTMLCanvasElement[] = [];
const POOL_MAX = 6;

export function acquirePooledCanvas(w: number, h: number): HTMLCanvasElement {
  let c = canvasPool.pop();
  // Guard against retaining an oversized texture: the pool caps COUNT.
  if (c && c.width * c.height > 2 * Math.max(1, Math.floor(w)) * Math.max(1, Math.floor(h))) {
    releaseCanvas(c);
    c = undefined;
  }
  if (!c) c = document.createElement("canvas");
  c.width = Math.max(1, Math.floor(w));
  c.height = Math.max(1, Math.floor(h));
  return c;
}

// A fresh offscreen canvas sized to a viewport, with its 2d context.
export function offscreenFor(
  viewport: { width: number; height: number },
  scale = 1
): { canvas: HTMLCanvasElement; ctx: CanvasRenderingContext2D } | null {
  const canvas = document.createElement("canvas");
  canvas.width = Math.max(1, Math.floor(viewport.width * scale));
  canvas.height = Math.max(1, Math.floor(viewport.height * scale));
  const ctx = canvas.getContext("2d", { alpha: false });
  if (!ctx) {
    releaseCanvas(canvas);
    return null;
  }
  return { canvas, ctx };
}

export function releasePooledCanvas(c: HTMLCanvasElement | null | undefined): void {
  if (!c || typeof c.getContext !== "function") return;
  if (canvasPool.length < POOL_MAX) {
    c.width = 1;
    c.height = 1;
    canvasPool.push(c);
    return;
  }
  releaseCanvas(c);
}

let scratch: HTMLCanvasElement | null = null;
let scratchInUse = false;

// The scratch is a free/held tri-state: one owner, others pooled.

// Borrow the shared bake scratchpad; concurrent callers get a pooled one.
export function acquireScratch(w: number, h: number): HTMLCanvasElement {
  if (scratchInUse) {
    return acquirePooledCanvas(w, h);
  }
  if (!scratch) scratch = document.createElement("canvas");
  scratch.width = Math.max(1, Math.floor(w));
  scratch.height = Math.max(1, Math.floor(h));
  scratchInUse = true;
  return scratch;
}

export function releaseScratch(owned?: HTMLCanvasElement | null): void {
  if (owned && owned !== scratch) {
    releasePooledCanvas(owned);
    return;
  }
  scratchInUse = false;
}

export function isSharedScratch(c: HTMLCanvasElement | null | undefined): boolean {
  return !!c && c === scratch;
}

// Estimated bytes the recycler holds now: pooled canvases plus scratch.
export function pooledIntermediateBytesEstimate(): number {
  let bytes = 0;
  for (const c of canvasPool) bytes += c.width * c.height * 4;
  if (scratch) bytes += scratch.width * scratch.height * 4;
  return bytes;
}

/** Drop the scratch backing store entirely (document teardown). */
export function disposeScratch(): void {
  if (scratch && !scratchInUse) {
    releaseCanvas(scratch);
    scratch = null;
  }
  while (canvasPool.length > 0) {
    releaseCanvas(canvasPool.pop() ?? null);
  }
}

export function blitInto(
  dst: HTMLCanvasElement | null,
  src: Raster | null
): boolean {
  if (!dst || !src) return false;
  const srcW = (src as ImageBitmap).width ?? (src as HTMLCanvasElement).width;
  const srcH = (src as ImageBitmap).height ?? (src as HTMLCanvasElement).height;
  if (!(srcW > 0) || !(srcH > 0)) return false;
  if (dst.width !== srcW || dst.height !== srcH) {
    dst.width = srcW;
    dst.height = srcH;
  }
  const ctx = dst.getContext("2d", { alpha: false });
  if (!ctx) return false;
  ctx.drawImage(src as CanvasImageSource, 0, 0);
  return true;
}


export type RasterThemeTag = "canvas-raw" | "thumb-raw";

/** Paint a raw raster and mark that exact visible canvas for live theming. */
export function showRaw(dst: HTMLCanvasElement | null, raw: Raster | null, tag: RasterThemeTag): boolean {
  const shown = blitInto(dst, raw);
  if (shown) dst!.classList.add(tag);
  return shown;
}

// Paint a baked raster and clear the raw marker in one turn.
export function showBaked(
  dst: HTMLCanvasElement | null,
  baked: Raster | null,
  tag: RasterThemeTag,
): boolean {
  const shown = blitInto(dst, baked);
  if (shown) dst!.classList.remove(tag);
  return shown;
}

export function el(id: string): HTMLElement | null {
  if (typeof id !== "string" || !id) return null;
  return document.getElementById(id);
}

// Whether `node` may be painted by session `sid`.
export function ownedBy(node: Element | null, sid: number): boolean {
  if (!node) return false;
  const owner = typeof node.getAttribute === "function" ? node.getAttribute(SESSION_ATTR) : null;
  return owner === null || owner === String(sid);
}

// The element with `id` that belongs to session `sid`.
export function sessionEl(sid: number, id: string): HTMLElement | null {
  const first = el(id);
  if (!first || ownedBy(first, sid)) return first;
  try {
    return document.querySelector(`[id="${id}"][${SESSION_ATTR}="${sid}"]`) as HTMLElement | null;
  } catch (_) {
    return null;
  }
}
