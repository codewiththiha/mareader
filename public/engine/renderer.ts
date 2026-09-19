// The only full-page raster entry point is renderPage: normal, zoom, theme
// and scrub work all pass through the same cancelable admission lane.
import type { PageState, PDFPageProxy, RenderResult, RenderTask } from "./types";
import { el, isSharedScratch, releaseCanvas, releasePooledCanvas, releaseScratch, showBaked } from "./canvas";
import { fail, failFrom } from "./errors";
import { stashPaperFrame } from "./paper";
import { bakeRaster } from "./theme/bake";
import { readPipeline } from "./theme/pipeline";
import { CLEANUP_EVERY, session } from "./state";
import { hostIdFromCanvasId, pageFromCanvasId, TEXT_LAYER_CLASS, TEXT_LAYER_SELECTOR } from "./dom-contract";
import { TextLayer } from "./loader";
import { applyHighlights } from "./highlights";
import { buildLinkLayer } from "./links";
import { rasterSize, type RasterTicket } from "./raster-scheduler";
import { motion, rasterTelemetry, pageIntent, pagePixelLimit, pagePriority, RASTER_BYTES_PER_PIXEL, rasterScheduler } from "./raster-resources";

function blankPage(page: number, canvas: HTMLCanvasElement | null, host: HTMLElement | null): PageState {
  return {
    page, canvas, host,
    textLayerEl: host?.querySelector(TEXT_LAYER_SELECTOR) as HTMLElement | null,
    renderTask: null, textLayer: null, viewport: null, scale: 1,
    dead: false, rawCanvas: null, queueGen: 0, queueHandle: 0,
  };
}

function ensurePage(canvasId: string, pageHint?: number, hostIdHint?: string): PageState | null {
  const existing = session.stateByCanvasId.get(canvasId);
  const canvas = el(canvasId) as HTMLCanvasElement | null;
  if (!canvas) return existing && !existing.dead ? existing : null;
  if (existing && !existing.dead && (!existing.canvas || existing.canvas === canvas)) {
    existing.canvas = canvas;
    existing.host = el(hostIdHint || hostIdFromCanvasId(canvasId));
    existing.textLayerEl = existing.host?.querySelector(TEXT_LAYER_SELECTOR) as HTMLElement | null;
    return existing;
  }
  if (existing) unregisterPage(canvasId);
  const st = blankPage(pageHint ?? pageFromCanvasId(canvasId) ?? 1, canvas, el(hostIdHint || hostIdFromCanvasId(canvasId)));
  session.stateByCanvasId.set(canvasId, st);
  return st;
}

export function registerPage(page: number, canvasId: string, hostId?: string): void {
  const existing = session.stateByCanvasId.get(canvasId);
  if (existing && existing.page !== page) unregisterPage(canvasId);
  if (!ensurePage(canvasId, page, hostId)) {
    session.stateByCanvasId.set(canvasId, blankPage(page, null, hostId ? el(hostId) : null));
  }
}

export function cancelPage(canvasId: string): void {
  rasterScheduler.cancel(canvasId);
  const st = session.stateByCanvasId.get(canvasId);
  if (!st) return;
  st.queueGen++;
  try { st.renderTask?.cancel(); } catch (_) { /* already done */ }
  try { st.textLayer?.cancel(); } catch (_) { /* already done */ }
}

/** Canonical true eviction: cancellation first, backing stores second. The
 * renderer owns its temporary canvas until finally, never the next mount's. */
export function unregisterPage(canvasId: string): void {
  cancelPage(canvasId);
  const st = session.stateByCanvasId.get(canvasId);
  if (st) { st.dead = true; session.releasePageSurfaces(st); }
  session.stateByCanvasId.delete(canvasId);
  rasterScheduler.wake();
}

function releaseBaked(baked: HTMLCanvasElement, target: HTMLCanvasElement): void {
  if (baked === target) return;
  if (isSharedScratch(baked)) releaseScratch(baked);
  else releasePooledCanvas(baked);
}

const cancelled = () => fail("cancelled", "Render cancelled");

// Private: every allocation below is covered by the ticket's reservation.
async function renderPageInternal(
  canvasId: string, st: PageState, scale: number, renderText: boolean,
  generation: number, ticket: RasterTicket,
): Promise<RenderResult> {
  const pdf = session.pdf;
  const canvas = st.canvas;
  if (!pdf || !canvas) return cancelled();
  const current = () => ticket.current() && !st.dead && st.queueGen === generation
    && session.pdf === pdf && session.stateByCanvasId.get(canvasId) === st && st.canvas === canvas;
  let page: PDFPageProxy | undefined;
  let target: HTMLCanvasElement | undefined;
  let task: RenderTask | undefined;
  let baked: HTMLCanvasElement | undefined;
  try {
    if (!current()) return cancelled();
    page = await pdf.getPage(st.page);
    if (!current()) return cancelled();
    const viewport = page.getViewport({ scale });
    const size = rasterSize(viewport.width, viewport.height, globalThis.devicePixelRatio || 1,
      Math.floor(ticket.bytes / RASTER_BYTES_PER_PIXEL));
    // Keep the existing display/thumbnail visible until a complete replacement
    // is ready. A cancellation never wipes a still-useful display surface.
    target = document.createElement("canvas");
    target.width = size.width;
    target.height = size.height;
    const ctx = target.getContext("2d", { alpha: false });
    if (!ctx) return fail("no_context", "No 2d context");
    const transform = [size.width / viewport.width, 0, 0, size.height / viewport.height, 0, 0];
    rasterTelemetry.fullRendersStarted++;
    if (motion.phase === "Fling") rasterTelemetry.fullRendersStartedDuringFling++;
    task = page.render({ canvasContext: ctx, viewport, transform });
    st.renderTask = task;
    await task.promise;
    if (!current()) return cancelled();
    rasterTelemetry.rasterPixelsProduced += size.width * size.height;
    if (!pageIntent(st).visible) rasterTelemetry.fullRendersCompletedOffscreen++;
    stashPaperFrame(canvasId, st.page, target);

    // A palette change re-bakes the same raw, never recursively rasterizes
    // the PDF. Bounded retries keep a continuous slider from starving a job.
    let ready = false;
    for (let attempt = 0; attempt < 3; attempt++) {
      const scrub = session.themeScrubActive;
      const pipeline = readPipeline();
      baked = scrub ? target : await bakeRaster(target, pipeline);
      if (!current()) return cancelled();
      if (scrub !== session.themeScrubActive || (!scrub && readPipeline().gen !== pipeline.gen)) {
        releaseBaked(baked, target);
        baked = undefined;
        continue;
      }
      showBaked(canvas, baked, "canvas-raw");
      canvas.classList.toggle("canvas-raw", scrub);
      releaseBaked(baked, target);
      baked = undefined;
      if (st.rawCanvas && st.rawCanvas !== canvas) releaseCanvas(st.rawCanvas);
      if (scrub) st.rawCanvas = canvas;
      else if (session.scrubIsPlausible()) {
        st.rawCanvas = target;
        session.dropRawIfIdle(st);
      } else st.rawCanvas = null;
      ready = true;
      break;
    }
    if (!ready) return cancelled();
    st.viewport = viewport;
    st.scale = scale;

    // Text/links are optional and visible-only. Extraction failures must not
    // turn a successfully painted page into an error (or skip page.cleanup).
    if (renderText && pageIntent(st).visible && motion.phase !== "Fling" && st.host && st.textLayerEl) {
      try {
        const textContent = await page.getTextContent();
        if (!current()) return cancelled();
        st.host.style.setProperty("--scale-factor", String(scale));
        const layer = document.createElement("div");
        layer.className = TEXT_LAYER_CLASS;
        layer.setAttribute("aria-hidden", "true");
        const tl = TextLayer({ textContentSource: textContent, container: layer, viewport });
        try { st.textLayer?.cancel(); } catch (_) { /* previous layer */ }
        st.textLayer = tl;
        await tl.render();
        if (!current()) return cancelled();
        const live = st.host.querySelector(TEXT_LAYER_SELECTOR);
        if (live?.parentNode) live.replaceWith(layer);
        else st.host.appendChild(layer);
        st.textLayerEl = layer;
        applyHighlights(st);
        await buildLinkLayer(st, viewport, page, current);
      } catch (_) { /* raster-only is still a successful page */ }
    }
    if (!current()) return cancelled();
    if (session.bumpRenderCount() % CLEANUP_EVERY === 0) session.sweepPdf();
    session.noteActivity();
    return { ok: true, width: Math.floor(viewport.width), height: Math.floor(viewport.height), scale };
  } catch (error) {
    return current() ? failFrom(error) : cancelled();
  } finally {
    if (baked && target) releaseBaked(baked, target);
    if (target && target !== st.rawCanvas) releaseCanvas(target);
    if (st.renderTask === task) st.renderTask = null;
    try { await page?.cleanup(); } catch (_) { /* pdf.js may still share the page */ }
  }
}

export async function renderPage(canvasId: string, scale: number, renderText: boolean): Promise<RenderResult> {
  if (!Number.isFinite(scale) || scale <= 0) return fail("invalid_scale", "Scale must be positive and finite");
  const pdf = session.pdf;
  let st = ensurePage(canvasId);
  if (!st?.canvas) {
    await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
    if (session.pdf !== pdf) return cancelled();
    st = ensurePage(canvasId);
  }
  if (!st?.canvas) return fail("no_canvas", "Canvas element not found in DOM: " + canvasId);
  if (!pdf) return fail("no_document", "No document open");
  const state = st;
  const generation = ++state.queueGen;
  const width = state.viewport ? state.viewport.width * scale / state.scale : undefined;
  const height = state.viewport ? state.viewport.height * scale / state.scale : undefined;
  const dpr = globalThis.devicePixelRatio || 1;
  const pixels = width && height ? Math.ceil(width * height * dpr * dpr) : pagePixelLimit();
  const bytes = Math.max(1, Math.min(pixels, pagePixelLimit())) * RASTER_BYTES_PER_PIXEL;
  return rasterScheduler.request<RenderResult>({
    key: canvasId, bytes, minimumBytes: Math.min(bytes, 512 * 1024 * RASTER_BYTES_PER_PIXEL),
    priority: () => state.queueGen === generation && session.pdf === pdf ? pagePriority(state) : null,
    run: (ticket) => renderPageInternal(canvasId, state, scale, renderText, generation, ticket),
    cancel: () => {
      try { state.renderTask?.cancel(); } catch (_) { /* already done */ }
      try { state.textLayer?.cancel(); } catch (_) { /* already done */ }
    },
    cancelled, failed: failFrom,
  });
}

export async function preparePagesForScrub(onRendered?: (canvasId: string) => void): Promise<void> {
  await Promise.all([...session.stateByCanvasId].map(async ([id, st]) => {
    if (st.dead || !st.canvas || st.rawCanvas) return;
    const result = await renderPage(id, st.scale || 1, false);
    if (result.ok) onRendered?.(id);
  }));
}

export async function rerenderLivePages(): Promise<void> {
  await Promise.all([...session.stateByCanvasId].map(async ([id, st]) => {
    if (!st.dead && st.canvas) await renderPage(id, st.scale || 1, !!st.textLayerEl);
  }));
}

/** A retained-raw theme bake uses the same lane and surface reservation as
 * PDF work, including per-canvas exclusion. No theme path bypasses admission. */
export async function rebakePage(canvasId: string): Promise<void> {
  const st = session.stateByCanvasId.get(canvasId);
  if (!st?.canvas || !st.rawCanvas || st.rawCanvas === st.canvas) return;
  const raw = st.rawCanvas;
  const canvas = st.canvas;
  const gen = ++st.queueGen;
  await rasterScheduler.request<RenderResult>({
    key: canvasId, bytes: raw.width * raw.height * RASTER_BYTES_PER_PIXEL,
    priority: () => st.queueGen === gen ? pagePriority(st) : null,
    cancel: () => { try { st.renderTask?.cancel(); } catch (_) { /* already done */ } },
    cancelled, failed: failFrom,
    run: async (ticket): Promise<RenderResult> => {
      if (!ticket.current() || st.dead || st.rawCanvas !== raw) return cancelled();
      const pipeline = readPipeline();
      const baked = await bakeRaster(raw, pipeline);
      try {
        if (!ticket.current() || st.dead || st.rawCanvas !== raw || st.queueGen !== gen
          || session.themeScrubActive || readPipeline().gen !== pipeline.gen) return cancelled();
        showBaked(canvas, baked, "canvas-raw");
        session.dropRawIfIdle(st);
        return { ok: true, width: canvas.width, height: canvas.height, scale: st.scale };
      } finally { releaseBaked(baked, raw); }
    },
  });
}
