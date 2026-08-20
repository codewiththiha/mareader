// Page registration + canvas render + text/link layers.

import type {
  Annotation,
  PageState,
  PDFPageProxy,
  RenderResult,
  Viewport,
} from "./types";
import { blitInto, el, errorInfo, fail, releaseCanvas, releasePooledCanvas } from "./canvas";
import {
  bakeRaster,
  pipelineIsIdentity,
  readPipeline,
} from "./theme";
import {
  activeMatch,
  bumpRenderCount,
  CLEANUP_EVERY,
  highlightMode,
  pdf,
  releasePageSurfaces,
  searchQuery,
  scrubbing,
  stateByCanvasId,
  dropRawIfIdle,
  noteActivity,
  sweepPdf,
  PAGE_MAX_PIXELS,
  CANVAS_AREA_FACTOR,
} from "./state";
import { TextLayer } from "./loader";

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

export function applyHighlights(st: PageState): void {
  const { host, textLayerEl } = st;
  if (!host) return;
  host.querySelectorAll(".highlight").forEach((n) => n.remove());
  if (!searchQuery || !textLayerEl) return;
  textLayerEl.classList.toggle("search-stale", highlightMode === "stale");
  const origin = host.getBoundingClientRect();
  const boxes: { r: DOMRect; ord: number }[] = [];
  const qlen = searchQuery.length;
  let ord = 0;
  for (const span of textLayerEl.querySelectorAll("span")) {
    const text = span.textContent;
    if (!text) continue;
    const hay = text.toLowerCase();
    if (!hay.includes(searchQuery)) continue;
    const node = span.firstChild;
    const textNode = node && node.nodeType === Node.TEXT_NODE ? (node as Text) : null;
    const addressable = !!(textNode && textNode.length >= qlen);
    for (
      let at = hay.indexOf(searchQuery);
      at !== -1;
      at = hay.indexOf(searchQuery, at + qlen)
    ) {
      const mine = ord;
      ord += 1;
      if (!addressable) continue;
      let rects: DOMRectList | undefined;
      try {
        if (!textNode) continue;
        const range = document.createRange();
        range.setStart(textNode, at);
        range.setEnd(textNode, at + qlen);
        rects = range.getClientRects();
        range.detach?.();
      } catch (_) {
        continue;
      }
      if (!rects) continue;
      for (const r of rects) {
        if (r.width <= 0 || r.height <= 0) continue;
        boxes.push({ r, ord: mine });
        if (boxes.length >= 200) break;
      }
      if (boxes.length >= 200) break;
    }
    if (boxes.length >= 200) break;
  }
  const activeOrd =
    activeMatch && activeMatch.page === st.page ? activeMatch.index : -1;
  const MAX_HIGHLIGHTS_PER_PAGE = 200;
  const painted = boxes.slice(0, MAX_HIGHLIGHTS_PER_PAGE);
  for (const { r, ord: n } of painted) {
    const d = document.createElement("div");
    d.className = n === activeOrd ? "highlight is-active" : "highlight";
    d.dataset.match = String(n);
    d.style.left = r.x - origin.x + "px";
    d.style.top = r.y - origin.y + "px";
    d.style.width = Math.max(1, r.width) + "px";
    d.style.height = Math.max(1, r.height) + "px";
    textLayerEl.appendChild(d);
  }
}

export function refreshHighlights(): void {
  for (const st of stateByCanvasId.values()) {
    if (st.textLayerEl) applyHighlights(st);
  }
}

function pageOutputScale(cssW: number, cssH: number): number {
  // Cap at 1.25: vectors stay sharp at reading distance; 1.5× cost ~30% more.
  // across mounted pages + bake intermediates. Text stays sharp enough.
  const dpr = Math.min(globalThis.devicePixelRatio || 1, 1.25);
  if (!(cssW > 0) || !(cssH > 0)) return dpr;

  const vw = globalThis.innerWidth || 0;
  const vh = globalThis.innerHeight || 0;
  const viewportBudget = vw > 0 && vh > 0 ? vw * vh * CANVAS_AREA_FACTOR : Infinity;
  const budget = Math.min(PAGE_MAX_PIXELS, viewportBudget);

  const capped = Math.sqrt(budget / (cssW * cssH));
  return Math.min(dpr, Math.max(0.5, capped));
}

async function destToPage(dest: string | unknown[] | null | undefined): Promise<number | null> {
  if (!pdf || !dest) return null;
  try {
    const explicit = typeof dest === "string" ? await pdf.getDestination(dest) : dest;
    if (!Array.isArray(explicit) || !explicit.length) return null;
    const ref = explicit[0];
    if (typeof ref === "object" && ref !== null) {
      return (await pdf.getPageIndex(ref)) + 1;
    }
    if (Number.isInteger(ref)) return (ref as number) + 1;
    return null;
  } catch (_) {
    return null;
  }
}

function safeExternalUrl(raw: string): string | null {
  if (typeof raw !== "string" || !raw) return null;
  let u: URL;
  try {
    u = new URL(raw, globalThis.location ? globalThis.location.href : undefined);
  } catch (_) {
    return null;
  }
  return ["http:", "https:", "mailto:"].includes(u.protocol) ? u.href : null;
}

async function buildLinkLayer(
  st: PageState,
  viewport: Viewport,
  page: PDFPageProxy | null
): Promise<void> {
  const { host } = st;
  if (!host) return;

  let annots: Annotation[] = [];
  try {
    const src = page || (await pdf!.getPage(st.page));
    annots = await src.getAnnotations({ intent: "display" });
    if (!page) {
      try { src.cleanup(); } catch (_) { /* ignore */ }
    }
  } catch (_) {
    annots = [];
  }

  const layer = document.createElement("div");
  layer.className = "linkLayer";

  for (const a of annots) {
    if (!a || a.subtype !== "Link" || !Array.isArray(a.rect)) continue;

    const url = safeExternalUrl(a.url ?? "");
    const linkPage = url ? null : await destToPage(a.dest ?? null);
    if (!url && !linkPage) continue;

    const [x1, y1] = viewport.convertToViewportPoint(a.rect[0]!, a.rect[1]!);
    const [x2, y2] = viewport.convertToViewportPoint(a.rect[2]!, a.rect[3]!);
    const x = Math.min(x1, x2);
    const y = Math.min(y1, y2);
    const w = Math.abs(x2 - x1);
    const h = Math.abs(y2 - y1);
    if (!(w > 0) || !(h > 0)) continue;

    const aEl = document.createElement("a");
    aEl.className = "pdf-link";
    aEl.style.left = x + "px";
    aEl.style.top = y + "px";
    aEl.style.width = w + "px";
    aEl.style.height = h + "px";

    if (url) {
      aEl.href = url;
      aEl.target = "_blank";
      aEl.rel = "noopener noreferrer";
      aEl.title = url;
    } else {
      aEl.href = "#";
      aEl.title = "Go to page " + linkPage;
      const p = linkPage!;
      aEl.dataset.page = String(p);
      aEl.addEventListener("click", (ev) => {
        ev.preventDefault();
        globalThis.dispatchEvent(
          new CustomEvent("pdfreader:navigate", { detail: { page: p } })
        );
      });
    }
    layer.appendChild(aEl);
  }

  const live = host.querySelector(".linkLayer");
  if (live && live.parentNode) {
    live.replaceWith(layer);
  } else {
    host.appendChild(layer);
  }
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

  const pipeline = scrubbing ? null : readPipeline();
  const needsBake = !scrubbing && pipeline ? !pipelineIsIdentity(pipeline) : false;
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
      blitInto(st.canvas, baked);
      if (baked !== target) releasePooledCanvas(baked);
    }
    if (st.rawCanvas && st.rawCanvas !== st.canvas && st.rawCanvas !== target) {
      releaseCanvas(st.rawCanvas);
    }
    st.rawCanvas = target;
    dropRawIfIdle(st);
  } else {
    // Identity / already-scrubbing: the live canvas IS the raw.
    st.rawCanvas = st.canvas;
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
