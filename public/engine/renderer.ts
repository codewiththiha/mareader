// Page registration + canvas render + text/link layers.

import type {
  PageSizeResult,
  PageState,
  RenderResult,
} from "./types";
import { blitInto, el, sessionEl, releaseCanvas } from "./canvas";
import { fail, failFrom } from "./errors";
import { acquireRasterSlot, cancelRasterWaiters } from "./raster-lane";
import { stashPaperFrame } from "./paper";
import { bakeFiltered, paintBaked, releaseBake } from "./theme/bake";
import { pipelineIsIdentity, readPipeline } from "./theme/pipeline";
import { observeThemeRoot, unobserveThemeRoot } from "./theme/paper";
import {
  CLEANUP_EVERY,
  REALM_PAGE_LIMIT,
  lifecycleEvent,
  PAGE_MAX_PIXELS,
  pumpLaneRegistrants,
  realmLane,
} from "./state";
import type { EngineSession } from "./state";
import {
  hostIdFromCanvasId,
  pageFromCanvasId,
  TEXT_LAYER_CLASS,
  TEXT_LAYER_SELECTOR,
} from "./dom-contract";
import { TextLayer } from "./loader";
import { applyHighlights } from "./highlights";
import { buildLinkLayer } from "./links";

// A page with nothing in flight: no task, layer, viewport or raw.
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
    pinned: false,
  };
}

// Pin one session to the pane root owning its first registered page.
function pinThemeRoot(s: EngineSession, host: HTMLElement | null): void {
  const paneRoot = host && typeof host.closest === "function"
    ? host.closest("[data-pane-root]") as HTMLElement | null
    : null;
  if (paneRoot && s.themeRoot !== paneRoot) {
    if (s.themeRoot) unobserveThemeRoot(s);
    s.themeRoot = paneRoot;
    s.themePipeline.token = null;
    observeThemeRoot(s);
  }
}

// Look up or create PageState, recovering when the canvas landed late.
function ensurePage(
  s: EngineSession,
  canvasId: string,
  pageHint?: number,
  hostIdHint?: string
): PageState | null {
  const existing = s.stateByCanvasId.get(canvasId);
  // A pinned page answers with its own elements; the id is its key.
  if (existing && existing.pinned) return existing.canvas && !existing.dead ? existing : null;
  const canvas = sessionEl(s.sid, canvasId) as HTMLCanvasElement | null;
  if (existing && existing.canvas && !existing.dead) {
    if (canvas && existing.canvas !== canvas) existing.canvas = canvas;
    return existing;
  }
  if (!canvas) return null;
  const hostId = hostIdHint || hostIdFromCanvasId(canvasId);
  const host = el(hostId);
  pinThemeRoot(s, host);
  const textLayerEl = host ? (host.querySelector(TEXT_LAYER_SELECTOR) as HTMLElement | null) : null;
  if (existing) {
    existing.dead = false;
    existing.canvas = canvas;
    existing.host = host;
    existing.textLayerEl = textLayerEl;
    return existing;
  }
  // Prefer the caller's hint; parse the id only for an unregistered mount.
  const page = pageHint && pageHint > 0 ? pageHint : (pageFromCanvasId(canvasId) ?? 1);
  const st = blankPage(page, canvas, host, textLayerEl);
  s.stateByCanvasId.set(canvasId, st);
  return st;
}

export function registerPage(
  s: EngineSession,
  page: number,
  canvasId: string,
  hostId?: string,
  canvas?: HTMLCanvasElement | null,
  host?: HTMLElement | null
): void {
  const existing = s.stateByCanvasId.get(canvasId);
  if (existing) {
    existing.dead = true;
    try { existing.renderTask && existing.renderTask.cancel(); } catch (_) { /* ignore */ }
    try { existing.textLayer && existing.textLayer.cancel(); } catch (_) { /* ignore */ }
    if (existing.queueHandle) {
      cancelAnimationFrame(existing.queueHandle);
      existing.queueHandle = 0;
    }
  }
  if (canvas) {
    // The page's own elements were handed over: pin them.
    const pinnedHost = host ?? null;
    pinThemeRoot(s, pinnedHost);
    const textLayerEl = pinnedHost
      ? (pinnedHost.querySelector(TEXT_LAYER_SELECTOR) as HTMLElement | null)
      : null;
    if (existing) {
      // Re-registered: keep the state, take the new elements.
      existing.dead = false;
      existing.page = page;
      existing.canvas = canvas;
      existing.host = pinnedHost;
      existing.textLayerEl = textLayerEl;
      existing.pinned = true;
      return;
    }
    const st = blankPage(page, canvas, pinnedHost, textLayerEl);
    st.pinned = true;
    s.stateByCanvasId.set(canvasId, st);
    return;
  }
  const st = ensurePage(s, canvasId, page, hostId);
  if (!st) {
    // Canvas not in the DOM yet; finish registration next tick.
    s.stateByCanvasId.set(
      canvasId,
      blankPage(page, null, hostId ? el(hostId) : null, null)
    );
  }
}

export function unregisterPage(s: EngineSession, canvasId: string): void {
  cancelRasterWaiters(s, canvasId);
  const st = s.stateByCanvasId.get(canvasId);
  if (st) {
    st.dead = true;
    try { st.renderTask && st.renderTask.cancel(); } catch (_) { /* ignore */ }
    try { st.textLayer && st.textLayer.cancel(); } catch (_) { /* ignore */ }
    if (st.queueHandle) {
      cancelAnimationFrame(st.queueHandle);
      st.queueHandle = 0;
    }
    s.releasePageSurfaces(st);
  }
  s.stateByCanvasId.delete(canvasId);
  s.rankByCanvas.delete(canvasId);
  // No sweepPdf here: an unmount-heavy move would force re-parses.
}

export function cancelPage(s: EngineSession, canvasId: string): void {
  cancelRasterWaiters(s, canvasId);
  const st = s.stateByCanvasId.get(canvasId);
  if (!st) return;
  st.queueGen = (st.queueGen || 0) + 1;
  if (st.renderTask) {
    try { st.renderTask.cancel(); } catch (_) { /* ignore */ }
    st.renderTask = null;
  }
  // A cancelled job drops only when the lane pops it.
  if (s.pageLane.queue.length > 0) pumpPageQueue(s);
}

// Cancel every in-flight page render; the close path calls this
// synchronously with the click.
export function cancelPageRenders(s: EngineSession): void {
  cancelRasterWaiters(s);
  for (const st of s.stateByCanvasId.values()) {
    st.queueGen = (st.queueGen || 0) + 1;
    if (st.renderTask) {
      try { st.renderTask.cancel(); } catch (_) { /* ignore */ }
      st.renderTask = null;
    }
  }
}

function pageOutputScale(cssW: number, cssH: number): number {
  // Full native DPR for crisp text; PAGE_MAX_PIXELS is the memory guardrail.
  const dpr = globalThis.devicePixelRatio || 1;
  if (!(cssW > 0) || !(cssH > 0)) return dpr;

  // Cap so one canvas never exceeds PAGE_MAX_PIXELS; no window term.
  const capped = Math.sqrt(PAGE_MAX_PIXELS / (cssW * cssH));
  return Math.min(dpr, Math.max(0.5, capped));
}

// --- Render trace: a bounded ring of actual raster events. ---
export type RenderTracePhase = "start" | "complete" | "cancel" | "fail";
export type RenderTraceEntry = {
  sid: number;
  gen: number;
  page: number;
  phase: RenderTracePhase;
  t: number;
};

const RENDER_TRACE_CAP = 128;
const renderTrace: RenderTraceEntry[] = [];
let renderGeneration = 0;

// Mark a measurement generation; rasters carry its id in the trace.
export function beginRenderGeneration(): number {
  renderGeneration += 1;
  return renderGeneration;
}

function traceRender(sid: number, page: number, phase: RenderTracePhase): void {
  renderTrace.push({ sid, gen: renderGeneration, page, phase, t: Date.now() });
  if (renderTrace.length > RENDER_TRACE_CAP) renderTrace.shift();
}

/** A bounded copy of the ring, oldest first. */
export function readRenderTrace(): RenderTraceEntry[] {
  return renderTrace.slice();
}

async function renderPageInternal(
  s: EngineSession,
  canvasId: string,
  scale: number,
  renderText: boolean,
  requestedAt: number
): Promise<RenderResult> {
  // One terminal classification per started render, so the lane drains.
  s.rendersStarted += 1;
  // The trace records the page when the raster STARTS.
  const tracePage = ensurePage(s, canvasId)?.page ?? -1;
  traceRender(s.sid, tracePage, "start");
  lifecycleEvent("render:start");
  const startMs = Date.now();
  try {
    const result = await renderPageNow(s, canvasId, scale, renderText);
    if (result.ok) {
      s.rendersCompleted += 1;
      // An exponential mean: one slow frame must not retune the session.
      // Timed from the request, not the lane slot: the reader waits for the
      // queue too, and the band's lead is measured against what they wait
      // for. The lane slot's own span is the floor of that.
      const ms = Math.max(Date.now() - requestedAt, Date.now() - startMs);
      s.fillMs = s.fillMs <= 0 ? ms : s.fillMs + (ms - s.fillMs) * 0.2;
      traceRender(s.sid, tracePage, "complete");
      lifecycleEvent("render:complete");
    } else if (result.error.name === "cancelled") {
      s.rendersCancelled += 1;
      traceRender(s.sid, tracePage, "cancel");
      lifecycleEvent("render:cancel");
    } else {
      s.rendersFailed += 1;
      traceRender(s.sid, tracePage, "fail");
    }
    return result;
  } catch (e) {
    // The wrapper owns the invariant: one classification even on a throw.
    s.rendersFailed += 1;
    traceRender(s.sid, tracePage, "fail");
    throw e;
  }
}

async function renderPageNow(
  s: EngineSession,
  canvasId: string,
  scale: number,
  renderText: boolean
): Promise<RenderResult> {
  const st = ensurePage(s, canvasId);
  if (!st || !st.canvas) return fail("no_canvas", "Canvas element not found in DOM: " + canvasId);
  if (!s.pdf) return fail("no_document", "No document open");

  try { st.renderTask && st.renderTask.cancel(); } catch (_) { /* ignore */ }
  try { st.textLayer && st.textLayer.cancel(); } catch (_) { /* ignore */ }
  st.renderTask = null;
  st.textLayer = null;

  const page = await s.pdf.getPage(st.page);
  if (st.dead || !st.canvas) {
    try { page.cleanup(); } catch (_) { /* ignore */ }
    s.releasePageSurfaces(st);
    return fail("cancelled", "Render cancelled");
  }
  const viewport = page.getViewport({ scale });

  const cssW = Math.floor(viewport.width);
  const cssH = Math.floor(viewport.height);
  const out = pageOutputScale(cssW, cssH);
  const pxW = Math.max(1, Math.floor(viewport.width * out));
  const pxH = Math.max(1, Math.floor(viewport.height * out));

  // ALWAYS a scratch outside a scrub; a scrub draws in place.
  const target = s.themeScrubActive ? st.canvas : document.createElement("canvas");
  target.width = pxW;
  target.height = pxH;
  const ctx = target.getContext("2d", { alpha: false });
  const transform = out !== 1 ? [out, 0, 0, out, 0, 0] : null;

  if (!ctx) {
    if (target !== st.canvas) releaseCanvas(target);
    return fail("no_context", "No 2d context");
  }
  // Start the text extraction before rendering so the two overlap.
  const textTask =
    renderText && st.host && st.textLayerEl ? page.getTextContent().catch(() => null) : null;
  const task = page.render({ canvasContext: ctx, viewport, transform });
  st.renderTask = task;
  try {
    await task.promise;
  } catch (e) {
    // The task settled: drop the handle if it is still this one.
    if (st.renderTask === task) st.renderTask = null;
    // The raster is dead; the orphaned extraction is infallible.
    try { page.cleanup(); } catch (_) { /* ignore */ }
    if (target !== st.canvas) releaseCanvas(target);
    if (st.dead) s.releasePageSurfaces(st);
    if ((e as { name?: string }).name === "RenderingCancelledException") {
      return fail("cancelled", "Render cancelled");
    }
    return failFrom(e);
  }
  if (st.renderTask === task) st.renderTask = null;
  if (st.dead) {
    try { page.cleanup(); } catch (_) { /* ignore */ }
    if (target !== st.canvas) releaseCanvas(target);
    s.releasePageSurfaces(st);
    return fail("cancelled", "Render cancelled");
  }

  // `target` still holds raw pixels: park a frame for the paper session.
  stashPaperFrame(s, canvasId, st.page, target);

  // GENERATION GUARD: settle under the pipeline current at landing.
  const pipeline = s.themeScrubActive ? null : readPipeline(s);
  const needsBake = pipeline ? !pipelineIsIdentity(pipeline) : false;

  if (needsBake && pipeline) {
    const bakeGen = pipeline.gen;
    // Only the FILTER waits; nothing paints until the check passes.
    const baked = await bakeFiltered(target, pipeline);
    if (readPipeline(s).gen !== bakeGen) {
      releaseBake(baked);
      if (target !== st.canvas) releaseCanvas(target);
      try { page.cleanup(); } catch (_) { /* ignore */ }
      return renderPageNow(s, canvasId, scale, renderText);
    }
    paintBaked(st.canvas, baked, pipeline, "canvas-raw");
    if (st.rawCanvas && st.rawCanvas !== st.canvas && st.rawCanvas !== target) {
      releaseCanvas(st.rawCanvas);
    }
    // Retain the unbaked raster only while a scrub is plausible.
    if (s.scrubIsPlausible()) {
      st.rawCanvas = target;
      s.dropRawIfIdle(st);
    } else if (target !== st.canvas) {
      st.rawCanvas = null;
      releaseCanvas(target);
    } else {
      // Started under a scrub and drew in place: that canvas IS the raw.
      st.rawCanvas = st.canvas;
    }
  } else {
    // Identity / already scrubbing: the live canvas IS the raw.
    if (target !== st.canvas) {
      blitInto(st.canvas, target);
      if (st.rawCanvas && st.rawCanvas !== st.canvas) releaseCanvas(st.rawCanvas);
      releaseCanvas(target);
    }
    st.rawCanvas = st.canvas;
    st.canvas.classList.toggle("canvas-raw", s.themeScrubActive);
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
      if (st.dead) s.releasePageSurfaces(st);
      if ((e as { name?: string }).name === "AbortException") {
        return fail("cancelled", "Text render cancelled");
      }
      return failFrom(e);
    }
    if (st.dead) {
      try { page.cleanup(); } catch (_) { /* ignore */ }
      s.releasePageSurfaces(st);
      return fail("cancelled", "Render cancelled");
    }

    const live = st.host.querySelector(TEXT_LAYER_SELECTOR);
    if (live && live.parentNode) {
      live.replaceWith(layer);
    } else {
      st.host.appendChild(layer);
    }
    st.textLayerEl = layer;

    applyHighlights(s, st);

    await buildLinkLayer(s, st, viewport, page);
  }

  st.viewport = viewport;
  st.scale = scale;
  page.cleanup();

  // The build above awaits; a dead surface must not re-arm the sweeper.
  if (!st.dead) {
    if (s.bumpRenderCount() % CLEANUP_EVERY === 0) s.sweepPdf();
    s.noteActivity();
  }

  return { ok: true, width: cssW, height: cssH, scale };
}

// A page's scale-1 box, from the document, cached per session.
export async function probePageSize(s: EngineSession, page: number): Promise<PageSizeResult> {
  if (!s.pdf) return fail("no_document", "No document open");
  const cached = s.intrinsicByPage.get(page);
  if (cached) return { ok: true, width: cached.width, height: cached.height };
  if (!(page >= 1) || page > s.numPages) {
    return fail("no_page", "No such page: " + page);
  }
  try {
    const p = await s.pdf.getPage(page);
    if (s.disposed) {
      try { p.cleanup(); } catch (_) { /* ignore */ }
      return fail("no_session", "Session destroyed during a size probe");
    }
    const vp = p.getViewport({ scale: 1 });
    try { p.cleanup(); } catch (_) { /* ignore */ }
    const width = vp.width;
    const height = vp.height;
    s.intrinsicByPage.set(page, { width, height });
    return { ok: true, width, height };
  } catch (e) {
    return failFrom(e);
  }
}

// Full-size renders share ONE bounded lane (session + realm caps).
const PAGE_RENDER_LIMIT = 2;

// The page lane's stats gauges; teardown requires an empty queue.
export function pageLaneGauge(s: EngineSession): {
  pageQueue: number;
  pageActive: number;
  pageLimit: number;
} {
  return {
    pageQueue: s.pageLane.queue.length,
    pageActive: s.pageLane.active,
    // Published rather than copied by the caller: the limit has one home.
    pageLimit: PAGE_RENDER_LIMIT,
  };
}

// Re-offer every registered session's queue head the lane.
function pumpAllLanes(): void {
  pumpLaneRegistrants(pumpPageQueue);
}

// Drain the queue on teardown; every job resolves as a drop.
export function drainPageLane(s: EngineSession): void {
  cancelRasterWaiters(s);
  const lane = s.pageLane;
  while (lane.queue.length > 0) {
    const next = lane.take();
    if (!next) return;
    lane.active += 1;
    next();
  }
}

/** A page's urgency changed while it was queued: re-read it at dequeue. */
export function reprioritizePage(s: EngineSession, canvasId: string, rank: number): void {
  if (s.rankByCanvas.get(canvasId) === rank) return;
  s.rankByCanvas.set(canvasId, rank);
  s.pageLane.reprioritize(canvasId, rank);
}

function pumpPageQueue(s: EngineSession): void {
  const lane = s.pageLane;
  while (
    lane.active < PAGE_RENDER_LIMIT &&
    realmLane.active < REALM_PAGE_LIMIT &&
    lane.queue.length > 0
  ) {
    const next = lane.take();
    if (!next) return;
    lane.active += 1;
    next();
  }
}

export async function renderPage(
  s: EngineSession,
  canvasId: string,
  scale: number,
  renderText: boolean,
  rank = 0
): Promise<RenderResult> {
  let st = ensurePage(s, canvasId);
  if (!st || !st.canvas) {
    await new Promise<void>((r) => {
      requestAnimationFrame(() => r());
    });
    st = ensurePage(s, canvasId);
  }
  if (!st) return fail("no_canvas", "Canvas element not found in DOM: " + canvasId);
  if (!s.pdf) return fail("no_document", "No document open");

  const gen = (st.queueGen || 0) + 1;
  st.queueGen = gen;
  if (st.queueHandle) {
    cancelAnimationFrame(st.queueHandle);
    st.queueHandle = 0;
  }
  // Time to visible is measured from here, before the frame the lane waits
  // for: a page that waits a frame in the queue and one in the lane are the
  // same wait to the reader.
  const requestedAt = Date.now();
  return await new Promise<RenderResult>((resolve) => {
    st.queueHandle = requestAnimationFrame(() => {
      st.queueHandle = 0;
      if (st.dead || s.disposed || st.queueGen !== gen) {
        s.rendersDropped += 1;
        lifecycleEvent("render:cancel");
        resolve(fail("cancelled", "Render cancelled"));
        return;
      }
      s.rendersQueued += 1;
      s.rankByCanvas.set(canvasId, rank);
      s.pageLane.push(canvasId, rank, () => {
        const finish = () => {
          s.pageLane.active -= 1;
          pumpPageQueue(s);
        };
        // Dropped without touching pdf.js or claiming a realm slot.
        if (st.dead || s.disposed || st.queueGen !== gen) {
          s.rendersDropped += 1;
          lifecycleEvent("render:cancel");
          resolve(fail("cancelled", "Render cancelled"));
          finish();
          return;
        }
        realmLane.active += 1;
        // Keep the session cap while waiting for the window cap.
        void (async () => {
          const permit = await acquireRasterSlot(s, canvasId);
          try {
            // A permit may land after unmount, close or a zoom: check again.
            if (!permit || st.dead || s.disposed || st.queueGen !== gen) {
              s.rendersDropped += 1;
              lifecycleEvent("render:cancel");
              resolve(fail("cancelled", "Render cancelled"));
              return;
            }
            resolve(await renderPageInternal(s, canvasId, scale, !!renderText, requestedAt));
          } finally {
            permit?.release();
          }
        })().catch((e: unknown) => {
          resolve(failFrom(e));
        }).finally(() => {
          realmLane.active -= 1;
          // A freed slot is every session's chance: re-offer the lane.
          pumpAllLanes();
          finish();
        });
      });
      pumpPageQueue(s);
    });
  });
}

// Re-render pages with no raw so a slider scrub can start.
export async function preparePagesForScrub(
  s: EngineSession,
  onRendered?: (canvasId: string) => void,
): Promise<void> {
  const jobs: Array<Promise<unknown>> = [];
  for (const [id, st] of s.stateByCanvasId) {
    if (st.dead || !st.canvas) continue;
    if (st.rawCanvas && st.rawCanvas !== st.canvas) continue;
    if (!st.rawCanvas) {
      jobs.push(
        renderPage(s, id, st.scale || 1, false).then((rendered) => {
          // A failed render keeps its cover; the final sweep releases it.
          if (rendered.ok) onRendered?.(id);
        }),
      );
    }
  }
  if (jobs.length) await Promise.all(jobs);
}

// Re-render every live page from pdf.js, through the page lane.
export async function rerenderLivePages(s: EngineSession): Promise<void> {
  const jobs: Array<Promise<unknown>> = [];
  for (const [id, st] of s.stateByCanvasId) {
    if (st.dead || !st.canvas) continue;
    jobs.push(renderPage(s, id, st.scale || 1, !!st.textLayerEl));
  }
  await Promise.all(jobs);
}
