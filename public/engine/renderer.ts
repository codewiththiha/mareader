// Page registration + canvas render + text/link layers.

import type {
  PageState,
  RenderResult,
} from "./types";
import { el, isSharedScratch, releaseCanvas, releasePooledCanvas, releaseScratch, showBaked } from "./canvas";
import { fail, failFrom } from "./errors";
import { stashPaperFrame } from "./paper";
import { bakeRaster } from "./theme/bake";
import { pipelineIsIdentity, readPipeline } from "./theme/pipeline";
import { CLEANUP_EVERY, PAGE_MAX_PIXELS, session } from "./state";
import { currentBudget } from "./motion";
import { reclaim } from "./memory";
import { schedule, unschedule } from "./scheduler";
import {
  hostIdFromCanvasId,
  pageFromCanvasId,
  TEXT_LAYER_CLASS,
  TEXT_LAYER_SELECTOR,
} from "./dom-contract";
import { TextLayer } from "./loader";
import { applyHighlights } from "./highlights";
import { buildLinkLayer } from "./links";

/** A page with nothing in flight: no render task, no text layer, no
 *  viewport, no raw raster, no tier painted, and no queued generation. Two
 *  callers build one — a canvas found in the DOM, and a page registered before
 *  its canvas exists — and a field added to PageState should have exactly one
 *  place to be given its initial value. */
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
    preview: false,
    queueGen: 0,
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
  // Prefer the caller's hint (registerPage passes the page number); parse
  // the id only when the mount never registered. An id this cannot parse is
  // not a reader host at all, and page 1 is the least wrong guess for a
  // canvas about to be told which page it is.
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
    // A mount supersedes whatever the previous owner of this recycled id had
    // waiting for a lane; the scheduler resolves that caller as cancelled.
    unschedule(canvasId);
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
  unschedule(canvasId);
  if (st) {
    st.dead = true;
    try { st.renderTask && st.renderTask.cancel(); } catch (_) { /* ignore */ }
    try { st.textLayer && st.textLayer.cancel(); } catch (_) { /* ignore */ }
    session.releasePageSurfaces(st);
  }
  session.stateByCanvasId.delete(canvasId);
  session.sweepPdf();
}

export function cancelPage(canvasId: string): void {
  unschedule(canvasId);
  const st = session.stateByCanvasId.get(canvasId);
  if (st && st.renderTask) {
    try { st.renderTask.cancel(); } catch (_) { /* ignore */ }
    st.renderTask = null;
  }
}

/** The output-scale multiplier for one page: full native DPR for crisp text,
 *  capped so a single canvas never exceeds PAGE_MAX_PIXELS, and scaled down
 *  again for the preview tier.
 *
 * The cap used to also weigh the page against one windowful of pixels — the
 * soft-text bug: a US Letter page at 100% zoom on a 2x display needs ~1.48M
 * pixels, more than a 1440x900 window's 1.30M, so the render was throttled and
 * the browser upscaled it. Dropping the window term lets a single page use its
 * full native resolution; the per-page ceiling bounds memory.
 *
 * A preview is the same page at a fraction of that resolution: the reader is
 * not looking at it, they are moving towards it, and the raster exists to be
 * upgraded. Its floor is lower than a full render's because legibility is not
 * what it is for — shape and colour are. */
function pageOutputScale(cssW: number, cssH: number, preview: boolean): number {
  const dpr = globalThis.devicePixelRatio || 1;
  if (!(cssW > 0) || !(cssH > 0)) return preview ? dpr * PREVIEW_FLOOR : dpr;

  const capped = Math.sqrt(PAGE_MAX_PIXELS / (cssW * cssH));
  const full = Math.min(dpr, Math.max(0.5, capped));
  if (!preview) return full;
  // The budget's multiplier, with a floor of its own so a huge page's preview
  // does not end up too small to read as the page it stands in for.
  return Math.max(PREVIEW_FLOOR, full * currentBudget().previewScale);
}

/** The lowest output scale a preview raster is rendered at. */
const PREVIEW_FLOOR = 0.3;

/** What a render of this page is expected to cost, in output pixels. Read off
 *  the canvas's CSS box rather than the pdf.js viewport, which would need a
 *  worker round trip the scheduler must not wait for; an unmeasured canvas
 *  falls back to the surface it already holds, and then to zero — an unknown
 *  cost is not treated as an expensive one. */
function estimatedOutputPixels(st: PageState, preview: boolean): number {
  const canvas = st.canvas;
  if (!canvas) return 0;
  let cssW = canvas.clientWidth || 0;
  let cssH = canvas.clientHeight || 0;
  if (!(cssW > 0 && cssH > 0)) {
    // A freshly mounted canvas has no layout box of its own yet (it is sized by
    // the stylesheet as a percentage of its host), so ask the host's rect —
    // which is the box the render is about to fill.
    const rect =
      typeof canvas.getBoundingClientRect === "function" ? canvas.getBoundingClientRect() : null;
    cssW = rect?.width ?? 0;
    cssH = rect?.height ?? 0;
  }
  if (cssW > 0 && cssH > 0) {
    const out = pageOutputScale(cssW, cssH, preview);
    return Math.round(cssW * out * cssH * out);
  }
  // Nothing measured at all: the surface the canvas already holds, if any. An
  // unknown cost is not treated as an expensive one.
  return canvas.width > 0 && canvas.height > 0 ? canvas.width * canvas.height : 0;
}

/** Free a bake's intermediate. A filter-only bake returns the shared scratch
 *  (bakeRaster's blend step is the only pooled destination), and returning
 *  that to the pool would give one canvas two owners — the scratch goes back
 *  to the scratch and everything else to the pool, the same rule bakeInto
 *  follows. The render's own `target` is the caller's to keep or release. */
function releaseBaked(baked: HTMLCanvasElement, target: HTMLCanvasElement): void {
  if (baked === target) return;
  if (isSharedScratch(baked)) {
    releaseScratch(baked);
  } else {
    releasePooledCanvas(baked);
  }
}

export async function renderPageInternal(
  canvasId: string,
  scale: number,
  renderText: boolean,
  preview: boolean
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
  const out = pageOutputScale(cssW, cssH, preview);
  const pxW = Math.max(1, Math.floor(viewport.width * out));
  const pxH = Math.max(1, Math.floor(viewport.height * out));

  // Where the render draws: a scratch when the pipeline in force at start is
  // non-identity (the visible canvas keeps its baked copy until the swap),
  // the live canvas otherwise. pdf.js needs the destination NOW, so this half
  // is start-time; the THEME decision itself is re-made at completion (the
  // generation guard below) — a render that spans a pipeline change must not
  // bake against the palette it started under.
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
  // raster-only page, never to a failed render.
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
  // every colour decision downstream lives in the pdf-paper crate. A preview
  // stashes nothing: its pixels are a fraction of the page's, and the paper
  // session would be choosing the document's colour from a thumbnail of a
  // page the reader has not reached.
  if (!preview) stashPaperFrame(canvasId, st.page, target);

  // GENERATION GUARD: settle under the pipeline CURRENT at landing, not the
  // one in force when the render was issued. readPipeline() caches by the
  // root style token, so an appearance repaint, a scrub or a pipeline flip
  // can land while this raster is in flight, and page renders are NOT
  // serialized with the theme queue: a spread's two pages, issued a beat
  // apart, could bake against different theme states or land one raw and one
  // baked — the half-theme seam. The raw pixels are in `target` either way,
  // so the decision is free to move here.
  const pipeline = session.themeScrubActive ? null : readPipeline();
  const needsBake = pipeline ? !pipelineIsIdentity(pipeline) : false;

  if (needsBake && pipeline) {
    const bakeGen = pipeline.gen;
    const baked = await bakeRaster(target, pipeline);
    if (readPipeline().gen !== bakeGen) {
      releaseBaked(baked, target);
      if (target !== st.canvas) releaseCanvas(target);
      try { page.cleanup(); } catch (_) { /* ignore */ }
      return renderPageInternal(canvasId, scale, renderText, preview);
    }
    if (baked !== st.canvas) {
      showBaked(st.canvas, baked, "canvas-raw");
      releaseBaked(baked, target);
    }
    if (st.rawCanvas && st.rawCanvas !== st.canvas && st.rawCanvas !== target) {
      releaseCanvas(st.rawCanvas);
    }
    st.canvas.classList.remove("canvas-raw");
    // Retain the unbaked raster only while a scrub is plausible — its
    // window (a recent scrub transition, or an open appearance menu, where
    // the next drag is being born). A tint drag inside the window restores
    // it instead of re-rendering (dropping it outright made Dark invert
    // twice and Dim apply twice). Outside the window the raw is a
    // full-page surface per mounted page that nothing will ever ask for,
    // held while the footprint latches onto the peak; the scrub path
    // re-renders on demand (preparePagesForScrub).
    if (session.scrubIsPlausible()) {
      st.rawCanvas = target;
      session.dropRawIfIdle(st);
    } else if (target !== st.canvas) {
      st.rawCanvas = null;
      releaseCanvas(target);
    } else {
      // The render started under the identity pipeline or a scrub and drew
      // straight into the live canvas: that canvas IS the raw, and releasing
      // "the raw" would blank the page. Same bookkeeping the identity path
      // below keeps.
      st.rawCanvas = st.canvas;
    }
  } else {
    // Identity / already scrubbing: the live canvas IS the raw.
    st.rawCanvas = st.canvas;
    st.canvas.classList.toggle("canvas-raw", session.themeScrubActive);
  }

  if (renderText && !preview && st.host && st.textLayerEl) {
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
  // Which tier this canvas now holds. The memory ledger weighs previews
  // against their own ceiling, and the app-side host reads it back through the
  // next render request — a page promoted from preview to full is a different
  // raster at the same scale, so the tier is part of what a render produced.
  st.preview = preview;
  page.cleanup();

  if (session.bumpRenderCount() % CLEANUP_EVERY === 0) session.sweepPdf();
  session.noteActivity();
  // The total just changed; weigh it now rather than on the next motion frame.
  reclaim();

  return { ok: true, width: cssW, height: cssH, scale };
}

// Full-size renders share ONE bounded lane, and the lane is a priority queue
// rather than a FIFO. The depth bound is the old fix — before it, a zoom
// commit re-rendered every mounted page in parallel and each in-flight render
// held several full-page surfaces at once, and the footprint latched onto that
// summed peak. What a FIFO could not do is notice that the reader had moved:
// the first request in was for the page they were looking at when they
// started scrolling, and by the time a lane freed up it was for the page they
// had left. `scheduler.ts` owns the ordering and the dropping; this is the
// adapter from a component's `renderPage` call to a job it can score.
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
  renderText: boolean,
  preview = false
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
  const page = st.page;
  const pixels = estimatedOutputPixels(st, preview);
  return await new Promise<RenderResult>((resolve) => {
    schedule({
      key: canvasId,
      page,
      quality: preview ? "preview" : "full",
      pixels,
      run: async () => {
        // The page unmounted, or a newer request superseded this one, while it
        // waited for a lane. Drop it without touching pdf.js.
        if (st.dead || st.queueGen !== gen) {
          const cancelled = fail("cancelled", "Render cancelled");
          resolve(cancelled);
          return cancelled;
        }
        try {
          const result = await renderPageInternal(canvasId, scale, !!renderText, preview);
          resolve(result);
          return result;
        } catch (e) {
          const failed = failFrom(e);
          resolve(failed);
          return failed;
        }
      },
      abandon: (error) => resolve(error),
    });
  });
}

/** Re-render pages that have no unbaked raw so slider scrub can start
 *  without applying CSS filters on already-baked pixels — the scrub entry's
 *  background half. `onRendered` fires per page the moment its raw pixels
 *  have landed and been tagged, in the same turn, so the caller can drop
 *  that page's snapshot cover with no paint in between. */
export async function preparePagesForScrub(
  onRendered?: (canvasId: string) => void,
): Promise<void> {
  const jobs: Array<() => Promise<unknown>> = [];
  for (const [id, st] of session.stateByCanvasId) {
    if (st.dead || !st.canvas) continue;
    if (st.rawCanvas && st.rawCanvas !== st.canvas) continue;
    if (!st.rawCanvas) {
      jobs.push(async () => {
        const rendered = await renderPageInternal(id, st.scale || 1, false, st.preview);
        // A failed render keeps its cover — settled pixels beat a wiped
        // canvas — and the caller's final sweep releases it.
        if (rendered.ok) onRendered?.(id);
      });
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
    jobs.push(() => renderPageInternal(id, st.scale || 1, !!st.textLayerEl && !st.preview, st.preview));
  }
  await runLimited(jobs, 2);
}
