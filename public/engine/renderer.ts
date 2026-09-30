// Page registration + canvas render + text/link layers.

import type {
  PageState,
  RenderResult,
} from "./types";
import { blitInto, el, isSharedScratch, sessionEl, releaseCanvas, releasePooledCanvas, releaseScratch, showBaked } from "./canvas";
import { fail, failFrom } from "./errors";
import { stashPaperFrame } from "./paper";
import { bakeRaster } from "./theme/bake";
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

/** A page with nothing in flight: no render task, no text layer, no
 *  viewport, no raw raster, both queue counters at zero. Two callers build
 *  one — a canvas found in the DOM, and a page registered before its canvas
 *  exists — and a field added to PageState should have exactly one place to
 *  be given its initial value. */
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

/** Pin one session to the pane root that owns its first registered page.
 *  Reader Rust passes its elements directly; the legacy id-only path resolves
 *  the same root here. */
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

/** Look up or create PageState. Recovers when registerPage ran before the
 *  <canvas> was in the DOM (Leptos mounts the effect one tick early). */
function ensurePage(
  s: EngineSession,
  canvasId: string,
  pageHint?: number,
  hostIdHint?: string
): PageState | null {
  const existing = s.stateByCanvasId.get(canvasId);
  // A page pinned to its own elements answers with them: the id is its key
  // in THIS session's map, not an address — another pane's page with the
  // same id is somebody else's canvas.
  // One whose surfaces were released is gone until its component registers
  // it again — never re-found by id.
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
  // Prefer the caller's hint (registerPage passes the page number); parse
  // the id only when the mount never registered. An id this cannot parse is
  // not a reader host at all, and page 1 is the least wrong guess for a
  // canvas about to be told which page it is.
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
    // The caller handed over the page's own elements (the reader always
    // does): pin them. No document-wide lookup can then pick a second
    // pane's page that happens to carry the same id.
    const pinnedHost = host ?? null;
    pinThemeRoot(s, pinnedHost);
    const textLayerEl = pinnedHost
      ? (pinnedHost.querySelector(TEXT_LAYER_SELECTOR) as HTMLElement | null)
      : null;
    if (existing) {
      // Re-registered (a remount of the same page): the state keeps what
      // it holds — its raw raster, viewport and scale — exactly as the id
      // path's revival does; only the elements are the new ones.
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
    // Canvas not in the DOM yet. Remember the page/host so renderPage can
    // finish registration on the next tick.
    s.stateByCanvasId.set(
      canvasId,
      blankPage(page, null, hostId ? el(hostId) : null, null)
    );
  }
}

export function unregisterPage(s: EngineSession, canvasId: string): void {
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
  // Deliberately NO sweepPdf here: a window move unmounts pages constantly,
  // and every unmount asking the worker to drop document caches would force
  // a re-parse of the very pages the next scroll remounts — churn paid at
  // the one moment the reader is moving. The sweep belongs to quiescence,
  // and three paths still run it there: the render-count cadence
  // (CLEANUP_EVERY), the session's idle timer, and the reader's
  // scroll-idle sweep.
}

export function cancelPage(s: EngineSession, canvasId: string): void {
  const st = s.stateByCanvasId.get(canvasId);
  if (st && st.renderTask) {
    try { st.renderTask.cancel(); } catch (_) { /* ignore */ }
    st.renderTask = null;
  }
}

/** Cancel every in-flight page render at once. The reader's close path calls
 * this synchronously with the click, BEFORE the navigate command crosses to
 * the Shell: the dispose returns over the frame channel, and a raster that
 * finished inside those hops would make the close look like it interrupted
 * nothing. Work observed in flight when the user leaves is cancelled here,
 * not raced; the session destroy during disposal still owns the teardown.
 * Superseding the per-canvas generation also stops work still queued behind
 * the canvas's rAF or the lane — those guards settle it as a drop — and the
 * rAF itself must fire to deliver that settle, so it is never cancelled. */
export function cancelPageRenders(s: EngineSession): void {
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

  // Cap so a single canvas never exceeds PAGE_MAX_PIXELS pixels. The old
  // code ALSO capped against one windowful of pixels — the soft-text bug: a
  // US Letter page at 100% zoom on a 2x display needs ~1.48M pixels, more
  // than a 1440x900 window's 1.30M, so the render was throttled and the
  // browser upscaled it. Dropping the window term lets a single page use its
  // full native resolution; the per-page ceiling bounds memory.
  const capped = Math.sqrt(PAGE_MAX_PIXELS / (cssW * cssH));
  return Math.min(dpr, Math.max(0.5, capped));
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

// --- Render trace (the fast-jump page-identity proof) ----------------------
// A bounded ring of ACTUAL raster events: the page number recorded when the
// engine starts a real page render, and that render's terminal
// classification. Render COUNTS alone cannot prove a jump skipped the pages
// it flew over — a burst over intermediate pages produces a small count just
// the same; this names the pages. Bounded: the oldest entry falls off at
// RENDER_TRACE_CAP, and nothing here touches the console in normal
// operation (the lifecycle counters stay the cheap primary signal).
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

/** Mark the beginning of a measurement generation: every raster the engine
 *  starts from now carries the returned id in the trace. */
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

export async function renderPageInternal(
  s: EngineSession,
  canvasId: string,
  scale: number,
  renderText: boolean
): Promise<RenderResult> {
  // Counting wrapper: every started render resolves exactly one of
  // completed / cancelled / failed, so the teardown baseline can assert the
  // lane is fully drained. The superseded-bake retry below recurses into
  // `renderPageNow` directly — the retry is the SAME started render, not a
  // second one.
  s.rendersStarted += 1;
  // The trace records the page at the moment the raster STARTS; the
  // terminal classification below pairs with it in the same generation.
  const tracePage = ensurePage(s, canvasId)?.page ?? -1;
  traceRender(s.sid, tracePage, "start");
  lifecycleEvent("render:start");
  try {
    const result = await renderPageNow(s, canvasId, scale, renderText);
    if (result.ok) {
      s.rendersCompleted += 1;
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
    // The counting wrapper OWNS the invariant: a started render gets
    // exactly one terminal classification even when the body throws
    // instead of returning a result — otherwise the pairing rule the
    // baseline asserts breaks on an exception path, not a real leak.
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

  // Where the render draws: ALWAYS a scratch outside a scrub, so the
  // visible canvas keeps its last good bitmap until one synchronous blit
  // replaces it at completion. Assigning `width`/`height` clears a canvas,
  // and pdf.js then paints progressively — drawing straight into the live
  // canvas (what the identity pipeline used to do) showed every re-render
  // as a page that went blank and filled back in, most visibly at a zoom
  // commit, which re-renders every mounted page at once. The cost is one
  // page-sized scratch per IN-FLIGHT render, bounded by the lane
  // (PAGE_RENDER_LIMIT) — the surface the non-identity pipelines already
  // paid. A scrub still draws in place: it covers the page with its own
  // `.page-snapshot` while its raws are rebuilt (theme/scrub.ts). The THEME
  // decision itself is re-made at completion (the generation guard below):
  // a render that spans a pipeline change must not bake against the palette
  // it started under.
  const target = s.themeScrubActive ? st.canvas : document.createElement("canvas");
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
    // The task settled: drop the handle (only if it is still THIS task —
    // a superseding render already cancelled and replaced it) so the
    // active-render count reads in-flight truth, not render history.
    if (st.renderTask === task) st.renderTask = null;
    // The raster is dead: the orphaned text extraction was already made
    // infallible at creation time, so nothing can leak here.
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

  // `target` still holds raw pixels here (bakeRaster runs below): the one
  // point in the pipeline where the document's own paper is intact. Park a
  // ≤96×96 frame for the Rust paper session to drain after the render —
  // every colour decision downstream lives in the pdf-paper crate.
  stashPaperFrame(s, canvasId, st.page, target);

  // GENERATION GUARD: settle under the pipeline CURRENT at landing, not the
  // one in force when the render was issued. readPipeline() caches by the
  // root style token, so an appearance repaint, a scrub or a pipeline flip
  // can land while this raster is in flight, and page renders are NOT
  // serialized with the theme queue: a spread's two pages, issued a beat
  // apart, could bake against different theme states or land one raw and one
  // baked — the half-theme seam. The raw pixels are in `target` either way,
  // so the decision is free to move here.
  const pipeline = s.themeScrubActive ? null : readPipeline(s);
  const needsBake = pipeline ? !pipelineIsIdentity(pipeline) : false;

  if (needsBake && pipeline) {
    const bakeGen = pipeline.gen;
    const baked = await bakeRaster(target, pipeline);
    if (readPipeline(s).gen !== bakeGen) {
      releaseBaked(baked, target);
      if (target !== st.canvas) releaseCanvas(target);
      try { page.cleanup(); } catch (_) { /* ignore */ }
      return renderPageNow(s, canvasId, scale, renderText);
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
    if (s.scrubIsPlausible()) {
      st.rawCanvas = target;
      s.dropRawIfIdle(st);
    } else if (target !== st.canvas) {
      st.rawCanvas = null;
      releaseCanvas(target);
    } else {
      // The render started under a scrub (the one case that draws in place)
      // and drew straight into the live canvas: that canvas IS the raw, and releasing
      // "the raw" would blank the page. Same bookkeeping the identity path
      // below keeps.
      st.rawCanvas = st.canvas;
    }
  } else {
    // Identity / already scrubbing: the live canvas IS the raw. A scratch
    // render lands here in one blit — the swap that keeps the old bitmap on
    // screen for the whole raster.
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

  // The link-layer build above awaits, so the document can be gone by the time
  // control returns here (a close during render). A dead surface must not touch
  // document-scoped bookkeeping — the idle sweeper belongs to the document
  // that just died, and re-arming it keeps the teardown baseline from ever
  // reading drained.
  if (!st.dead) {
    if (s.bumpRenderCount() % CLEANUP_EVERY === 0) s.sweepPdf();
    s.noteActivity();
  }

  return { ok: true, width: cssW, height: cssH, scale };
}

// Full-size renders share ONE bounded lane, the thumbnail lane's pattern.
// The per-canvas rAF below coalesces a single page's requests; it never
// limited how many pages rasterise at once, so a zoom commit re-rendered
// every mounted page in parallel and each in-flight render held several
// full-page surfaces (scratch, bake output) at the same time. The footprint
// latches onto that summed peak, which is what made one commit cost
// hundreds of MB it never handed back. Queued jobs re-check their
// generation at the front of the lane, so a page that unmounted or was
// superseded while waiting drops without touching pdf.js.
//
// The cap is TWO-LAYERED. PAGE_RENDER_LIMIT bounds one session's in-flight
// rasters (its own queue, its own teardown drain). REALM_PAGE_LIMIT bounds
// in-flight rasters across ALL sessions, because a raster is main-thread
// work wherever it runs: four panes re-theming or scrolling together would
// otherwise stack four sessions' worth of concurrent rasters into one long
// frame stall. With the realm cap the panes pace as one progressive sweep;
// a pane reading alone sees the same two slots it always had.
const PAGE_RENDER_LIMIT = 2;

/** The page lane's gauges for the stats surface (queue depth, active
 *  slots): the teardown baseline requires an EMPTY lane, not merely one
 *  whose in-flight jobs have settled. */
export function pageLaneGauge(s: EngineSession): { pageQueue: number; pageActive: number } {
  return { pageQueue: s.pageLane.queue.length, pageActive: s.pageLane.active };
}

/** Re-offer every registered session's queue head the lane. The registry
 *  holds sessions weakly (state.ts), so this walk can never keep a session
 *  alive; retired or collected ones are pruned as the walk meets them. */
function pumpAllLanes(): void {
  pumpLaneRegistrants(pumpPageQueue);
}

/** Drain the queue on teardown: every queued job's guard sees the dead
 *  state, resolves its caller with a drop, and pumps the next — the same
 *  cascade the thumbnail lane's epoch bump runs. Without this, queued
 *  closures (and the promise resolvers they capture) sit in the array
 *  until the FIFO happens to reach them, retaining canvases, scales and
 *  resolvers across the dispose.
 *
 *  A drain is a teardown act, not scheduling: it pops regardless of the
 *  realm cap, which may be full of ANOTHER session's rasters at the moment
 *  this session dies. The popped jobs all resolve as drops (their guard
 *  sees the dead state) and never claim a raster slot, so bypassing the cap
 *  starts no work — it only empties the queue the baseline requires empty. */
export function drainPageLane(s: EngineSession): void {
  const lane = s.pageLane;
  while (lane.queue.length > 0) {
    const next = lane.queue.shift();
    if (!next) return;
    lane.active += 1;
    next();
  }
}

function pumpPageQueue(s: EngineSession): void {
  const lane = s.pageLane;
  while (
    lane.active < PAGE_RENDER_LIMIT &&
    realmLane.active < REALM_PAGE_LIMIT &&
    lane.queue.length > 0
  ) {
    const next = lane.queue.shift();
    if (!next) return;
    lane.active += 1;
    next();
  }
}

export async function renderPage(
  s: EngineSession,
  canvasId: string,
  scale: number,
  renderText: boolean
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
      s.pageLane.queue.push(() => {
        const finish = () => {
          s.pageLane.active -= 1;
          pumpPageQueue(s);
        };
        // The page unmounted, a newer scale superseded this job, or the
        // session retired while it waited for a lane slot. Drop it without
        // touching pdf.js — and without ever holding a realm slot, which
        // is claimed only by work that actually runs.
        if (st.dead || s.disposed || st.queueGen !== gen) {
          s.rendersDropped += 1;
          lifecycleEvent("render:cancel");
          resolve(fail("cancelled", "Render cancelled"));
          finish();
          return;
        }
        realmLane.active += 1;
        renderPageInternal(s, canvasId, scale, !!renderText)
          .then(resolve)
          .catch((e: unknown) => {
            resolve(failFrom(e));
          })
          .finally(() => {
            realmLane.active -= 1;
            // A freed slot is every session's chance: re-offer the lane to
            // each registered queue so the panes pace as one sweep.
            pumpAllLanes();
            finish();
          });
      });
      pumpPageQueue(s);
    });
  });
}

/** Re-render pages that have no unbaked raw so slider scrub can start
 *  without applying CSS filters on already-baked pixels — the scrub entry's
 *  background half. `onRendered` fires per page the moment its raw pixels
 *  have landed and been tagged, in the same turn, so the caller can drop
 *  that page's snapshot cover with no paint in between. The renders ride
 *  the page lane (and its realm cap), so a multi-pane scrub queues as one
 *  paced sweep instead of stacking full-page rasters. */
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
          // A failed render keeps its cover — settled pixels beat a wiped
          // canvas — and the caller's final sweep releases it.
          if (rendered.ok) onRendered?.(id);
        }),
      );
    }
  }
  if (jobs.length) await Promise.all(jobs);
}

/** Re-render every live page from pdf.js. Used when a theme change arrives
 *  after we have already dropped the raw raster. Through the page lane, so
 *  a theme change with several panes open re-renders as one paced sweep
 *  across the realm instead of one stall per pane in parallel. */
export async function rerenderLivePages(s: EngineSession): Promise<void> {
  const jobs: Array<Promise<unknown>> = [];
  for (const [id, st] of s.stateByCanvasId) {
    if (st.dead || !st.canvas) continue;
    jobs.push(renderPage(s, id, st.scale || 1, !!st.textLayerEl));
  }
  await Promise.all(jobs);
}
