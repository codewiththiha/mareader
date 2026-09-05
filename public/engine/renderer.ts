// Page registration + canvas render + text/link layers.

import type {
  PageState,
  RenderResult,
} from "./types";
import { el, fail, failFrom, releaseCanvas, releasePooledCanvas, showBaked } from "./canvas";
import { stashPaperFrame } from "./paper";
import { bakeRaster } from "./theme/bake";
import { pipelineIsIdentity, readPipeline } from "./theme/pipeline";
import { CLEANUP_EVERY, PAGE_MAX_PIXELS, session } from "./state";
import {
  hostIdFromCanvasId,
  pageFromCanvasId,
  TEXT_LAYER_CLASS,
  TEXT_LAYER_SELECTOR,
} from "./dom-contract";
import { TextLayer } from "./loader";
import { applyHighlights } from "./highlights";
import { buildLinkLayer } from "./links";

/** A page with nothing in flight: no render task, no text layer, no viewport,
 *  no raw raster, and both queue counters at zero. Two callers build one — a
 *  canvas found in the DOM, and a page registered before its canvas exists —
 *  and a field added to `PageState` should have exactly one place to be given
 *  its initial value. */
function blankPage(
  page: number,
  canvas: HTMLCanvasElement | null,
  host: HTMLElement | null,
  textLayerEl: HTMLElement | null
): PageState {
  return {
    page,
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
}

/** Look up or create PageState. Recovers when registerPage ran before the
 *  <canvas> was in the DOM (Leptos mounts the effect one tick early). */
function ensurePage(
  canvasId: string,
  pageHint?: number,
  hostIdHint?: string
): PageState | null {
  const existing = session.stateByCanvasId.get(canvasId);
  const canvas = el(canvasId) as HTMLCanvasElement | null;
  if (existing && existing.canvas && !existing.dead) {
    if (canvas && existing.canvas !== canvas) existing.canvas = canvas;
    return existing;
  }
  if (!canvas) return null;
  const hostId = hostIdHint || hostIdFromCanvasId(canvasId);
  const host = el(hostId);
  const textLayerEl = host ? (host.querySelector(TEXT_LAYER_SELECTOR) as HTMLElement | null) : null;
  if (existing) {
    existing.dead = false;
    existing.canvas = canvas;
    existing.host = host;
    existing.textLayerEl = textLayerEl;
    return existing;
  }
  // Prefer the caller's hint (registerPage passes the page number); fall back to
  // parsing the id only when the mount never registered. An id this cannot parse
  // is not a reader host at all, and page 1 is the least wrong guess for a
  // canvas that is about to be told which page it is.
  const page = pageHint && pageHint > 0 ? pageHint : (pageFromCanvasId(canvasId) ?? 1);
  const st = blankPage(page, canvas, host, textLayerEl);
  session.stateByCanvasId.set(canvasId, st);
  return st;
}

export function registerPage(page: number, canvasId: string, hostId?: string): void {
  const existing = session.stateByCanvasId.get(canvasId);
  if (existing) {
    existing.dead = true;
    try { existing.renderTask && existing.renderTask.cancel(); } catch (_) { /* ignore */ }
    try { existing.textLayer && existing.textLayer.cancel(); } catch (_) { /* ignore */ }
    if (existing.queueHandle) {
      cancelAnimationFrame(existing.queueHandle);
      existing.queueHandle = 0;
    }
  }
  const st = ensurePage(canvasId, page, hostId);
  if (!st) {
    // Canvas not in the DOM yet. Remember the page/host so renderPage can
    // finish registration on the next tick.
    session.stateByCanvasId.set(
      canvasId,
      blankPage(page, null, hostId ? el(hostId) : null, null)
    );
  }
}

export function unregisterPage(canvasId: string): void {
  const st = session.stateByCanvasId.get(canvasId);
  if (st) {
    st.dead = true;
    try { st.renderTask && st.renderTask.cancel(); } catch (_) { /* ignore */ }
    try { st.textLayer && st.textLayer.cancel(); } catch (_) { /* ignore */ }
    if (st.queueHandle) {
      cancelAnimationFrame(st.queueHandle);
      st.queueHandle = 0;
    }
    session.releasePageSurfaces(st);
  }
  session.stateByCanvasId.delete(canvasId);
  session.sweepPdf();
}

export function cancelPage(canvasId: string): void {
  const st = session.stateByCanvasId.get(canvasId);
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
  if (!session.pdf) return fail("no_document", "No document open");

  try { st.renderTask && st.renderTask.cancel(); } catch (_) { /* ignore */ }
  try { st.textLayer && st.textLayer.cancel(); } catch (_) { /* ignore */ }
  st.renderTask = null;
  st.textLayer = null;

  const page = await session.pdf.getPage(st.page);
  if (st.dead || !st.canvas) {
    try { page.cleanup(); } catch (_) { /* ignore */ }
    session.releasePageSurfaces(st);
    return fail("cancelled", "Render cancelled");
  }
  const viewport = page.getViewport({ scale });

  const cssW = Math.floor(viewport.width);
  const cssH = Math.floor(viewport.height);
  const out = pageOutputScale(cssW, cssH);
  const pxW = Math.max(1, Math.floor(viewport.width * out));
  const pxH = Math.max(1, Math.floor(viewport.height * out));

  // Where the render draws: a scratch when the pipeline in force at start is
  // non-identity (the visible canvas keeps its baked copy until the swap),
  // the live canvas otherwise. pdf.js needs the destination NOW, so this
  // half is start-time; the THEME decision itself is re-made at completion
  // (see the generation guard below) — a render that spans a pipeline
  // change must not bake against the palette it started under.
  const pipeline0 = session.themeScrubActive ? null : readPipeline();
  const needsBake0 = !session.themeScrubActive && pipeline0 ? !pipelineIsIdentity(pipeline0) : false;
  const target = needsBake0 ? document.createElement("canvas") : st.canvas;
  target.width = pxW;
  target.height = pxH;
  const ctx = target.getContext("2d", { alpha: false });
  const transform = out !== 1 ? [out, 0, 0, out, 0, 0] : null;

  if (!ctx) {
    if (target !== st.canvas) releaseCanvas(target);
    return fail("no_context", "No 2d context");
  }
  // The text-extraction worker round trip is independent of the raster path:
  // start it before rendering so the two overlap instead of paying
  // getTextContent serially after the paint. A text failure degrades to a
  // raster-only page (never to a failed render).
  const textTask =
    renderText && st.host && st.textLayerEl ? page.getTextContent().catch(() => null) : null;
  const task = page.render({ canvasContext: ctx, viewport, transform });
  st.renderTask = task;
  try {
    await task.promise;
  } catch (e) {
    // The raster is dead: the orphaned text extraction was already made
    // infallible at creation time, so nothing can leak here.
    try { page.cleanup(); } catch (_) { /* ignore */ }
    if (target !== st.canvas) releaseCanvas(target);
    if (st.dead) session.releasePageSurfaces(st);
    if ((e as { name?: string }).name === "RenderingCancelledException") {
      return fail("cancelled", "Render cancelled");
    }
    return failFrom(e);
  }
  if (st.dead) {
    try { page.cleanup(); } catch (_) { /* ignore */ }
    if (target !== st.canvas) releaseCanvas(target);
    session.releasePageSurfaces(st);
    return fail("cancelled", "Render cancelled");
  }

  // `target` still holds raw pixels here (bakeRaster runs below): the one
  // point in the pipeline where the document's own paper is intact. Park a
  // ≤96×96 frame for the Rust paper session to drain after the render —
  // every colour decision downstream lives in the pdf-paper crate.
  stashPaperFrame(canvasId, st.page, target);

  // GENERATION GUARD: settle under the pipeline CURRENT at landing, not the
  // one in force when the render was issued. `readPipeline()` caches by the
  // root style token, so a Rust appearance repaint (which bumps the cache
  // generation and re-bakes through the theme queue) or a scrub / pipeline
  // flip can land while this raster is still in flight; page renders are
  // NOT serialized with the theme queue, so a spread's two pages — issued a
  // beat apart — used to be able to bake against different theme states, or
  // land one on `canvas-raw` and its sibling baked, which is exactly the
  // half-theme seam. The raw pixels are in `target` either way, so the
  // decision is free to move to here.
  const pipeline = session.themeScrubActive ? null : readPipeline();
  const needsBake = pipeline ? !pipelineIsIdentity(pipeline) : false;

  if (needsBake && pipeline) {
    // Keep the unbaked `target` on the page. Slider scrub restores it and
    // lets live CSS filter/blend the raw pixels; dropping it made Dark
    // invert twice (flash to light) and Dim apply twice (go darker).
    const baked = await bakeRaster(target, pipeline);
    if (baked !== st.canvas) {
      showBaked(st.canvas, baked, "canvas-raw");
      if (baked !== target) releasePooledCanvas(baked);
    }
    if (st.rawCanvas && st.rawCanvas !== st.canvas && st.rawCanvas !== target) {
      releaseCanvas(st.rawCanvas);
    }
    st.rawCanvas = target;
    st.canvas.classList.remove("canvas-raw");
    session.dropRawIfIdle(st);
  } else {
    // Identity / already scrubbing: the live canvas IS the raw.
    st.rawCanvas = st.canvas;
    st.canvas.classList.toggle("canvas-raw", session.themeScrubActive);
  }

  if (renderText && st.host && st.textLayerEl) {
    st.host.style.setProperty("--scale-factor", String(scale));

    const layer = document.createElement("div");
    layer.className = TEXT_LAYER_CLASS;
    layer.setAttribute("aria-hidden", "true");

    const textContent = await textTask;
    if (!textContent) return fail("no_text", "Text extraction failed for page " + st.page);

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
      if (st.dead) session.releasePageSurfaces(st);
      if ((e as { name?: string }).name === "AbortException") {
        return fail("cancelled", "Text render cancelled");
      }
      return failFrom(e);
    }
    if (st.dead) {
      try { page.cleanup(); } catch (_) { /* ignore */ }
      session.releasePageSurfaces(st);
      return fail("cancelled", "Render cancelled");
    }

    const live = st.host.querySelector(TEXT_LAYER_SELECTOR);
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

  if (session.bumpRenderCount() % CLEANUP_EVERY === 0) session.sweepPdf();
  session.noteActivity();

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
  if (!session.pdf) return fail("no_document", "No document open");

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
        resolve(failFrom(e));
      }
    });
  });
}

/** Re-render pages that have no unbaked raw so slider scrub can start
 *  without applying CSS filters on already-baked pixels. */
export async function preparePagesForScrub(): Promise<void> {
  const jobs: Array<() => Promise<unknown>> = [];
  for (const [id, st] of session.stateByCanvasId) {
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
  for (const [id, st] of session.stateByCanvasId) {
    if (st.dead || !st.canvas) continue;
    jobs.push(() => renderPageInternal(id, st.scale || 1, !!st.textLayerEl));
  }
  await runLimited(jobs, 2);
}
