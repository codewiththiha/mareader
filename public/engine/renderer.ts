// Page registration + canvas render + text/link layers.

import type {
  PageState,
  RenderResult,
} from "./types";
import { el, errorInfo, fail, releaseCanvas, releasePooledCanvas, showBaked } from "./canvas";
import { bakeRaster } from "./theme/bake";
import { pipelineIsIdentity, readPipeline } from "./theme/pipeline";
import {
  bumpRenderCount,
  CLEANUP_EVERY,
  pdf,
  releasePageSurfaces,
  themeScrubActive,
  stateByCanvasId,
  dropRawIfIdle,
  noteActivity,
  sweepPdf,
  PAGE_MAX_PIXELS,
} from "./state";
import { TextLayer } from "./loader";
import { applyHighlights } from "./highlights";
import { buildLinkLayer } from "./links";

function pageFromCanvasId(canvasId: string): number {
  const sp = /^sp-(\d+)-cv$/.exec(canvasId);
  if (sp && sp[1]) return parseInt(sp[1], 10);
  const cont = /^cont-(\d+)-cv$/.exec(canvasId);
  if (cont && cont[1]) return parseInt(cont[1], 10) + 1;
  return 1;
}

/** Look up or create PageState. Recovers when registerPage ran before the
 *  <canvas> was in the DOM (Leptos mounts the effect one tick early). */
function ensurePage(
  canvasId: string,
  pageHint?: number,
  hostIdHint?: string
): PageState | null {
  const existing = stateByCanvasId.get(canvasId);
  const canvas = el(canvasId) as HTMLCanvasElement | null;
  if (existing && existing.canvas && !existing.dead) {
    if (canvas && existing.canvas !== canvas) existing.canvas = canvas;
    return existing;
  }
  if (!canvas) return null;
  const hostId = hostIdHint || canvasId.replace(/-cv$/, "-pg");
  const host = el(hostId);
  const textLayerEl = host ? (host.querySelector(".textLayer") as HTMLElement | null) : null;
  if (existing) {
    existing.dead = false;
    existing.canvas = canvas;
    existing.host = host;
    existing.textLayerEl = textLayerEl;
    return existing;
  }
  const st: PageState = {
    page: pageHint && pageHint > 0 ? pageHint : pageFromCanvasId(canvasId),
    canvas,
    host,
    textLayerEl,
    renderTask: null,
    textLayer: null,
    viewport: null,
    scale: 1,
    dead: false,
    rawCanvas: null,
    queueGen: 0,
    queueHandle: 0,
  };
  stateByCanvasId.set(canvasId, st);
  return st;
}

export function registerPage(payload: { canvasId: string; hostId: string; page: number }): void {
  const existing = stateByCanvasId.get(payload.canvasId);
  if (existing) {
    existing.dead = true;
    try { existing.renderTask && existing.renderTask.cancel(); } catch (_) { /* ignore */ }
    try { existing.textLayer && existing.textLayer.cancel(); } catch (_) { /* ignore */ }
    if (existing.queueHandle) {
      cancelAnimationFrame(existing.queueHandle);
      existing.queueHandle = 0;
    }
  }
  const st = ensurePage(payload.canvasId, payload.page, payload.hostId);
  if (!st) {
    // Canvas not in the DOM yet. Remember the page/host so renderPage can
    // finish registration on the next tick.
    stateByCanvasId.set(payload.canvasId, {
      page: payload.page,
      canvas: null,
      host: payload.hostId ? el(payload.hostId) : null,
      textLayerEl: null,
      renderTask: null,
      textLayer: null,
      viewport: null,
      scale: 1,
      dead: false,
      rawCanvas: null,
      queueGen: 0,
      queueHandle: 0,
    });
  }
}

export function unregisterPage(canvasId: string): void {
  const st = stateByCanvasId.get(canvasId);
  if (st) {
    st.dead = true;
    try { st.renderTask && st.renderTask.cancel(); } catch (_) { /* ignore */ }
    try { st.textLayer && st.textLayer.cancel(); } catch (_) { /* ignore */ }
    if (st.queueHandle) {
      cancelAnimationFrame(st.queueHandle);
      st.queueHandle = 0;
    }
    releasePageSurfaces(st);
  }
  stateByCanvasId.delete(canvasId);
  sweepPdf();
}

export function cancelPage(canvasId: string): void {
  const st = stateByCanvasId.get(canvasId);
  if (st && st.renderTask) {
    try { st.renderTask.cancel(); } catch (_) { /* ignore */ }
    st.renderTask = null;
  }
}

function pageOutputScale(cssW: number, cssH: number): number {
  // Full native DPR for crisp text; PAGE_MAX_PIXELS is the memory guardrail.
  const dpr = globalThis.devicePixelRatio || 1;
  if (!(cssW > 0) || !(cssH > 0)) return dpr;

  // Cap so a single canvas never exceeds PAGE_MAX_PIXELS pixels.
  //
  // (The old code ALSO capped against one windowful of pixels —
  // `vw * vh * CANVAS_AREA_FACTOR` — which was the soft-text bug: a US Letter
  // page at 100% zoom on a 2x display needs 612*2 × 792*2 ≈ 1.48M pixels,
  // more than a 1440×900 window's 1.30M, so the render was throttled to
  // ~1.64x and the browser upscaled it. Dropping the window term lets a
  // single page use its full native resolution; the per-page ceiling below
  // bounds memory.)
  const capped = Math.sqrt(PAGE_MAX_PIXELS / (cssW * cssH));
  return Math.min(dpr, Math.max(0.5, capped));
}

export async function renderPageInternal(
  canvasId: string,
  scale: number,
  renderText: boolean
): Promise<RenderResult> {
  const st = ensurePage(canvasId);
  if (!st || !st.canvas) return fail("no_canvas", "Canvas element not found in DOM: " + canvasId);
  if (!pdf) return fail("no_document", "No document open");

  try { st.renderTask && st.renderTask.cancel(); } catch (_) { /* ignore */ }
  try { st.textLayer && st.textLayer.cancel(); } catch (_) { /* ignore */ }
  st.renderTask = null;
  st.textLayer = null;

  const page = await pdf.getPage(st.page);
  if (st.dead || !st.canvas) {
    try { page.cleanup(); } catch (_) { /* ignore */ }
    releasePageSurfaces(st);
    return fail("cancelled", "Render cancelled");
  }
  const viewport = page.getViewport({ scale });

  const cssW = Math.floor(viewport.width);
  const cssH = Math.floor(viewport.height);
  const out = pageOutputScale(cssW, cssH);
  const pxW = Math.max(1, Math.floor(viewport.width * out));
  const pxH = Math.max(1, Math.floor(viewport.height * out));

  const pipeline = themeScrubActive ? null : readPipeline();
  const needsBake = !themeScrubActive && pipeline ? !pipelineIsIdentity(pipeline) : false;
  const target = needsBake ? document.createElement("canvas") : st.canvas;
  target.width = pxW;
  target.height = pxH;
  const ctx = target.getContext("2d", { alpha: false });
  const transform = out !== 1 ? [out, 0, 0, out, 0, 0] : null;

  if (!ctx) {
    if (target !== st.canvas) releaseCanvas(target);
    return fail("no_context", "No 2d context");
  }
  const task = page.render({ canvasContext: ctx, viewport, transform });
  st.renderTask = task;
  try {
    await task.promise;
  } catch (e) {
    try { page.cleanup(); } catch (_) { /* ignore */ }
    if (target !== st.canvas) releaseCanvas(target);
    if (st.dead) releasePageSurfaces(st);
    if ((e as { name?: string }).name === "RenderingCancelledException") {
      return fail("cancelled", "Render cancelled");
    }
    const info = errorInfo(e);
    return fail(info.name, info.message);
  }
  if (st.dead) {
    try { page.cleanup(); } catch (_) { /* ignore */ }
    if (target !== st.canvas) releaseCanvas(target);
    releasePageSurfaces(st);
    return fail("cancelled", "Render cancelled");
  }

  if (needsBake && pipeline) {
    // Keep the unbaked `target` on the page. Slider scrub restores it and
    // lets live CSS filter/blend the raw pixels; dropping it made Dark
    // invert twice (flash to light) and Dim apply twice (go darker).
    const baked = bakeRaster(target, pipeline);
    if (baked !== st.canvas) {
      showBaked(st.canvas, baked, "canvas-raw");
      if (baked !== target) releasePooledCanvas(baked);
    }
    if (st.rawCanvas && st.rawCanvas !== st.canvas && st.rawCanvas !== target) {
      releaseCanvas(st.rawCanvas);
    }
    st.rawCanvas = target;
    st.canvas.classList.remove("canvas-raw");
    dropRawIfIdle(st);
  } else {
    // Identity / already scrubbing: the live canvas IS the raw.
    st.rawCanvas = st.canvas;
    st.canvas.classList.toggle("canvas-raw", themeScrubActive);
  }

  if (renderText && st.host && st.textLayerEl) {
    st.host.style.setProperty("--scale-factor", String(scale));

    const layer = document.createElement("div");
    layer.className = "textLayer";
    layer.setAttribute("aria-hidden", "true");

    const textContent = await page.getTextContent();

    const tl = TextLayer({
      textContentSource: textContent,
      container: layer,
      viewport,
    });
    st.textLayer = tl;
    try {
      await tl.render();
    } catch (e) {
      try { page.cleanup(); } catch (_) { /* ignore */ }
      if (st.dead) releasePageSurfaces(st);
      if ((e as { name?: string }).name === "AbortException") {
        return fail("cancelled", "Text render cancelled");
      }
      const info = errorInfo(e);
      return fail(info.name, info.message);
    }
    if (st.dead) {
      try { page.cleanup(); } catch (_) { /* ignore */ }
      releasePageSurfaces(st);
      return fail("cancelled", "Render cancelled");
    }

    const live = st.host.querySelector(".textLayer");
    if (live && live.parentNode) {
      live.replaceWith(layer);
    } else {
      st.host.appendChild(layer);
    }
    st.textLayerEl = layer;

    applyHighlights(st);

    await buildLinkLayer(st, viewport, page);
  }

  st.viewport = viewport;
  st.scale = scale;
  page.cleanup();

  if (bumpRenderCount() % CLEANUP_EVERY === 0) sweepPdf();
  noteActivity();

  return { ok: true, width: cssW, height: cssH, scale };
}

async function runLimited<T>(jobs: Array<() => Promise<T>>, limit = 2): Promise<T[]> {
  const out: T[] = [];
  let i = 0;
  const workers = Array.from(
    { length: Math.min(Math.max(limit, 1), Math.max(jobs.length, 1)) },
    async () => {
      while (i < jobs.length) {
        const idx = i;
        i += 1;
        const job = jobs[idx];
        if (job) out[idx] = await job();
      }
    },
  );
  await Promise.all(workers);
  return out;
}

export async function renderPage(
  canvasId: string,
  scale: number,
  renderText: boolean
): Promise<RenderResult> {
  let st = ensurePage(canvasId);
  if (!st || !st.canvas) {
    await new Promise<void>((r) => {
      requestAnimationFrame(() => r());
    });
    st = ensurePage(canvasId);
  }
  if (!st) return fail("no_canvas", "Canvas element not found in DOM: " + canvasId);
  if (!pdf) return fail("no_document", "No document open");

  const gen = (st.queueGen || 0) + 1;
  st.queueGen = gen;
  if (st.queueHandle) {
    cancelAnimationFrame(st.queueHandle);
    st.queueHandle = 0;
  }
  return await new Promise<RenderResult>((resolve) => {
    st.queueHandle = requestAnimationFrame(() => {
      st.queueHandle = 0;
      if (st.dead || st.queueGen !== gen) {
        resolve(fail("cancelled", "Render cancelled"));
        return;
      }
      try {
        renderPageInternal(canvasId, scale, !!renderText).then(resolve);
      } catch (e) {
        const info = errorInfo(e);
        resolve(fail(info.name, info.message));
      }
    });
  });
}

/** Re-render pages that have no unbaked raw so slider scrub can start
 *  without applying CSS filters on already-baked pixels. */
export async function preparePagesForScrub(): Promise<void> {
  const jobs: Array<() => Promise<unknown>> = [];
  for (const [id, st] of stateByCanvasId) {
    if (st.dead || !st.canvas) continue;
    if (st.rawCanvas && st.rawCanvas !== st.canvas) continue;
    if (!st.rawCanvas) {
      jobs.push(() => renderPageInternal(id, st.scale || 1, false));
    }
  }
  if (jobs.length) await runLimited(jobs, 2);
}

/** Re-render every live page from pdf.js. Used when a theme change arrives
 *  after we have already dropped the raw raster. */
export async function rerenderLivePages(): Promise<void> {
  const jobs: Array<() => Promise<unknown>> = [];
  for (const [id, st] of stateByCanvasId) {
    if (st.dead || !st.canvas) continue;
    jobs.push(() => renderPageInternal(id, st.scale || 1, !!st.textLayerEl));
  }
  await runLimited(jobs, 2);
}
