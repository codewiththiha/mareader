// =====================================================================
// pdfEngine.ts — window.PDFReader: an imperative pdf.js wrapper for the
// Leptos UI. Loaded as an ES module in index.html AFTER /vendor/pdfjs/pdf.min.mjs,
// so globalThis.pdfjsLib already exists.
//
// Design contract:
//  - All functions RESOLVE, never reject. Error shape: { ok:false, error:{name,message} }.
//  - Rust passes only strings/numbers/JSON + element *ids*; this module resolves
//    elements via getElementById. No DOM nodes cross the wasm boundary.
//  - The engine owns pdf.js state (loadingTask, PDFDocumentProxy, per-canvas
//    render/text state, search index, highlights) and fetches file bytes itself.
//
// This file is the TypeScript source; Trunk's pre-build hook compiles it to
// `public/pdfEngine.js` (ES2022 module) for the browser. The smoke test in
// `scripts/test-engine-smoke.ts` reads the source through `vm.runInContext`
// rather than the compiled output, so source-level changes are exercised
// directly.
// =====================================================================

export {};

// --- ambient globals -------------------------------------------------------
// pdf.js is loaded as an ESM script tag before this module runs, so we read
// the library off globalThis. The vendored types ship in pdfjs-dist's .d.ts
// (not currently a direct dependency of this repo), so the type is `unknown`
// here and cast at the only callsite that needs to pick members off it.
declare global {
  interface Window {
    pdfjsLib: unknown;
  }
  // eslint-disable-next-line no-var
  var pdfjsLib: unknown;
  // eslint-disable-next-line no-var
  var __TAURI__: TauriCore | undefined;
  // eslint-disable-next-line no-var
  var PDFReader: PDFReaderApi;
}

interface TauriCore {
  core: {
    invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;
    convertFileSrc: (p: string) => string;
  };
}

// --- minimal pdf.js shape --------------------------------------------------
// We only ever use a handful of pdf.js APIs. Defining a hand-rolled shape
// here keeps the engine self-contained and lets `tsc --strict` actually check
// the callsites rather than passing `any` through.
type PdfjsLib = {
  getDocument: (params: Record<string, unknown>) => LoadingTask;
  GlobalWorkerOptions: { workerSrc: string };
  TextLayer: new (opts: {
    textContentSource: { items: unknown[] };
    container: HTMLElement;
    viewport: Viewport;
  }) => TextLayerHandle;
};

type LoadingTask = {
  promise: Promise<PDFDocumentProxy>;
  destroy: () => Promise<void>;
};

type PDFDocumentProxy = {
  numPages: number;
  getPage: (n: number) => Promise<PDFPageProxy>;
  getMetadata: () => Promise<{ info?: { Title?: string | null; Author?: string | null } }>;
  getOutline: () => Promise<OutlineItem[] | null>;
  getPageIndex: (ref: unknown) => Promise<number>;
  getDestination: (name: string) => Promise<unknown[] | null>;
  cleanup: () => Promise<void>;
};

type PDFPageProxy = {
  getViewport: (opts: { scale: number }) => Viewport;
  render: (opts: {
    canvasContext: CanvasRenderingContext2D;
    viewport: Viewport;
    transform?: number[] | null;
  }) => RenderTask;
  getTextContent: () => Promise<{ items: TextItem[] }>;
  getAnnotations: (opts: { intent: string }) => Promise<Annotation[]>;
  cleanup: () => Promise<void>;
};

type RenderTask = { promise: Promise<void>; cancel: () => void };
type TextLayerHandle = { render: () => Promise<void>; cancel: () => void };

type Viewport = {
  width: number;
  height: number;
  convertToViewportPoint: (x: number, y: number) => [number, number];
};

type TextItem = {
  str: string;
  transform?: number[];
  width?: number;
  height?: number;
};

type OutlineItem = {
  title?: string | null;
  dest: string | unknown[];
  items?: OutlineItem[];
};

type Annotation = {
  subtype?: string;
  url?: string;
  dest?: string | unknown[];
  rect?: [number, number, number, number];
};

// --- engine state types ----------------------------------------------------
type MaybeCanvas = HTMLCanvasElement | ImageBitmap | null;
type Raster = HTMLCanvasElement | ImageBitmap;

type PageState = {
  page: number;
  canvas: HTMLCanvasElement | null;
  host: HTMLElement | null;
  textLayerEl: HTMLElement | null;
  renderTask: RenderTask | null;
  textLayer: TextLayerHandle | null;
  viewport: Viewport | null;
  scale: number;
  dead: boolean;
  rawCanvas: HTMLCanvasElement | null;
  queueGen: number;
  queueHandle: number;
};

type ThumbEntry = {
  raw: MaybeCanvas;
  display: MaybeCanvas;
  cssW: number;
  cssH: number;
  scale: number;
  gen: number;
  pending: Promise<MaybeCanvas> | null;
};

type PipelineCache = {
  token: string | null;
  filter: string;
  blend: string;
  paperInfo: PaperInfo | null;
  gen: number;
};

type PaperInfo = { color: string; rgb: [number, number, number] };

type FilterMatrix = { m: number[]; o: number[] };

type SearchRect = { x: number; y: number; w: number; h: number };
type TextIndexEntry = { str: string; x: number; y: number; w: number; h: number };
type SearchMatch = SearchRect & { page: number; index: number; text: string };
type ActiveMatch = { page: number; index: number } | null;

// Result wrappers used by every public API entry. The error variant is the
// universal shape Rust unwraps into a `Result<T, E>`; the ok variant carries
// whatever fields the specific call returns.
type Err = { ok: false; error: { name: string; message: string } };
type Ok<T extends Record<string, unknown>> = T & { ok: true };
type Result<T extends Record<string, unknown>> = Ok<T> | Err;

type OpenResult = Result<{
  numPages: number;
  title: string | null;
  author: string | null;
  outline: { title: string; page: number; depth: number }[];
  page1Size: { width: number; height: number };
  pageHeights: number[];
  pageWidths: number[];
}>;
type RenderResult = Result<{ width: number; height: number; scale: number }>;
type ThumbResult = Result<{ width: number; height: number; scale: number; cached: boolean }>;
type CoverResult = Result<{ dataUrl: string; width: number; height: number }>;
type Stats = {
  pages: number;
  thumbs: number;
  thumbLimit: number;
  thumbTasks: number;
};
type SearchResult = Result<{
  query: string;
  total: number;
  matches: SearchMatch[];
}>;

/// The PDFReader public API surface. Rust calls into this via wasm-bindgen
/// extern declarations (one per method); the shape here mirrors what those
/// externs expect.
type PDFReaderApi = {
  version: () => string;
  releaseAllSurfaces: () => void;
  open: (path: string) => Promise<OpenResult>;
  destroy: () => Promise<void>;
  registerPage: (payload: {
    canvasId: string;
    hostId: string;
    page: number;
  }) => void;
  unregisterPage: (canvasId: string) => void;
  cancelPage: (canvasId: string) => void;
  renderPage: (
    canvasId: string,
    scale: number,
    renderText: boolean
  ) => Promise<RenderResult>;
  renderThumb: (
    canvasId: string,
    page: number,
    scale: number
  ) => Promise<ThumbResult>;
  cancelThumb: (canvasId: string) => void;
  hasThumb: (page: number, scale: number) => boolean;
  blitThumb: (canvasId: string, page: number) => boolean;
  coverDataUrl: (path: string, maxWidth?: number) => Promise<CoverResult>;
  stats: () => Stats;
  buildSearchIndex: () => Promise<number>;
  search: (query: string) => Promise<SearchResult>;
  setActiveMatch: (page: number, index: number) => void;
  setHighlightMode: (mode: "live" | "stale") => void;
  clearHighlights: () => void;
  refreshTheme: () => Promise<void>;
  setScrubMode: (on: boolean) => Promise<void>;
  takePendingFile: () => Promise<string | null>;
};

// =====================================================================
// Implementation
// =====================================================================

const pdfjsLib = globalThis.pdfjsLib as unknown as PdfjsLib;
const { getDocument, GlobalWorkerOptions, TextLayer } = pdfjsLib;

GlobalWorkerOptions.workerSrc = "/vendor/pdfjs/pdf.worker.min.mjs";

const ENGINE_VERSION = "0.1.0";

// --- engine state ----------------------------------------------------
let loadingTask: LoadingTask | null = null;
let pdf: PDFDocumentProxy | null = null;
let numPages = 0;
let currentPath: string | null = null;

const stateByCanvasId = new Map<string, PageState>();
const thumbCache = new Map<number, ThumbEntry>();
const THUMB_CACHE_MAX = 24;
const thumbTasks = new Map<string, RenderTask>();
const thumbCancelled = new Set<string>();
const textIndex = new Map<number, TextIndexEntry[]>();
const highlightsByPage = new Map<number, SearchRect[]>();
let searchQuery = "";
let highlightMode: "live" | "stale" = "live";
let activeMatch: ActiveMatch = null;

let renderCount = 0;
const CLEANUP_EVERY = 5;

let scrubbing = false;

const PAGE_MAX_PIXELS = 8 * 1024 * 1024;
const CANVAS_AREA_FACTOR = 2.0;

// --- helpers ---------------------------------------------------------
const fail = (name: string, message: string): Err => ({
  ok: false,
  error: { name, message },
});

function errorInfo(e: unknown): { name: string; message: string } {
  const er = e as { name?: string; message?: string } | null;
  const name = (er && er.name) || "Error";
  const message = (er && er.message) || String(e);
  return { name, message };
}

function el(id: string): HTMLElement | null {
  if (typeof id !== "string" || !id) return null;
  return document.getElementById(id);
}

/// Force the browser to drop a canvas backing store.
function releaseCanvas(canvas: MaybeCanvas): void {
  if (!canvas) return;
  const c = canvas as HTMLCanvasElement;
  try {
    if (c.width !== 0) c.width = 0;
    if (c.height !== 0) c.height = 0;
  } catch (_) {
    /* detached / already gone */
  }
}

function releaseThumbEntry(entry: ThumbEntry | null | undefined): void {
  if (!entry) return;
  try {
    if (entry.display && typeof (entry.display as ImageBitmap).close === "function") {
      (entry.display as ImageBitmap).close();
    }
  } catch (_) {
    /* already closed */
  }
  const display = entry.display;
  const raw = entry.raw;
  entry.display = null;
  entry.raw = null;
  releaseCanvas(display);
  if (raw && raw !== display) releaseCanvas(raw);
}

function releasePageSurfaces(st: PageState | null): void {
  if (!st) return;
  if (st.host) {
    try {
      st.host.querySelectorAll("canvas").forEach(releaseCanvas);
      const text = st.host.querySelector(".textLayer");
      if (text) text.replaceChildren();
      const links = st.host.querySelector(".linkLayer");
      if (links) links.remove();
      st.host.querySelectorAll(".highlight").forEach((n) => n.remove());
      st.host.querySelectorAll(".page-snapshot").forEach((n) => n.remove());
    } catch (_) {
      /* host already detached */
    }
  }
  if (st.rawCanvas && st.rawCanvas !== st.canvas) releaseCanvas(st.rawCanvas);
  st.rawCanvas = null;
  releaseCanvas(st.canvas);
  st.canvas = null;
  st.host = null;
  st.textLayerEl = null;
  st.viewport = null;
}

function sweepPdf(): void {
  if (!pdf) return;
  try {
    Promise.resolve(pdf.cleanup()).catch(() => {
      /* advisory rejection, see comment above */
    });
  } catch (_) {
    /* ignore */
  }
}

// --- theme baking ----------------------------------------------------------
const pipelineCache: PipelineCache = {
  token: null,
  filter: "none",
  blend: "normal",
  paperInfo: null,
  gen: 0,
};

function readPipeline(): PipelineCache {
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
    /* fall back to identity */
  }
  pipelineCache.token = token;
  pipelineCache.filter = filter;
  pipelineCache.blend = blend;
  pipelineCache.paperInfo = null;
  pipelineCache.gen += 1;
  return pipelineCache;
}

function paperInfo(pipeline: PipelineCache): PaperInfo {
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
      const c = document.createElement("canvas");
      c.width = 1;
      c.height = 1;
      const ctx = c.getContext("2d");
      if (ctx) {
        ctx.fillStyle = resolved;
        ctx.fillRect(0, 0, 1, 1);
        const d = ctx.getImageData(0, 0, 1, 1).data;
        info.rgb = [d[0] ?? 255, d[1] ?? 255, d[2] ?? 255];
      }
    }
  } catch (_) {
    /* white paper */
  }
  pipeline.paperInfo = info;
  return info;
}

function pipelineIsIdentity(pipeline: PipelineCache): boolean {
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

  const out = document.createElement("canvas");
  out.width = w;
  out.height = h;
  const octx = out.getContext("2d", { alpha: false });
  if (!octx) return src;
  octx.putImageData(img, 0, 0);
  return out;
}

function bakeRaster(
  src: HTMLCanvasElement,
  pipeline: PipelineCache
): HTMLCanvasElement {
  if (pipelineIsIdentity(pipeline)) return src;

  let filtered: HTMLCanvasElement = src;
  if (pipeline.filter !== "none") {
    filtered = applyFilterPixels(src, pipeline.filter);
  }
  if (pipeline.blend === "normal") return filtered;

  const out = document.createElement("canvas");
  out.width = src.width;
  out.height = src.height;
  const octx = out.getContext("2d", { alpha: false });
  if (!octx) {
    if (filtered !== src) releaseCanvas(filtered);
    return filtered;
  }
  octx.fillStyle = paperInfo(pipeline).color;
  octx.fillRect(0, 0, out.width, out.height);
  octx.globalCompositeOperation = pipeline.blend as GlobalCompositeOperation;
  octx.drawImage(filtered, 0, 0);
  if (filtered !== src) releaseCanvas(filtered);
  return out;
}

function blitInto(
  dst: HTMLCanvasElement | null,
  src: Raster | null
): boolean {
  if (!dst || !src) return false;
  const srcW = (src as ImageBitmap).width ?? (src as HTMLCanvasElement).width;
  const srcH = (src as ImageBitmap).height ?? (src as HTMLCanvasElement).height;
  dst.width = srcW;
  dst.height = srcH;
  const ctx = dst.getContext("2d", { alpha: false });
  if (!ctx) return false;
  ctx.drawImage(src as CanvasImageSource, 0, 0);
  return true;
}

function releaseDisplayOnly(entry: ThumbEntry | null): void {
  if (!entry) return;
  try {
    if (entry.display && typeof (entry.display as ImageBitmap).close === "function") {
      (entry.display as ImageBitmap).close();
    }
  } catch (_) {
    /* already closed */
  }
  if (entry.display && entry.display !== entry.raw) releaseCanvas(entry.display);
  entry.display = null;
}

async function refreshTheme(): Promise<void> {
  try {
    await refreshThemeInternal();
  } catch (e) {
    const msg = (e as { message?: string })?.message ?? e;
    console.warn("[pdfEngine] refreshTheme failed:", msg);
  }
}

async function refreshThemeInternal(): Promise<void> {
  if (scrubbing) return;
  pipelineCache.token = null;
  const pipeline = readPipeline();

  for (const st of stateByCanvasId.values()) {
    if (st.dead || !st.canvas) continue;
    const raw = st.rawCanvas;
    if (!raw) continue;
    const baked = bakeRaster(raw, pipeline);
    if (baked !== st.canvas) {
      blitInto(st.canvas, baked);
      if (baked !== raw) releaseCanvas(baked);
    }
  }

  for (const [canvasId, { page }] of thumbLive) {
    const entry = thumbCache.get(page);
    const live = el(canvasId) as HTMLCanvasElement | null;
    if (!entry || !live) continue;
    if (!(await ensureEntryCurrent(entry))) continue;
    if (scrubbing) return;
    paintCached(live, entry);
  }
}

async function setScrubMode(on: boolean): Promise<void> {
  try {
    await setScrubModeInternal(on);
  } catch (e) {
    const msg = (e as { message?: string })?.message ?? e;
    console.warn("[pdfEngine] setScrubMode failed:", msg);
  }
}

async function setScrubModeInternal(on: boolean): Promise<void> {
  if (scrubbing === on) return;
  scrubbing = on;

  if (on) {
    // ENTERING scrub: swap every live page canvas and every mounted thumb
    // canvas to its RAW (un-themed) raster, and add the `appearance-scrubbing`
    // class to <html> in the SAME synchronous task. The class is what gates
    // `html.appearance-scrubbing .pdf-page canvas` / `.thumb-canvas` in CSS —
    // those rules apply the live `--canvas-filter` / `--canvas-blend` to the
    // canvas elements so the page re-colours per frame under the user's drag.
    // Doing the swap and the class toggle together is what makes the
    // compositor see one consistent frame: raw raster + live CSS filter.
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

  // LEAVING scrub: two-phase settle so no composited frame is ever
  // double-filtered (baked raster under the still-on live CSS filter) or
  // unfiltered (raw raster with the class already gone).
  //
  // Phase 1 (awaits): re-bake the mounted thumb cells' cache entries. Their
  // live canvases still hold RAW rasters and the scrub CSS class is still
  // on, so the screen stays consistent while this runs.
  const pipeline = readPipeline();
  for (const [canvasId, { page }] of thumbLive) {
    const entry = thumbCache.get(page);
    if (!entry) continue;
    if (!(await ensureEntryCurrent(entry))) continue;
    if (scrubbing) return; // a new scrub started mid-rebake; it owns the canvases
  }
  // Phase 2 (synchronous settle): bake the pages, repaint the thumb cells
  // and drop the scrub CSS class in ONE task.
  for (const st of stateByCanvasId.values()) {
    if (st.dead || !st.canvas) continue;
    const raw = st.rawCanvas;
    if (!raw) continue;
    const baked = bakeRaster(raw, pipeline);
    if (baked !== st.canvas) {
      blitInto(st.canvas, baked);
      if (baked !== raw) releaseCanvas(baked);
    }
  }
  for (const [canvasId, { page }] of thumbLive) {
    const entry = thumbCache.get(page);
    const live = el(canvasId) as HTMLCanvasElement | null;
    if (entry && live) paintCached(live, entry);
  }
  document.documentElement.classList.remove("appearance-scrubbing");
}

function pageOutputScale(cssW: number, cssH: number): number {
  const dpr = Math.min(globalThis.devicePixelRatio || 1, 2);
  if (!(cssW > 0) || !(cssH > 0)) return dpr;

  const vw = globalThis.innerWidth || 0;
  const vh = globalThis.innerHeight || 0;
  const viewportBudget = vw > 0 && vh > 0 ? vw * vh * CANVAS_AREA_FACTOR : Infinity;
  const budget = Math.min(PAGE_MAX_PIXELS, viewportBudget);

  const capped = Math.sqrt(budget / (cssW * cssH));
  return Math.min(dpr, Math.max(0.5, capped));
}

function thumbSource(entry: ThumbEntry | null | undefined): MaybeCanvas {
  if (!entry) return null;
  if (entry.display && (entry.display as ImageBitmap).width > 0) return entry.display;
  return null;
}

function thumbRaw(entry: ThumbEntry | null | undefined): MaybeCanvas {
  if (!entry) return null;
  if (entry.raw && (entry.raw as ImageBitmap).width > 0) return entry.raw;
  return thumbSource(entry);
}

async function ensureEntryCurrent(entry: ThumbEntry): Promise<MaybeCanvas> {
  if (scrubbing || entry.gen === pipelineCache.gen) {
    return entry.display && (entry.display as ImageBitmap).width > 0 ? entry.display : null;
  }
  if (entry.pending) return await entry.pending;
  entry.pending = (async () => {
    const raw = thumbRaw(entry);
    if (!raw) return null;
    if (entry.display !== entry.raw) releaseDisplayOnly(entry);
    const pipeline = readPipeline();
    const baked = bakeRaster(raw as HTMLCanvasElement, pipeline);
    entry.display = await cacheDisplay({ display: baked } as ThumbEntry);
    if (baked === raw) entry.raw = entry.display;
    entry.gen = pipelineCache.gen;
    return entry.display;
  })();
  const result = await entry.pending;
  entry.pending = null;
  return result;
}

async function cacheDisplay(entry: Pick<ThumbEntry, "display">): Promise<MaybeCanvas> {
  const off = entry.display;
  if (!off || typeof createImageBitmap !== "function") return off;
  try {
    const bitmap = await createImageBitmap(off as ImageBitmap);
    releaseCanvas(off);
    return bitmap;
  } catch (_) {
    return off;
  }
}

const thumbLive = new Map<string, { page: number }>();

function cachePut(page: number, entry: ThumbEntry): void {
  if (thumbCache.has(page)) {
    const prev = thumbCache.get(page);
    thumbCache.delete(page);
    if (prev && prev !== entry) releaseThumbEntry(prev);
  }
  thumbCache.set(page, entry);
  while (thumbCache.size > THUMB_CACHE_MAX) {
    const oldest = thumbCache.keys().next();
    if (oldest.done || oldest.value === undefined) break;
    const oldEntry = thumbCache.get(oldest.value);
    thumbCache.delete(oldest.value);
    if (oldEntry && oldEntry !== entry) releaseThumbEntry(oldEntry);
  }
}

function paintCached(
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

function hasThumb(page: number, scale: number): boolean {
  const hit = thumbCache.get(page);
  return (
    !!hit &&
    Math.abs(hit.scale - scale) < 1e-9 &&
    (scrubbing || hit.gen === pipelineCache.gen)
  );
}

function blitThumb(canvasId: string, page: number): boolean {
  const dst = el(canvasId) as HTMLCanvasElement | null;
  const entry = thumbCache.get(page);
  const src = scrubbing ? thumbRaw(entry ?? null) : thumbSource(entry ?? null);
  if (!dst || !src) return false;
  if (!scrubbing && entry && entry.gen !== pipelineCache.gen) return false;
  if (dst.width <= 0 || dst.height <= 0) {
    dst.width = (src as ImageBitmap).width;
    dst.height = (src as ImageBitmap).height;
  }
  const ctx = dst.getContext("2d");
  if (!ctx) return false;
  try {
    ctx.drawImage(src as CanvasImageSource, 0, 0, dst.width, dst.height);
    return true;
  } catch (_) {
    return false;
  }
}

function itemRect(item: TextItem, pageH: number): SearchRect {
  const t = item.transform || [1, 0, 0, 1, 0, 0];
  const fontSize = Math.hypot(t[2] ?? 0, t[3] ?? 0);
  const ascent = (fontSize || 0) * 0.8;
  return {
    x: t[4] ?? 0,
    y: (pageH || 0) - (t[5] ?? 0) - ascent,
    w: item.width || 0,
    h: item.height || 0,
  };
}

// --- bytes -----------------------------------------------------------
async function doFetch(src: string): Promise<Uint8Array> {
  const res = await fetch(src);
  if (!res.ok) {
    throw Object.assign(new Error("HTTP " + res.status), {
      name: "UnexpectedResponseException",
    });
  }
  return new Uint8Array(await res.arrayBuffer());
}

async function fetchBytes(path: string): Promise<Uint8Array> {
  if (/^https?:\/\//i.test(path)) return doFetch(path);
  const tauri = globalThis.__TAURI__;
  if (tauri && tauri.core && typeof tauri.core.invoke === "function") {
    try {
      const bytes = await tauri.core.invoke("read_file_bytes", { path });
      return new Uint8Array(bytes as ArrayBufferLike);
    } catch (_) {
      /* Not a readable filesystem path; fall through. */
    }
  }
  return doFetch(path);
}

function outlineTitle(raw: string | null | undefined): string {
  return String(raw == null ? "" : raw).trim() || "(untitled)";
}

async function flattenOutline(
  items: OutlineItem[] | null | undefined,
  depth: number,
  acc: { title: string; page: number; depth: number }[]
): Promise<typeof acc> {
  for (const it of items || []) {
    let page: number | null = null;
    try {
      if (Array.isArray(it.dest)) {
        const ref = it.dest[0];
        if (ref && typeof ref === "object" && "num" in ref) {
          const idx = await pdf!.getPageIndex(ref);
          page = idx + 1;
        } else if (typeof ref === "number") {
          page = ref + 1;
        }
      } else if (typeof it.dest === "string") {
        const d = await pdf!.getDestination(it.dest);
        if (d && d[0]) {
          const ref = d[0];
          if (ref && typeof ref === "object" && "num" in ref) {
            const idx = await pdf!.getPageIndex(ref);
            page = idx + 1;
          }
        }
      }
    } catch (_) {
      page = null;
    }
    if (page) acc.push({ title: outlineTitle(it.title), page, depth });
    await flattenOutline(it.items, depth + 1, acc);
  }
  return acc;
}

// --- public API ------------------------------------------------------
async function open(path: string): Promise<OpenResult> {
  try {
    await destroy();
    const bytes = await fetchBytes(path);
    loadingTask = getDocument({
      data: bytes,
      cMapUrl: "/vendor/pdfjs/cmaps/",
      cMapPacked: true,
      disableAutoFetch: true,
      disableStream: true,
    });
    pdf = await loadingTask.promise;
    numPages = pdf.numPages;
    currentPath = path;

    let title: string | null = null;
    let author: string | null = null;
    try {
      const meta = await pdf.getMetadata();
      title = (meta && meta.info && meta.info.Title) || null;
      author = (meta && meta.info && meta.info.Author) || null;
    } catch (_) {
      /* exotic docs */
    }

    let outline: { title: string; page: number; depth: number }[] = [];
    try {
      outline = await flattenOutline(await pdf.getOutline(), 0, []);
    } catch (_) {
      /* ignore */
    }

    const page1 = await pdf.getPage(1);
    const vp = page1.getViewport({ scale: 1 });

    const pageHeights: number[] = new Array(numPages);
    const pageWidths: number[] = new Array(numPages);
    pageHeights[0] = vp.height;
    pageWidths[0] = vp.width;
    for (let n = 2; n <= numPages; n += 1) {
      try {
        const pg = await pdf.getPage(n);
        const v = pg.getViewport({ scale: 1 });
        pageHeights[n - 1] = v.height;
        pageWidths[n - 1] = v.width;
        pg.cleanup();
      } catch (_) {
        pageHeights[n - 1] = vp.height;
        pageWidths[n - 1] = vp.width;
      }
    }
    try { page1.cleanup(); } catch (_) { /* ignore */ }

    return {
      ok: true,
      numPages,
      title,
      author,
      outline,
      page1Size: { width: vp.width, height: vp.height },
      pageHeights,
      pageWidths,
    };
  } catch (e) {
    const er = e as { name?: string };
    if (er && er.name === "PasswordException") {
      return fail("encrypted", "This PDF is password-protected.");
    }
    if (
      er &&
      (er.name === "InvalidPDFException" ||
        er.name === "MissingPDFException" ||
        er.name === "UnexpectedResponseException")
    ) {
      const d = errorInfo(e);
      return fail("corrupt", `Could not read this PDF. (${d.name}: ${d.message})`);
    }
    const info = errorInfo(e);
    return fail(info.name, info.message);
  }
}

async function destroy(): Promise<void> {
  for (const st of stateByCanvasId.values()) {
    st.dead = true;
    try { st.renderTask && st.renderTask.cancel(); } catch (_) { /* ignore */ }
    try { st.textLayer && st.textLayer.cancel(); } catch (_) { /* ignore */ }
    if (st.queueHandle) {
      cancelAnimationFrame(st.queueHandle);
      st.queueHandle = 0;
    }
    releasePageSurfaces(st);
  }
  stateByCanvasId.clear();
  for (const task of thumbTasks.values()) {
    try { task.cancel(); } catch (_) { /* ignore */ }
  }
  thumbTasks.clear();
  thumbCancelled.clear();
  thumbLive.clear();
  for (const entry of thumbCache.values()) releaseThumbEntry(entry);
  thumbCache.clear();
  textIndex.clear();
  highlightsByPage.clear();
  searchQuery = "";
  activeMatch = null;
  if (loadingTask) {
    const lt = loadingTask;
    loadingTask = null;
    Promise.resolve(lt.destroy()).catch(() => { /* fire-and-forget */ });
  }
  pdf = null;
  numPages = 0;
  currentPath = null;
}

function registerPage(payload: { canvasId: string; hostId: string; page: number }): void {
  const canvas = el(payload.canvasId) as HTMLCanvasElement | null;
  if (!canvas) return;
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
  const host = payload.hostId ? el(payload.hostId) : null;
  const textLayerEl = host ? host.querySelector(".textLayer") as HTMLElement | null : null;
  stateByCanvasId.set(payload.canvasId, {
    page: payload.page,
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
  });
}

function unregisterPage(canvasId: string): void {
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

function cancelPage(canvasId: string): void {
  const st = stateByCanvasId.get(canvasId);
  if (st && st.renderTask) {
    try { st.renderTask.cancel(); } catch (_) { /* ignore */ }
    st.renderTask = null;
  }
}

function applyHighlights(st: PageState): void {
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
      }
    }
  }
  const activeOrd =
    activeMatch && activeMatch.page === st.page ? activeMatch.index : -1;
  for (const { r, ord: n } of boxes) {
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

function refreshHighlights(): void {
  for (const st of stateByCanvasId.values()) {
    if (st.textLayerEl) applyHighlights(st);
  }
}

function setHighlightMode(mode: "live" | "stale"): void {
  highlightMode = mode === "stale" ? "stale" : "live";
  const stale = highlightMode === "stale";
  for (const st of stateByCanvasId.values()) {
    if (st.textLayerEl) st.textLayerEl.classList.toggle("search-stale", stale);
  }
}

function setActiveMatch(page: number, index: number): void {
  activeMatch =
    Number.isFinite(page) && page > 0 && Number.isFinite(index) && index >= 0
      ? { page, index: index | 0 }
      : null;
  for (const st of stateByCanvasId.values()) {
    if (!st.textLayerEl) continue;
    const wanted = activeMatch && activeMatch.page === st.page ? String(activeMatch.index) : null;
    for (const d of st.textLayerEl.querySelectorAll(".highlight") as NodeListOf<HTMLElement>) {
      d.classList.toggle("is-active", wanted !== null && d.dataset.match === wanted);
    }
  }
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

async function renderPageInternal(
  canvasId: string,
  scale: number,
  renderText: boolean
): Promise<RenderResult> {
  const st = stateByCanvasId.get(canvasId);
  if (!st) return fail("not_registered", "Page not registered: " + canvasId);
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
    st.rawCanvas = target;
    const baked = bakeRaster(target, pipeline);
    if (baked !== st.canvas) {
      blitInto(st.canvas, baked);
      if (baked !== target) releaseCanvas(baked);
    }
  } else {
    st.rawCanvas = st.canvas;
  }

  if (renderText && st.host && st.textLayerEl) {
    st.host.style.setProperty("--scale-factor", String(scale));

    const layer = document.createElement("div");
    layer.className = "textLayer";
    layer.setAttribute("aria-hidden", "true");

    const textContent = await page.getTextContent();

    const tl = new TextLayer({
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

  renderCount += 1;
  if (renderCount % CLEANUP_EVERY === 0) sweepPdf();

  return { ok: true, width: cssW, height: cssH, scale };
}

async function renderPage(
  canvasId: string,
  scale: number,
  renderText: boolean
): Promise<RenderResult> {
  const st = stateByCanvasId.get(canvasId);
  if (!st) return fail("not_registered", "Page not registered: " + canvasId);
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

async function renderThumb(
  canvasId: string,
  page: number,
  scale: number
): Promise<ThumbResult> {
  try {
    return await renderThumbInternal(canvasId, page, scale);
  } catch (e) {
    const info = errorInfo(e);
    return fail(info.name, info.message);
  }
}

async function renderThumbInternal(
  canvasId: string,
  page: number,
  scale: number
): Promise<ThumbResult> {
  const canvas = el(canvasId) as HTMLCanvasElement | null;
  if (!canvas) return fail("no_canvas", "No canvas: " + canvasId);
  if (!pdf) return fail("no_document", "No document open");

  const hit = thumbCache.get(page);
  if (hit && Math.abs(hit.scale - scale) < 1e-9) {
    if (scrubbing) {
      if (blitInto(canvas, thumbRaw(hit))) {
        cachePut(page, hit);
        thumbLive.set(canvasId, { page });
        return { ok: true, width: hit.cssW, height: hit.cssH, scale, cached: true };
      }
    } else if (hit.gen === pipelineCache.gen) {
      const size = paintCached(canvas, hit);
      if (size) {
        cachePut(page, hit);
        thumbLive.set(canvasId, { page });
        return { ok: true, width: size.width, height: size.height, scale, cached: true };
      }
    } else if (await ensureEntryCurrent(hit)) {
      const size = paintCached(canvas, hit);
      if (size) {
        cachePut(page, hit);
        thumbLive.set(canvasId, { page });
        return { ok: true, width: size.width, height: size.height, scale, cached: false };
      }
    }
  }

  try { const t = thumbTasks.get(canvasId); if (t) t.cancel(); } catch (_) { /* ignore */ }
  thumbTasks.delete(canvasId);
  thumbCancelled.delete(canvasId);

  try {
    const pg = await pdf.getPage(page);
    const viewport = pg.getViewport({ scale });
    const out = 1;
    const cssW = Math.floor(viewport.width);
    const cssH = Math.floor(viewport.height);

    const off = document.createElement("canvas");
    off.width = Math.max(1, Math.floor(viewport.width * out));
    off.height = Math.max(1, Math.floor(viewport.height * out));
    const ctx = off.getContext("2d", { alpha: false });
    if (!ctx) return fail("no_context", "No 2d context");
    const transform = out !== 1 ? [out, 0, 0, out, 0, 0] : null;

    const task = pg.render({ canvasContext: ctx, viewport, transform });
    thumbTasks.set(canvasId, task);
    try {
      await task.promise;
    } catch (e) {
      thumbTasks.delete(canvasId);
      releaseCanvas(off);
      try { pg.cleanup(); } catch (_) { /* ignore */ }
      if ((e as { name?: string }).name === "RenderingCancelledException") {
        return fail("cancelled", "Thumb render cancelled");
      }
      const info = errorInfo(e);
      return fail(info.name, info.message);
    }
    thumbTasks.delete(canvasId);
    pg.cleanup();

    const raw = off;
    let rawRaster: MaybeCanvas = raw;
    let display: MaybeCanvas = scrubbing ? raw : bakeRaster(raw, readPipeline());
    if (display === raw && typeof createImageBitmap === "function") {
      try {
        const bitmap = await createImageBitmap(raw);
        releaseCanvas(raw);
        display = bitmap;
        rawRaster = bitmap;
      } catch (_) { /* keep the canvas */ }
    } else if (display !== raw) {
      display = await cacheDisplay({ display } as ThumbEntry);
    }
    const entry: ThumbEntry = {
      raw: rawRaster,
      display,
      cssW,
      cssH,
      scale,
      gen: scrubbing ? -1 : pipelineCache.gen,
      pending: null,
    };
    cachePut(page, entry);

    if (!thumbCancelled.has(canvasId)) {
      const live = el(canvasId) as HTMLCanvasElement | null;
      if (live) {
        thumbLive.set(canvasId, { page });
        paintCached(live, entry);
      }
    }
    thumbCancelled.delete(canvasId);

    return { ok: true, width: cssW, height: cssH, scale, cached: false };
  } catch (e) {
    thumbTasks.delete(canvasId);
    const info = errorInfo(e);
    return fail(info.name, info.message);
  }
}

function renderCoverFromPdf(
  doc: PDFDocumentProxy,
  maxWidth: number
): Promise<{ dataUrl: string; width: number; height: number }> {
  return doc.getPage(1).then((page) => {
    const vp1 = page.getViewport({ scale: 1 });
    const scale = Math.min((maxWidth || 240) / (vp1.width || 1), 2);
    const viewport = page.getViewport({ scale });

    const off = document.createElement("canvas");
    off.width = Math.max(1, Math.floor(viewport.width));
    off.height = Math.max(1, Math.floor(viewport.height));
    const ctx = off.getContext("2d", { alpha: false });
    if (!ctx) throw new Error("no_context");
    return page
      .render({ canvasContext: ctx, viewport })
      .promise.then(() => {
        try { page.cleanup(); } catch (_) { /* ignore */ }
        const dataUrl = off.toDataURL("image/jpeg", 0.82);
        return { dataUrl, width: viewport.width, height: viewport.height };
      });
  });
}

async function coverDataUrl(path: string, maxWidth = 240): Promise<CoverResult> {
  try {
    if (!path) return fail("no_path", "No path");
    let result: { dataUrl: string; width: number; height: number };
    if (pdf && currentPath === path) {
      result = await renderCoverFromPdf(pdf, maxWidth);
    } else {
      const bytes = await fetchBytes(path);
      const task = getDocument({
        data: bytes,
        cMapUrl: "/vendor/pdfjs/cmaps/",
        cMapPacked: true,
        disableAutoFetch: true,
        disableStream: true,
      });
      try {
        const doc = await task.promise;
        result = await renderCoverFromPdf(doc, maxWidth);
      } finally {
        try { await task.destroy(); } catch (_) { /* ignore */ }
      }
    }
    return { ok: true, dataUrl: result.dataUrl, width: result.width, height: result.height };
  } catch (e) {
    const info = errorInfo(e);
    return fail(info.name, info.message);
  }
}

function cancelThumb(canvasId: string): void {
  const task = thumbTasks.get(canvasId);
  if (task) {
    try { task.cancel(); } catch (_) { /* ignore */ }
    thumbTasks.delete(canvasId);
  }
  thumbCancelled.add(canvasId);
  thumbLive.delete(canvasId);
  releaseCanvas(el(canvasId) as HTMLCanvasElement | null);
}

function stats(): Stats {
  return {
    pages: stateByCanvasId.size,
    thumbs: thumbCache.size,
    thumbLimit: THUMB_CACHE_MAX,
    thumbTasks: thumbTasks.size,
  };
}

async function buildSearchIndex(): Promise<number> {
  if (!pdf) return 0;
  textIndex.clear();
  let count = 0;
  for (let n = 1; n <= numPages; n += 1) {
    try {
      const page = await pdf.getPage(n);
      const tc = await page.getTextContent();
      const pageH = page.getViewport({ scale: 1 }).height;
      const items: TextIndexEntry[] = [];
      for (const item of tc.items) {
        if (!item.str) continue;
        const r = itemRect(item, pageH);
        if (r.w <= 0) continue;
        items.push({ str: item.str, x: r.x, y: r.y, w: r.w, h: r.h });
      }
      textIndex.set(n, items);
      count += items.length;
      page.cleanup();
    } catch (_) { /* skip unreadable page */ }
  }
  return count;
}

async function search(query: string): Promise<SearchResult> {
  if (!pdf) return fail("no_document", "No document open");
  const q = String(query || "").toLowerCase().trim();
  if (!q) {
    searchQuery = "";
    activeMatch = null;
    highlightsByPage.clear();
    return { ok: true, query: "", total: 0, matches: [] };
  }

  searchQuery = q;
  highlightMode = "live";
  highlightsByPage.clear();
  const matches: SearchMatch[] = [];
  const qlen = q.length;

  for (const page of [...textIndex.keys()].sort((a, b) => a - b)) {
    const items = textIndex.get(page) || [];
    const pageMatches: SearchRect[] = [];
    let ord = 0;
    for (const item of items) {
      const lower = item.str.toLowerCase();
      const len = lower.length || 1;
      for (
        let at = lower.indexOf(q);
        at !== -1;
        at = lower.indexOf(q, at + qlen)
      ) {
        const rect: SearchRect = {
          x: item.x + (item.w * at) / len,
          y: item.y,
          w: Math.max(1, (item.w * qlen) / len),
          h: item.h,
        };
        pageMatches.push(rect);
        matches.push({
          page,
          index: ord,
          text: snippetText(item.str, q, at),
          ...rect,
        });
        ord += 1;
      }
    }
    if (pageMatches.length) highlightsByPage.set(page, pageMatches);
  }

  activeMatch = null;
  refreshHighlights();
  return { ok: true, query, total: matches.length, matches };
}

function snippetText(str: string, q: string, from: number | undefined): string {
  const idx = from === undefined ? str.toLowerCase().indexOf(q) : from;
  const start = Math.max(0, idx - 25);
  const end = Math.min(str.length, idx + q.length + 30);
  const pre = start > 0 ? "…" : "";
  const post = end < str.length ? "…" : "";
  return pre + str.slice(start, end) + post;
}

function clearHighlights(): void {
  highlightsByPage.clear();
  searchQuery = "";
  activeMatch = null;
  highlightMode = "live";
  for (const st of stateByCanvasId.values()) {
    if (st.host) {
      st.host.querySelectorAll(".highlight").forEach((n) => n.remove());
    }
    if (st.textLayerEl) st.textLayerEl.classList.remove("search-stale");
  }
}

async function takePendingFile(): Promise<string | null> {
  const tauri = globalThis.__TAURI__;
  if (!tauri || !tauri.core || typeof tauri.core.invoke !== "function") {
    return null;
  }
  try {
    const path = await tauri.core.invoke("take_pending_file");
    return typeof path === "string" && path ? path : null;
  } catch (_) {
    return null;
  }
}

function releaseAllSurfaces(): void {
  for (const st of stateByCanvasId.values()) {
    st.dead = true;
    try { st.renderTask && st.renderTask.cancel(); } catch (_) { /* ignore */ }
    try { st.textLayer && st.textLayer.cancel(); } catch (_) { /* ignore */ }
    if (st.queueHandle) {
      cancelAnimationFrame(st.queueHandle);
      st.queueHandle = 0;
    }
    releasePageSurfaces(st);
  }
  for (const entry of thumbCache.values()) releaseThumbEntry(entry);
  try {
    document.querySelectorAll("canvas").forEach((c) => releaseCanvas(c as HTMLCanvasElement));
  } catch (_) { /* document already torn down */ }
}

globalThis.addEventListener("pagehide", releaseAllSurfaces);

// --- selection page-range tracking (for virtualization pinning) -------------
// While the reader drags across pages, the selection's anchor and focus land
// on different .pdf-page hosts. We walk up from each endpoint to find its
// page index, compute the inclusive range, and dispatch a CustomEvent so the
// Rust side can pin those pages in the virtualization window — otherwise
// scrolling would evict the page the selection started on, orphaning its DOM
// nodes and breaking copy of multi-page selections.
//
// NO CLAMP. The previous version had a `selectionchange` clamp that called
// `sel.removeAllRanges(); sel.addRange(lastGoodRange)` whenever the focus
// left the .textLayer (e.g. crossing the 24px inter-page gap). In WebKit/Blink,
// calling `removeAllRanges()` mid-drag DESTROYS active drag-selection tracking
// — the selection jumps, stutters, and the reader cannot drag across pages
// smoothly. Removing the clamp entirely lets the browser handle selection
// naturally; the CSS `user-select` scoping (`.textLayer` is non-selectable,
// only the glyph spans are `user-select: text`) prevents the "selection
// spread" the clamp was originally written to prevent.
//
// PIN PRESERVATION DURING GAPS. When `findPageNumber` returns null (the
// focus is in an inter-page gap), the previous version immediately dispatched
// `detail: null`, which cleared `selected_pages` in Rust, which unmounted
// the anchor's page — destroying the selection. Now we preserve the last
// known anchor/focus page via `lastKnownAnchorPage`/`lastKnownFocusPage` so
// the pin stays alive across gaps. Only clear when `!selDragging` (the drag
// is truly over).
let selDragging = false;
let lastKnownAnchorPage: number | null = null;
let lastKnownFocusPage: number | null = null;
let lastSelectionRangeKey: string | null = null;

function findPageNumber(node: Node | null): number | null {
  if (!node) return null;
  const el = node.nodeType === Node.TEXT_NODE
    ? node.parentElement
    : (node as HTMLElement | null);
  if (!el) return null;
  // Try the .pdf-page host first (cont-{i}-pg).
  const pageEl = el.closest(".pdf-page");
  if (pageEl && pageEl.id) {
    const m = /^cont-(\d+)-pg$/.exec(pageEl.id);
    if (m && m[1]) return parseInt(m[1], 10) + 1;
  }
  // Fall back to the wrapper div (cont-{i}-wrap). During a drag across the
  // inter-page gap, the focus may land on the wrapper rather than the host.
  const wrapEl = el.closest("[id^='cont-'][id$='-wrap']");
  if (wrapEl && wrapEl.id) {
    const m = /^cont-(\d+)-wrap$/.exec(wrapEl.id);
    if (m && m[1]) return parseInt(m[1], 10) + 1;
  }
  return null;
}

function dispatchSelectionPages(): void {
  const sel = document.getSelection();
  if (!sel || sel.rangeCount === 0 || sel.isCollapsed) {
    // Only clear when NOT dragging — during a drag, a momentary collapsed
    // state (e.g. crossing a gap) must not unpin the anchor's page.
    if (!selDragging && lastSelectionRangeKey !== null) {
      lastSelectionRangeKey = null;
      lastKnownAnchorPage = null;
      lastKnownFocusPage = null;
      globalThis.dispatchEvent(
        new CustomEvent("pdfreader:selection-pages", { detail: null })
      );
    }
    return;
  }

  // Use the last known page if the current endpoint is in a gap (null).
  // This keeps the pin alive so the page doesn't get evicted mid-drag.
  const anchorPage = findPageNumber(sel.anchorNode) ?? lastKnownAnchorPage;
  const focusPage = findPageNumber(sel.focusNode) ?? lastKnownFocusPage;

  if (anchorPage !== null) lastKnownAnchorPage = anchorPage;
  if (focusPage !== null) lastKnownFocusPage = focusPage;

  if (anchorPage === null || focusPage === null) {
    // Both endpoints are in gaps and we have no prior page — don't clear
    // (that would unpin), just wait for the next selectionchange.
    return;
  }

  const first = Math.min(anchorPage, focusPage);
  const last = Math.max(anchorPage, focusPage);
  const key = `${first}-${last}`;
  if (key === lastSelectionRangeKey) return;
  lastSelectionRangeKey = key;
  globalThis.dispatchEvent(
    new CustomEvent("pdfreader:selection-pages", {
      detail: { first, last },
    })
  );
}

document.addEventListener("mousedown", (e) => {
  const t = e.target as HTMLElement | null;
  if (t && t.closest && t.closest(".textLayer")) {
    selDragging = true;
  }
});

window.addEventListener("mouseup", () => {
  selDragging = false;
  // One final dispatch in case the selection changed on the last mousemove
  // but the selectionchange event hasn't fired yet. Also handles the case
  // where the drag ended in a gap — the `!selDragging` guard in
  // `dispatchSelectionPages` now allows clearing.
  dispatchSelectionPages();
});

document.addEventListener("selectionchange", () => {
  // Always dispatch the selection's page range so the Rust side can pin
  // those pages in the virtualization window.
  dispatchSelectionPages();
});

globalThis.PDFReader = {
  version: () => ENGINE_VERSION,
  releaseAllSurfaces,
  open,
  destroy,
  registerPage,
  unregisterPage,
  cancelPage,
  renderPage,
  renderThumb,
  cancelThumb,
  hasThumb,
  blitThumb,
  coverDataUrl,
  stats,
  buildSearchIndex,
  search,
  setActiveMatch,
  setHighlightMode,
  clearHighlights,
  refreshTheme,
  setScrubMode,
  takePendingFile,
} satisfies PDFReaderApi;
