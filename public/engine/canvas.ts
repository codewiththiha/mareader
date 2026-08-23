// Canvas backing-store helpers. A small pool + one scratch pad recycle
// bake intermediates so theme changes do not allocate a new full-page
// RGBA buffer on every page/thumb.

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
  if (!c) c = document.createElement("canvas");
  c.width = Math.max(1, Math.floor(w));
  c.height = Math.max(1, Math.floor(h));
  return c;
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

/** Borrow the shared bake scratchpad. Concurrent callers get a pooled canvas. */
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

/** Paint a baked raster and clear the raw marker in the same synchronous turn. */
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

export function fail(name: string, message: string): { ok: false; error: { name: string; message: string } } {
  return { ok: false, error: { name, message } };
}

export function errorInfo(e: unknown): { name: string; message: string } {
  const er = e as { name?: string; message?: string } | null;
  const name = (er && er.name) || "Error";
  const message = (er && er.message) || String(e);
  return { name, message };
}
