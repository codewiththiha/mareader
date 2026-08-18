// =====================================================================
// pdfEngine.js — window.PDFReader: an imperative pdf.js wrapper for the
// Leptos UI. Loaded as an ES module in index.html AFTER /vendor/pdfjs/pdf.min.mjs,
// so globalThis.pdfjsLib already exists.
//
// Design contract:
//  - All functions RESOLVE, never reject. Error shape: { ok:false, error:{name,message} }.
//  - Rust passes only strings/numbers/JSON + element *ids*; this module resolves
//    elements via getElementById. No DOM nodes cross the wasm boundary.
//  - The engine owns pdf.js state (loadingTask, PDFDocumentProxy, per-canvas
//    render/text state, search index, highlights) and fetches file bytes itself.
// =====================================================================

const { getDocument, GlobalWorkerOptions, TextLayer } = globalThis.pdfjsLib;

GlobalWorkerOptions.workerSrc = "/vendor/pdfjs/pdf.worker.min.mjs";

const ENGINE_VERSION = "0.1.0";

// --- engine state ----------------------------------------------------
let loadingTask = null;
let pdf = null; // PDFDocumentProxy
let numPages = 0;
// Path the CURRENT document was opened from (null when none). coverDataUrl
// uses it to tell whether a requested cover can be rendered from the live
// document or needs its own throwaway open.
let currentPath = null;

// canvasId -> { page, canvas, host, textLayerEl, renderTask, textLayer, viewport, scale }
const stateByCanvasId = new Map();
// --- thumbnail bitmap cache ------------------------------------------------
// page (1-based) -> { bitmap|canvas, cssW, cssH, scale }
//
// Thumbnails are virtualized: a cell UNMOUNTS when it scrolls out of the grid
// window and REMOUNTS when it scrolls back. Without a cache every remount
// restarts a full pdf.js render, so a row re-entering the viewport showed its
// loading skeleton and then crossfaded to the painted canvas — the subtle
// per-row brightness flicker on scroll. The cache keeps the finished bitmap of
// every page the user has already seen, so a remount can blit it into the fresh
// canvas SYNCHRONOUSLY (see paintCached) — painted on the first frame, no
// skeleton, no crossfade, no flicker.
//
// LRU-bounded: entries are re-inserted on hit (Map preserves insertion order)
// and the oldest are dropped past THUMB_CACHE_MAX. 48 is ~4 screens of the
// 2-column grid — enough that a remount inside normal browsing is still a
// sync blit — while pinning a quarter fewer full bitmaps than 64. Evicted
// entries are released immediately (ImageBitmap.close / canvas width=0);
// just deleting the Map key is not enough in WKWebView.
//
// Stored as ImageBitmap when the browser allows it so eviction can free GPU
// memory synchronously. Falls back to a detached canvas.
const thumbCache = new Map();
// 24 thumbnails is still 2 screens; 48 was ~4 screens but each entry holds
// both a raw and a display raster, so the cap matters.
const THUMB_CACHE_MAX = 24;
// canvasId -> in-flight thumbnail RenderTask (cancelled by cancelThumb).
const thumbTasks = new Map();
// canvasId -> the cell unmounted while a render was in flight. The slow path
// still caches the finished frame (so a remount is instant) but must NOT
// re-allocate a backing store on a canvas Leptos is about to drop.
const thumbCancelled = new Set();
// page (1-based) -> [{ str, x, y, w, h }] searchable text with rects in TOP-origin
// scale-1 CSS px (see itemRect). Used for search matching + the results API.
const textIndex = new Map();
// page (1-based) -> [{ x, y, w, h }] match rects (top-origin scale-1) fed to the
// search results JSON. NOTE: on-page highlight boxes are NOT drawn from these —
// applyHighlights derives them from the rendered DOM spans so they always align
// pixel-perfectly with the text regardless of font/scale rounding.
const highlightsByPage = new Map();
// Lowercased, trimmed query from the last search(); drives DOM-derived highlights.
let searchQuery = "";
// "live" while the reader is searching, "stale" once they dismiss the bar. A
// stale pass keeps the boxes on screen in a muted colour — the search is over,
// but the reader can still see where the hits were, and reopening the bar
// brings the query back. See setHighlightMode.
let highlightMode = "live";
// The one match the reader is currently sitting on: `{ page, index }`, where
// `index` counts MATCHES (not pages) within that page, in reading order.
// applyHighlights stamps the same counter onto every box it paints, so the
// active match keeps its distinct styling across re-renders and remounts.
let activeMatch = null;

let renderCount = 0;
// pdf.js keeps operator lists / decoded images per page. Once a page has
// left the window we want those gone; every-N is a backstop for pages that
// stay mounted across many re-renders (zoom).
const CLEANUP_EVERY = 5;

// --- theme baking state ----------------------------------------------------
// True while an appearance slider scrub (tint hue/strength, texture, grain)
// is being dragged. During a scrub the theme CSS variables change EVERY
// FRAME, and the reader expects the page to re-colour live under the slider.
// A per-frame re-bake of full-resolution rasters cannot keep up, so scrub
// mode falls back to the classic pipeline for the duration of the drag: the
// RAW raster is shown with the CSS filter/blend re-applied (compositor-side,
// free per frame — exactly what the old `.scrubbing`-less CSS did), and the
// baked rasters come back the moment the gesture pauses. The look is
// pixel-identical in both modes; only the WHERE of the filter/blend changes.
let scrubbing = false;

// Cap one page's RGBA backing store. At 5× zoom on a retina display an
// uncapped letter page is ~48MP / 192MB; 8MP is 32MB and still sharp.
//
// This is the ABSOLUTE ceiling. `CANVAS_AREA_FACTOR` below is usually the
// binding constraint, and is the one that keeps memory flat under zoom.
const PAGE_MAX_PIXELS = 8 * 1024 * 1024;

// Cap a page raster relative to the VIEWPORT rather than to the page.
//
// A fixed per-page pixel budget is the wrong shape for a zooming viewer: it
// spends the same 32MB on a page whether the reader can see all of it or a
// tenth of it, so zooming in costs more and more memory for pixels that are
// scrolled off screen. pdf.js solves this with `capCanvasAreaFactor` (200% of
// the viewport by default) and falls back to CSS scaling beyond it; the same
// idea applies here.
//
// 2.0 means "never rasterise more than two viewports' worth of pixels for one
// page". At 1280x900 that is ~2.3MP / 9MB, and it does not grow with zoom —
// the CSS box keeps stretching the bitmap, exactly as it already does between
// the zoom gesture and the crisp re-render.
//
// Raise it for sharper deep zoom at proportionally more memory; 1.0 is the
// leanest setting that still covers a full screen of page.
const CANVAS_AREA_FACTOR = 2.0;

// --- helpers ---------------------------------------------------------
const fail = (name, message) => ({ ok: false, error: { name, message } });

function errorInfo(e) {
  const name = (e && e.name) || "Error";
  const message = (e && e.message) || String(e);
  return { name, message };
}

function el(id) {
  if (typeof id !== "string" || !id) return null;
  return document.getElementById(id);
}

/// Force the browser to drop a canvas backing store.
///
/// Removing a <canvas> from the DOM is NOT enough in WKWebView (Tauri): the
/// IOSurface stays allocated until a later GC, so RAM grows with every page
/// the reader has scrolled past — 400MB, then 800MB, then 1GB. Assigning
/// width/height to 0 releases the buffer immediately.
function releaseCanvas(canvas) {
  if (!canvas) return;
  try {
    if (canvas.width !== 0) canvas.width = 0;
    if (canvas.height !== 0) canvas.height = 0;
  } catch (_) { /* detached / already gone */ }
}

/// Drop a cached thumbnail's GPU buffers. ImageBitmap.close() is synchronous;
/// a leftover canvas is zeroed the same way as a live page. Releases BOTH the
/// baked display raster and the un-themed raw raster kept for re-bakes.
function releaseThumbEntry(entry) {
  if (!entry) return;
  try {
    if (entry.display && typeof entry.display.close === "function") {
      entry.display.close();
    }
  } catch (_) { /* already closed */ }
  const display = entry.display;
  const raw = entry.raw;
  entry.display = null;
  entry.raw = null;
  releaseCanvas(display);
  // When the pipeline is identity the raw raster IS the display raster, so
  // never zero the same canvas twice.
  if (raw && raw !== display) releaseCanvas(raw);
}

/// Zero every canvas inside a page host (the live bitmap AND any zoom
/// snapshot) and drop the text/link layers the state object is keeping alive.
function releasePageSurfaces(st) {
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
    } catch (_) { /* host already detached */ }
  }
  // The un-themed raw raster (kept for theme re-bakes and scrub mode). When
  // the theme pipeline is identity it IS the live canvas, so never zero a
  // canvas twice.
  if (st.rawCanvas && st.rawCanvas !== st.canvas) releaseCanvas(st.rawCanvas);
  st.rawCanvas = null;
  releaseCanvas(st.canvas);
  st.canvas = null;
  st.host = null;
  st.textLayerEl = null;
  st.viewport = null;
}

/// Ask pdf.js to drop cached operator lists / decoded images for pages that
/// are no longer rendering.
///
/// `pdf.cleanup()` returns a PROMISE that REJECTS with "startCleanup: Page N is
/// currently rendering." whenever any page still has a live render task. A
/// `try/catch` around the call cannot catch that — the throw happens later, on
/// the microtask queue — so every sweep that overlapped an in-flight render
/// surfaced as an unhandled rejection in the console (measured: 10+ per scroll
/// through the sample document).
///
/// The rejection is advisory, not a failure: pdf.js simply declines to sweep
/// this time, and the next sweep succeeds once the render settles. So we attach
/// a no-op catch rather than papering over it with a synchronous try/catch that
/// never fires.
function sweepPdf() {
  if (!pdf) return;
  try {
    Promise.resolve(pdf.cleanup()).catch(() => {});
  } catch (_) {}
}

// --- theme baking ----------------------------------------------------------
// The page/thumbnail canvases used to carry the theme as LIVE CSS:
//
//     canvas { filter: var(--canvas-filter);
//              mix-blend-mode: var(--canvas-blend); }
//
// over the paper-coloured host. That made every canvas TWO or THREE
// compositor surfaces — the texture, a full-resolution filter intermediate,
// and a blend group for the backdrop — and the compositor re-rasterised
// them on EVERY size change: each scroll that mounts a page, each frame of
// a sidebar slide (the host width animates), each frame of a zoom stretch.
// That churn is what pushed the webview's GPU process above the renderer
// process in memory, on every OS.
//
// The same result can be computed ONCE and baked into the raster: apply the
// filter, then blend over the paper colour, and display a plain opaque
// texture. Colours are byte-identical (the same filter + blend ops the
// compositor would run), sharpness is untouched (full resolution, no
// scaling — the render scale / DPR budget is unchanged), and a theme change
// re-bakes the rasters we already hold in a few drawImage calls instead of
// re-rendering every page. Only while an appearance slider scrub is in
// flight do we fall back to the live-CSS pipeline (see `scrubbing`), so the
// page still re-colours live under the user's drag.

/// Cached pipeline read. Invalidated whenever the theme applier rewrites the
/// inline style on <html> (it writes the CSS variables there on every
/// appearance change), so renders and re-bakes always see the live theme.
/// `gen` is a monotonically increasing stamp bumped on every invalidation;
/// thumbnail cache entries carry the gen they were baked at, so a theme
/// change marks every cached entry stale and they re-bake lazily on next use
/// instead of re-baking the whole cache up front.
const pipelineCache = { token: null, filter: "none", blend: "normal", paperInfo: null, gen: 0 };

function readPipeline() {
  const root = document.documentElement;
  const token = root.getAttribute("style") || "";
  if (pipelineCache.token === token) return pipelineCache;
  let filter = "none";
  let blend = "normal";
  try {
    const cs = getComputedStyle(root);
    filter = (cs.getPropertyValue("--canvas-filter") || "none").trim() || "none";
    blend = (cs.getPropertyValue("--canvas-blend") || "normal").trim() || "normal";
  } catch (_) { /* fall back to identity */ }
  pipelineCache.token = token;
  pipelineCache.filter = filter;
  pipelineCache.blend = blend;
  pipelineCache.paperInfo = null;
  pipelineCache.gen += 1;
  return pipelineCache;
}

/// Resolve --color-paper to a concrete colour (string for fillStyle) plus its
/// RGB triple. The custom property may hold `var(--base-paper)` (custom
/// properties are not substituted at read time) or an oklch() tint, so it is
/// resolved through a throwaway probe element and a 1×1 canvas sample instead
/// of parsing colour strings. The RGB triple lets `pipelineIsIdentity`
/// recognise multiply-over-pure-white — the default light theme — as an
/// identity pipeline, so the most common theme pays nothing per render.
function paperInfo(pipeline) {
  if (pipeline.paperInfo) return pipeline.paperInfo;
  const info = { color: "#ffffff", rgb: [255, 255, 255] };
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
        info.rgb = [d[0], d[1], d[2]];
      }
    }
  } catch (_) { /* white paper */ }
  pipeline.paperInfo = info;
  return info;
}

function pipelineIsIdentity(pipeline) {
  if (pipeline.filter !== "none") return false;
  if (pipeline.blend === "normal") return true;
  if (pipeline.blend === "multiply") {
    // multiply over pure white is identity — the default light theme skips
    // the bake entirely.
    const rgb = paperInfo(pipeline).rgb;
    return rgb[0] === 255 && rgb[1] === 255 && rgb[2] === 255;
  }
  return false;
}

/// Deterministic CSS-filter baking.
///
/// The theme's canvas filter chains (invert / hue-rotate / saturate /
/// brightness / sepia / contrast — the only functions
/// `Appearance::canvas_filter` ever generates) are all AFFINE COLOUR
/// MATRICES per the CSS Filter Effects spec. Two browser-dependent ways to
/// run them — first `ctx.filter`, then CSS `filter` + drawImage capture —
/// both failed silently in real engines: drawImage reads a canvas's BITMAP,
/// and a CSS filter only affects the element's composited output, not the
/// bitmap, so the capture drew the UNFILTERED raster. The filter never
/// applied, Dark rendered as a light page, and Dim "worked" only because its
/// soft-light blend alone darkens. So the chain is composed HERE into one
/// 3x3 matrix + offset and applied pixel-exactly with the same algebra the
/// spec defines — identical colours on every engine, GPU or software raster
/// alike. LUT-based fixed-point application keeps a page-sized bake in the
/// tens of milliseconds, and the blend pass below still runs through the
/// compositor's own `globalCompositeOperation` (the exact spec math).

/// Parse one filter function token into { m: 3x3 row-major, o: [r,g,b] }.
/// Returns null for anything unknown — the app only generates the six
/// functions above, and an unknown token degrades to identity rather than
/// guessing.
function filterTokenToMatrix(tok) {
  const m = /^([a-z-]+)\(([^)]*)\)$/.exec(String(tok).trim());
  if (!m) return null;
  const name = m[1];
  const arg = parseFloat(m[2]);
  if (!Number.isFinite(arg)) return null;
  switch (name) {
    case "invert": {
      // c' = p + (1 - 2p)·c
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
      // Luma weights per SVG/CSS: 0.213, 0.715, 0.072.
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
      // sepia(1) matrix from the spec, interpolated by t.
      const S = [0.393, 0.769, 0.189, 0.349, 0.686, 0.168, 0.272, 0.534, 0.131];
      const out = [];
      for (let i = 0; i < 9; i += 1) {
        const ident = i === 0 || i === 4 || i === 8 ? 1 : 0;
        out.push((1 - arg) * ident + arg * S[i]);
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

/// Compose a whole filter chain (space-separated function list, applied
/// left-to-right like CSS) into one { m, o } transform: out = m·in + o.
function composeFilter(filterString) {
  let m = [1, 0, 0, 0, 1, 0, 0, 0, 1];
  let o = [0, 0, 0];
  for (const tok of String(filterString).split(/\s+/)) {
    if (!tok) continue;
    const op = filterTokenToMatrix(tok);
    if (!op) continue;
    const nm = [];
    const no = [];
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

/// Apply the composed matrix to `src`'s pixels and return a fresh canvas.
/// Returns `src` unchanged when the chain composes to identity. Fixed-point
/// 16.16 LUTs (coefficient x value x 65536) turn each pixel into nine
/// lookups and a few adds; Uint8ClampedArray assignment rounds and clamps to
/// 0..255, exactly where CSS clamps the filter output.
function applyFilterPixels(src, filterString) {
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
  let img;
  try {
    img = sctx && sctx.getImageData(0, 0, w, h);
  } catch (_) {
    return src; // unreadable backing store: keep the raw raster
  }
  if (!img) return src;

  const SCALE = 1 << 16;
  const luts = new Array(9);
  for (let i = 0; i < 9; i += 1) {
    const coef = m[i];
    const lut = new Int32Array(256);
    for (let v = 0; v < 256; v += 1) lut[v] = Math.round(coef * v * SCALE);
    luts[i] = lut;
  }
  const o0 = Math.round(o[0] * 255 * SCALE);
  const o1 = Math.round(o[1] * 255 * SCALE);
  const o2 = Math.round(o[2] * 255 * SCALE);
  const L0 = luts[0], L1 = luts[1], L2 = luts[2];
  const L3 = luts[3], L4 = luts[4], L5 = luts[5];
  const L6 = luts[6], L7 = luts[7], L8 = luts[8];
  const d = img.data;
  for (let i = 0; i < d.length; i += 4) {
    const r = d[i];
    const g = d[i + 1];
    const b = d[i + 2];
    d[i] = (L0[r] + L1[g] + L2[b] + o0) >> 16;
    d[i + 1] = (L3[r] + L4[g] + L5[b] + o1) >> 16;
    d[i + 2] = (L6[r] + L7[g] + L8[b] + o2) >> 16;
    // Alpha (d[i+3]) is untouched: the rasters are opaque.
  }

  const out = document.createElement("canvas");
  out.width = w;
  out.height = h;
  const octx = out.getContext("2d", { alpha: false });
  if (!octx) return src;
  octx.putImageData(img, 0, 0);
  return out;
}

/// Bake `src` (the un-themed raster) through the theme pipeline into a fresh
/// opaque canvas of the same pixel size. Returns `src` itself when the
/// pipeline is identity (light, untinted — the common case: zero extra work
/// per render).
///
/// Order mirrors CSS exactly: the element's own `filter` runs first, then
/// `mix-blend-mode` blends the filtered result (source) over the backdrop
/// (destination = paper colour).
function bakeRaster(src, pipeline) {
  if (pipelineIsIdentity(pipeline)) return src;

  let filtered = src;
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
  octx.globalCompositeOperation = pipeline.blend;
  octx.drawImage(filtered, 0, 0);
  // The filtered intermediate lived only for this blend.
  if (filtered !== src) releaseCanvas(filtered);
  return out;
}

/// Paint `src` into `dst`'s backing store 1:1. The width/height assignment
/// clears the target and the very next statement paints, so the compositor
/// never shows the empty intermediate (same pattern as paintCached).
function blitInto(dst, src) {
  if (!dst || !src) return false;
  dst.width = src.width;
  dst.height = src.height;
  const ctx = dst.getContext("2d", { alpha: false });
  if (!ctx) return false;
  ctx.drawImage(src, 0, 0);
  return true;
}

/// Drop only the DISPLAY raster of a cached thumbnail (the raw raster stays
/// for re-bakes). Used when re-baking in place.
function releaseDisplayOnly(entry) {
  if (!entry) return;
  try {
    if (entry.display && typeof entry.display.close === "function") {
      entry.display.close();
    }
  } catch (_) { /* already closed */ }
  if (entry.display && entry.display !== entry.raw) releaseCanvas(entry.display);
  entry.display = null;
}

/// Re-bake every raster the engine already holds after an appearance change.
/// Called by the theme applier right after it writes the new CSS variables;
/// pages re-colour in place without a pdf.js re-render storm. The page swap
/// is synchronous (no wrong-coloured frame). The thumbnail CACHE is not
/// re-baked wholesale — the pipeline generation stamp marks every entry
/// stale, only the currently-mounted cells re-bake now (that is all the
/// reader can see), and anything else re-bakes lazily on its next use. That
/// keeps a theme switch a few blits instead of a full-cache bake storm.
async function refreshTheme() {
  // Called from a synchronous wasm-bindgen extern (the returned promise is
  // discarded), so a rejection here would be an unhandled promise error.
  // Internal failures degrade gracefully: stale entries re-bake lazily.
  try {
    await refreshThemeInternal();
  } catch (e) {
    console.warn("[pdfEngine] refreshTheme failed:", e && e.message ? e.message : e);
  }
}

async function refreshThemeInternal() {
  if (scrubbing) return; // scrub mode shows the raw raster + live CSS
  pipelineCache.token = null; // force a fresh read of the new variables
  const pipeline = readPipeline();

  for (const st of stateByCanvasId.values()) {
    if (st.dead || !st.canvas) continue;
    const raw = st.rawCanvas;
    if (!raw) continue;
    const baked = bakeRaster(raw, pipeline);
    if (baked !== st.canvas) {
      blitInto(st.canvas, baked);
      if (baked !== raw) releaseCanvas(baked); // transient bake buffer
    }
  }

  for (const [canvasId, { page }] of thumbLive) {
    const entry = thumbCache.get(page);
    const live = el(canvasId);
    if (!entry || !live) continue;
    if (!(await ensureEntryCurrent(entry))) continue;
    if (scrubbing) return; // a scrub started mid-bake; scrub mode owns the canvases
    paintCached(live, entry);
  }
}

/// Enter/leave appearance-scrub mode (slider drag). Entering shows the RAW
/// rasters under the live CSS filter/blend so the page re-colours per frame
/// like it always did; leaving re-bakes.
///
/// Frame correctness: entering swaps every canvas to raw AND adds the
/// `appearance-scrubbing` class in the same synchronous task, so the
/// compositor sees one consistent frame. Leaving keeps the class on while
/// the thumbnail cache re-bakes (awaits), then swaps the pages and drops the
/// class in one synchronous task — pages and thumbs never composite a
/// double-filtered or unfiltered frame.
async function setScrubMode(on) {
  // Same call shape as refreshTheme (sync wasm-bindgen extern, discarded
  // promise): never reject.
  try {
    await setScrubModeInternal(on);
  } catch (e) {
    console.warn("[pdfEngine] setScrubMode failed:", e && e.message ? e.message : e);
  }
}

async function setScrubModeInternal(on) {
  if (scrubbing === on) return;
  scrubbing = on;

  if (on) {
    for (const st of stateByCanvasId.values()) {
      if (st.dead || !st.canvas) continue;
      const raw = st.rawCanvas;
      if (raw && raw !== st.canvas) blitInto(st.canvas, raw);
    }
    for (const [canvasId, { page }] of thumbLive) {
      const entry = thumbCache.get(page);
      const live = el(canvasId);
      const raw = entry ? thumbRaw(entry) : null;
      if (live && raw) blitInto(live, raw);
    }
    document.documentElement.classList.add("appearance-scrubbing");
    return;
  }

  // The pipeline generation moves on (the scrub rewrote the variables), so
  // every cached entry is stale by stamp; only the mounted cells re-bake
  // now, the rest lazily on next use.
  const pipeline = readPipeline();
  // Phase 1 (awaits): re-bake the mounted cells' cache entries. Their live
  // canvases still hold RAW rasters and the scrub CSS class is still on, so
  // the screen stays consistent while this runs.
  for (const [canvasId, { page }] of thumbLive) {
    const entry = thumbCache.get(page);
    if (!entry) continue;
    await ensureEntryCurrent(entry);
    if (scrubbing) return; // a new scrub started mid-rebake; it owns the canvases
  }
  // Phase 2 (synchronous settle): bake the pages, repaint the cells and drop
  // the scrub CSS class in ONE task — no composited frame is ever
  // double-filtered (baked rasters under the live CSS) or unfiltered (raw
  // rasters with the class gone).
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
    const live = el(canvasId);
    if (entry && live) paintCached(live, entry);
  }
  document.documentElement.classList.remove("appearance-scrubbing");
}

/// Device-pixel scale for a page raster. Retina up to 2× while the page is
/// small; shrinks below 1× when the CSS box itself would blow the pixel budget
/// (a 5× zoom of a letter page is already 12MP before any DPR multiply).
function pageOutputScale(cssW, cssH) {
  const dpr = Math.min(globalThis.devicePixelRatio || 1, 2);
  if (!(cssW > 0) || !(cssH > 0)) return dpr;

  // Viewport-relative budget: the pixels a reader can actually see at once,
  // times CANVAS_AREA_FACTOR. Falls back to the absolute cap when the window
  // has no size yet (headless/startup).
  const vw = globalThis.innerWidth || 0;
  const vh = globalThis.innerHeight || 0;
  const viewportBudget = vw > 0 && vh > 0 ? vw * vh * CANVAS_AREA_FACTOR : Infinity;
  const budget = Math.min(PAGE_MAX_PIXELS, viewportBudget);

  const capped = Math.sqrt(budget / (cssW * cssH));
  return Math.min(dpr, Math.max(0.5, capped));
}

function thumbSource(entry) {
  if (!entry) return null;
  if (entry.display && entry.display.width > 0) return entry.display;
  return null;
}

/// The UN-THEMED raster of a thumbnail, used in scrub mode (raw + live CSS)
/// and as the source for theme re-bakes.
function thumbRaw(entry) {
  if (!entry) return null;
  if (entry.raw && entry.raw.width > 0) return entry.raw;
  return thumbSource(entry);
}

/// Make sure a cached thumbnail's display raster matches the CURRENT theme,
/// re-baking lazily when the pipeline generation moved on since it was
/// baked. Deduped with `entry.pending` so a remounting cell and a theme
/// repaint never bake the same entry twice. Returns the display raster.
async function ensureEntryCurrent(entry) {
  if (scrubbing || entry.gen === pipelineCache.gen) {
    return entry.display && entry.display.width > 0 ? entry.display : null;
  }
  if (entry.pending) return await entry.pending;
  entry.pending = (async () => {
    const raw = thumbRaw(entry);
    if (!raw) return null;
    // When the display IS the raw raster (identity themes), releasing it
    // would close the bitmap the re-bake is about to draw from. The bake
    // overwrites the display afterwards anyway.
    if (entry.display !== entry.raw) releaseDisplayOnly(entry);
    const pipeline = readPipeline();
    const baked = bakeRaster(raw, pipeline);
    entry.display = await cacheDisplay({ display: baked });
    if (baked === raw) entry.raw = entry.display; // bitmap became the raw
    entry.gen = pipelineCache.gen;
    return entry.display;
  })();
  const result = await entry.pending;
  entry.pending = null;
  return result;
}

/// Convert a baked raster canvas into the cache's preferred display form
/// (ImageBitmap when available, so eviction can free GPU memory
/// synchronously). Falls back to keeping the canvas itself.
async function cacheDisplay(entry) {
  const off = entry.display;
  if (!off || typeof createImageBitmap !== "function") return off;
  try {
    const bitmap = await createImageBitmap(off);
    releaseCanvas(off);
    return bitmap;
  } catch (_) {
    return off; // keep the canvas
  }
}

// canvasId -> { page } for thumbnails whose cell is currently mounted, so a
// theme re-bake can repaint the live cells without a remount.
const thumbLive = new Map();

// --- thumbnail cache helpers ----------------------------------------------
/// LRU insert: re-inserting an existing key moves it to the end (Map keeps
/// insertion order), so the first key is always the least recently used.
/// Evicted / replaced entries are RELEASED, not just dropped from the Map.
function cachePut(page, entry) {
  if (thumbCache.has(page)) {
    const prev = thumbCache.get(page);
    thumbCache.delete(page);
    if (prev && prev !== entry) releaseThumbEntry(prev);
  }
  thumbCache.set(page, entry);
  while (thumbCache.size > THUMB_CACHE_MAX) {
    const oldest = thumbCache.keys().next();
    if (oldest.done) break;
    const oldEntry = thumbCache.get(oldest.value);
    thumbCache.delete(oldest.value);
    if (oldEntry && oldEntry !== entry) releaseThumbEntry(oldEntry);
  }
}

/// Blit a cached bitmap into `dst` (a live <canvas>) 1:1. Returns the CSS size
/// of the source frame, or null when nothing was painted.
///
/// Only the BACKING STORE (width/height attributes) is touched — the canvas's
/// CSS box is owned by the cell's stylesheet classes. Assigning width/height
/// clears the target, but the very next statement paints the full cached frame
/// in the SAME task, so the browser never composites the empty intermediate.
function paintCached(dst, entry) {
  const src = thumbSource(entry);
  if (!dst || !src) return null;
  dst.width = src.width;
  dst.height = src.height;
  // Opaque for the same reason as the page raster: a rendered page has no
  // transparent pixels, so blending the thumbnail is wasted compositor work.
  const ctx = dst.getContext("2d", { alpha: false });
  if (!ctx) return null;
  ctx.drawImage(src, 0, 0);
  return { width: entry.cssW, height: entry.cssH };
}

/// SYNCHRONOUS cache probe: is `page` already rendered at `scale`?
///
/// The Rust cell calls this while BUILDING its view, before the first frame is
/// composited, so a cache-hit cell can mount with its loading cover already
/// transparent and un-animated. Without the probe every remounted row would
/// mount an opaque skeleton for one frame and then crossfade it away — the
/// per-row flicker when scrolling a virtualized grid back over pages that were
/// already rendered once.
///
/// A hit whose theme generation is stale is reported as a MISS: the entry
/// must re-bake before it can be blitted, and that bake awaits
/// `createImageBitmap`, so mounting the cell with `loaded` already true would
/// show a blank canvas for the bake's duration. A miss keeps the skeleton
/// cover up until the fresh bake paints — no blank, no wrong-coloured flash.
function hasThumb(page, scale) {
  const hit = thumbCache.get(page);
  return (
    !!hit &&
    Math.abs(hit.scale - scale) < 1e-9 &&
    (scrubbing || hit.gen === pipelineCache.gen)
  );
}

/// Paint the cached THUMBNAIL of `page` into a full-size page canvas as a
/// placeholder, stretched to fill it. Returns true if anything was painted.
///
/// A page scrolled freshly into view is a white card until its render resolves
/// — a bright pop against the reader. The sidebar has usually already
/// rasterised a thumbnail of that page, and an upscaled thumbnail is blurry but
/// the RIGHT COLOUR and the right shape, so the card reads as "this page,
/// loading" instead of a flash of white. The real render replaces it a moment
/// later.
///
/// Purely additive and best-effort: no cache entry, no canvas, no paint — the
/// caller carries on exactly as before. Scale is ignored deliberately; any
/// cached thumbnail of the page is better than nothing.
function blitThumb(canvasId, page) {
  const dst = el(canvasId);
  const entry = thumbCache.get(page);
  // Scrub mode shows raw rasters under the live CSS pipeline (the page
  // canvas is filter-blended again), so the placeholder must be raw too —
  // a baked thumbnail under the CSS filter would be double-filtered.
  const src = scrubbing ? thumbRaw(entry) : thumbSource(entry);
  if (!dst || !src) return false;
  // A stale entry (theme changed since it was baked) is skipped rather than
  // blitted: this call is synchronous (the Rust side calls it while building
  // a view), the fresh bake awaits createImageBitmap, and a placeholder in
  // the OLD theme's colours would flash worse than no placeholder at all.
  // The real page render lands a moment later.
  if (!scrubbing && entry.gen !== pipelineCache.gen) return false;
  // Keep the destination's own backing store if it already has one (the host
  // sizes it); otherwise adopt the thumb's aspect at a usable resolution.
  if (dst.width <= 0 || dst.height <= 0) {
    dst.width = src.width;
    dst.height = src.height;
  }
  const ctx = dst.getContext("2d");
  if (!ctx) return false;
  try {
    ctx.drawImage(src, 0, 0, dst.width, dst.height);
    return true;
  } catch (_) {
    return false;
  }
}

function itemRect(item, pageH) {
  // Approximate TOP-origin CSS-px bounding box of a text item at scale 1.
  // pdf.js text items use PDF user space (y-up, origin at page BOTTOM-left):
  // transform[4]/[5] = x / baseline y from the bottom. The TextLayer places a
  // span's TOP at `pageHeight - baselineY - ascent` (see its span-positioning
  // code), so we mirror that here. ascent ≈ 0.8 * fontSize for the default
  // fonts pdf.js substitutes; on-page highlights don't rely on this (they are
  // derived from the DOM spans), these rects only feed the results JSON.
  const t = item.transform || [1, 0, 0, 1, 0, 0];
  const fontSize = Math.hypot(t[2], t[3]);
  const ascent = (fontSize || 0) * 0.8;
  return {
    x: t[4],
    y: (pageH || 0) - t[5] - ascent,
    w: item.width || 0,
    h: item.height || 0,
  };
}

/// Reorder pdf.js text items so copy/paste and drag-selection follow visual

// --- bytes -----------------------------------------------------------
async function doFetch(src) {
  const res = await fetch(src);
  if (!res.ok) {
    throw Object.assign(new Error("HTTP " + res.status), { name: "UnexpectedResponseException" });
  }
  return new Uint8Array(await res.arrayBuffer());
}

async function fetchBytes(path) {
  // http(s) URLs are always fetched directly. Inside Tauri the bytes come
  // over IPC (read_file_bytes), which bypasses scheme/scope/CORS entirely;
  // anywhere else, plain web fetch.
  if (/^https?:\/\//i.test(path)) return doFetch(path);
  const tauri = globalThis.__TAURI__;
  if (tauri && tauri.core && typeof tauri.core.invoke === "function") {
    try {
      return new Uint8Array(await tauri.core.invoke("read_file_bytes", { path }));
    } catch (_) {
      // Not a readable filesystem path (bundled asset in dev); fall through.
    }
  }
  return doFetch(path);
}

/// Normalise one outline entry's title into something that can actually be
/// rendered as a row of text.
///
/// `it.title || "(untitled)"` only catches the empty string. Real PDFs also
/// carry titles that are whitespace-only ("   "), newline-only ("\r\n"), or
/// made of zero-width characters (U+200B, U+FEFF, soft hyphen) — often from
/// generators that emit a spacer bookmark. Those produce a row whose text has
/// no height, so the row collapsed from 28px to 8px: the "outline entries are
/// too low to be seen, barely visible as dots" report.
///
/// Also collapses interior newlines/tabs: a title that legitimately contains a
/// line break would otherwise wrap and make one row twice as tall as the rest.
function outlineTitle(raw) {
  return String(raw == null ? "" : raw).trim() || "(untitled)";
}

async function flattenOutline(items, depth, acc) {
  for (const it of items || []) {
    let page = null;
    try {
      if (Array.isArray(it.dest)) {
        const ref = it.dest[0];
        if (ref && typeof ref === "object" && "num" in ref) {
          const idx = await pdf.getPageIndex(ref);
          page = idx + 1;
        } else if (typeof ref === "number") {
          page = ref + 1;
        }
      } else if (typeof it.dest === "string") {
        const d = await pdf.getDestination(it.dest);
        if (d && d[0]) {
          const ref = d[0];
          if (ref && typeof ref === "object" && "num" in ref) {
            const idx = await pdf.getPageIndex(ref);
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
async function open(path) {
  try {
    await destroy();
    const bytes = await fetchBytes(path);
    loadingTask = getDocument({
      data: bytes,
      cMapUrl: "/vendor/pdfjs/cmaps/",
      cMapPacked: true,
      // We already hold the full byte buffer; don't let the worker prefetch
      // every page's stream on top of that.
      disableAutoFetch: true,
      disableStream: true,
    });
    pdf = await loadingTask.promise;
    numPages = pdf.numPages;
    currentPath = path;

    let title = null;
    let author = null;
    try {
      const meta = await pdf.getMetadata();
      title = (meta && meta.info && meta.info.Title) || null;
      author = (meta && meta.info && meta.info.Author) || null;
    } catch (_) { /* exotic docs */ }

    let outline = [];
    try {
      outline = await flattenOutline(await pdf.getOutline(), 0, []);
    } catch (_) { /* ignore */ }

    const page1 = await pdf.getPage(1);
    const vp = page1.getViewport({ scale: 1 });

    // Intrinsic (scale-1) height of EVERY page, in document order.
    //
    // The continuous viewer needs a height for every page up front so its
    // spacer and its scroll->page mapping are correct before a page has
    // rendered. Seeding all of them from page 1 makes the document silently
    // change height as off-screen pages are measured for the first time,
    // which slides the scroll anchor and lands the reader on the wrong page
    // after a zoom. getPage is cheap here: it
    // only parses the page dictionary, not its content stream.
    const pageHeights = new Array(numPages);
    const pageWidths = new Array(numPages);
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
        pageHeights[n - 1] = vp.height; // unreadable page: fall back to page 1
        pageWidths[n - 1] = vp.width;
      }
    }
    try { page1.cleanup(); } catch (_) {}

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
    if (e && e.name === "PasswordException") {
      return fail("encrypted", "This PDF is password-protected.");
    }
    if (
      e &&
      (e.name === "InvalidPDFException" ||
        e.name === "MissingPDFException" ||
        e.name === "UnexpectedResponseException")
    ) {
      const d = errorInfo(e);
      return fail("corrupt", `Could not read this PDF. (${d.name}: ${d.message})`);
    }
    const info = errorInfo(e);
    return fail(info.name, info.message);
  }
}

async function destroy() {
  for (const st of stateByCanvasId.values()) {
    st.dead = true;
    try { st.renderTask && st.renderTask.cancel(); } catch (_) {}
    try { st.textLayer && st.textLayer.cancel(); } catch (_) {}
    if (st.queueHandle) {
      cancelAnimationFrame(st.queueHandle);
      st.queueHandle = 0;
    }
    releasePageSurfaces(st);
  }
  stateByCanvasId.clear();
  // Thumbnail lane: cancel in-flight renders and drop every cached bitmap —
  // a new document must never blit the previous one's pages. close()/zero
  // each entry; Map.clear() alone leaves the IOSurfaces alive in WKWebView.
  for (const task of thumbTasks.values()) {
    try { task.cancel(); } catch (_) {}
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
    // Fire-and-forget the loading-task teardown. `loadingTask.destroy()`
    // terminates the worker, and on a document that has been scrolled (many
    // renders, text extractions, a search-index build in flight) it can take
    // arbitrarily long — long enough, in WKWebView, to never resolve and hang
    // the next open, which awaits this destroy. Drop the reference
    // SYNCHRONOUSLY so a re-entrant destroy is a no-op, and let the worker
    // die in the background; a fresh getDocument spawns its own worker.
    const lt = loadingTask;
    loadingTask = null;
    Promise.resolve(lt.destroy()).catch(() => {});
  }
  pdf = null;
  numPages = 0;
  currentPath = null;
}

function registerPage(payload) {
  const canvas = el(payload.canvasId);
  if (!canvas) return;
  const existing = stateByCanvasId.get(payload.canvasId);
  if (existing) {
    // Same id, new mount (or a stray re-register). Kill the old render but
    // do NOT zero the canvas — the element is being reused.
    existing.dead = true;
    try { existing.renderTask && existing.renderTask.cancel(); } catch (_) {}
    try { existing.textLayer && existing.textLayer.cancel(); } catch (_) {}
    if (existing.queueHandle) {
      cancelAnimationFrame(existing.queueHandle);
      existing.queueHandle = 0;
    }
  }
  const host = payload.hostId ? el(payload.hostId) : null;
  const textLayerEl = host ? host.querySelector(".textLayer") : null;
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

function unregisterPage(canvasId) {
  const st = stateByCanvasId.get(canvasId);
  if (st) {
    st.dead = true;
    try { st.renderTask && st.renderTask.cancel(); } catch (_) {}
    try { st.textLayer && st.textLayer.cancel(); } catch (_) {}
    if (st.queueHandle) {
      cancelAnimationFrame(st.queueHandle);
      st.queueHandle = 0;
    }
    // Zero the backing store BEFORE the element leaves the DOM. This is the
    // load-bearing WKWebView fix: without it, every page the reader has
    // scrolled past keeps its full-resolution IOSurface forever.
    releasePageSurfaces(st);
  }
  stateByCanvasId.delete(canvasId);
  sweepPdf();
}

function cancelPage(canvasId) {
  const st = stateByCanvasId.get(canvasId);
  if (st && st.renderTask) {
    try { st.renderTask.cancel(); } catch (_) {}
    st.renderTask = null;
  }
}

function applyHighlights(st) {
  // DOM-derived highlights: overlay a box over each rendered span whose text
  // contains the current query. Because rects come from getBoundingClientRect()
  // on the SAME spans the user sees, highlights always align with the glyphs —
  // regardless of scale rounding, font substitution, or the span's scaleX
  // kerning transform.
  //
  // Highlight divs go INSIDE the text layer so the `.textLayer .highlight` CSS
  // rules apply. That placement makes them direct children of the layer, which
  // the vendored pdf_viewer.css styles with a transform + font-size intended
  // for text spans; `styles/input.css` resets both on `.highlight`, so the
  // already-transformed rects measured here are not transformed a SECOND time.
  // Keep the two in sync: measuring in final screen px only lines up while the
  // boxes themselves are transform-free.
  const { host, textLayerEl } = st;
  // Remove the previous pass before measuring: a stale highlight is itself a
  // laid-out box, and leaving it would also let duplicates accumulate across
  // re-renders (doubled, slightly-offset highlight boxes).
  host.querySelectorAll(".highlight").forEach((n) => n.remove());
  if (!searchQuery || !textLayerEl) return;
  // A page mounted while the search is dismissed must paint its boxes stale
  // straight away, not flash them live first.
  textLayerEl.classList.toggle("search-stale", highlightMode === "stale");
  const origin = host.getBoundingClientRect();
  // Collect first, mutate after: appending into the layer while iterating a
  // live query would force a re-layout per insertion (and could re-measure
  // against shifted geometry).
  //
  // MEASURE THE MATCHED SUBSTRING, NOT THE SPAN. A pdf.js text-layer span is
  // whatever run of glyphs the PDF happened to emit as one item — frequently a
  // whole line. Highlighting `span.getBoundingClientRect()` therefore painted
  // the entire line for a two-letter query. A Range over just the matched
  // character offsets measures the glyphs themselves, and `getClientRects()`
  // returns one rect per line box, so a match that wraps still lands correctly.
  // ORDINALS MUST AGREE WITH search(). Each painted box carries the index of
  // the occurrence it belongs to, counted per page in reading order, and
  // search() counts occurrences the same way over the same item sequence. That
  // shared numbering is what lets the UI say "match 7 of this page is the
  // current one" and have this function stamp the right box — across
  // re-renders, remounts and zoom changes, with no rect matching.
  //
  // `ord` therefore advances for EVERY occurrence found, including ones that
  // cannot be measured (unaddressable span, zero-size rect). Skipping the
  // increment on an unpaintable match would slide every later box's number by
  // one and mark the wrong match as current.
  const boxes = [];
  const qlen = searchQuery.length;
  let ord = 0;
  for (const span of textLayerEl.querySelectorAll("span")) {
    const text = span.textContent;
    if (!text) continue;
    const hay = text.toLowerCase();
    if (!hay.includes(searchQuery)) continue;
    // The text node is what a Range can address; a span with no text child
    // (or a nested structure) is skipped rather than mismeasured.
    const node = span.firstChild;
    const addressable = node && node.nodeType === 3 && node.length >= qlen;
    // Every occurrence within the span, not just the first.
    for (let at = hay.indexOf(searchQuery); at !== -1; at = hay.indexOf(searchQuery, at + qlen)) {
      const mine = ord;
      ord += 1;
      if (!addressable) continue;
      let rects;
      try {
        const range = document.createRange();
        range.setStart(node, at);
        range.setEnd(node, at + qlen);
        rects = range.getClientRects();
        range.detach?.();
      } catch (_) {
        continue;
      }
      // A match that wraps across lines yields several rects; they all belong
      // to the same occurrence and so share its ordinal.
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

/// Repaint the highlight boxes of every mounted page for the current query.
///
/// Search must not go through the rasteriser. applyHighlights only reads the
/// text layer's client rects and appends absolutely-positioned divs, so this
/// costs a measure + a few appends per mounted page (~1 ms) against the ~80 ms
/// per page a re-render would cost. That is what makes highlight-as-you-type
/// affordable: the old code nudged `render_scale` to force a re-render purely
/// so highlights would be re-applied.
///
/// Pages that are not mounted yet get their highlights when they render, from
/// the stored `searchQuery`.
function refreshHighlights() {
  for (const st of stateByCanvasId.values()) {
    if (st.textLayerEl) applyHighlights(st);
  }
}

/// Switch the painted highlights between "live" and "stale" without touching
/// the query, the match list or the rasteriser.
///
/// One class toggle per mounted text layer; the colours live in CSS. This is
/// what lets dismissing the bar leave a visible trace of the search behind
/// instead of wiping it instantly — the reader can still see what they were
/// looking at, and reopening restores the query.
function setHighlightMode(mode) {
  highlightMode = mode === "stale" ? "stale" : "live";
  const stale = highlightMode === "stale";
  for (const st of stateByCanvasId.values()) {
    if (st.textLayerEl) st.textLayerEl.classList.toggle("search-stale", stale);
  }
}

/// Move the "current match" marker without re-rendering anything.
///
/// Stepping between matches only changes which box is emphasised, so it
/// retags the already-painted divs of the mounted pages instead of going
/// through a render (which costs ~80 ms/page and would make next/prev feel
/// heavy). Pages mounted later pick the marker up in applyHighlights.
function setActiveMatch(page, index) {
  activeMatch =
    Number.isFinite(page) && page > 0 && Number.isFinite(index) && index >= 0
      ? { page, index: index | 0 }
      : null;
  for (const st of stateByCanvasId.values()) {
    if (!st.textLayerEl) continue;
    const wanted = activeMatch && activeMatch.page === st.page ? String(activeMatch.index) : null;
    for (const d of st.textLayerEl.querySelectorAll(".highlight")) {
      d.classList.toggle("is-active", wanted !== null && d.dataset.match === wanted);
    }
  }
}

/// Resolve a pdf.js destination (named or explicit array) to a 1-based page
/// number, or null if it cannot be resolved.
async function destToPage(dest) {
  if (!pdf || !dest) return null;
  try {
    // Named destinations arrive as a string and need a lookup; explicit ones
    // are already the array form.
    const explicit = typeof dest === "string" ? await pdf.getDestination(dest) : dest;
    if (!Array.isArray(explicit) || !explicit.length) return null;
    const ref = explicit[0];
    // A page can be referenced either by object ref or by a direct index.
    if (typeof ref === "object" && ref !== null) {
      return (await pdf.getPageIndex(ref)) + 1;
    }
    if (Number.isInteger(ref)) return ref + 1;
    return null;
  } catch (_) {
    return null;
  }
}

/// Only allow schemes that are safe to hand to the OS/browser.
///
/// A PDF is untrusted input and its annotations can carry any URI at all,
/// including `javascript:` (script execution in our origin) and `file:`
/// (probing the local filesystem). Allow-list rather than block-list: a new
/// exotic scheme should be inert by default, not enabled by omission.
function safeExternalUrl(raw) {
  if (typeof raw !== "string" || !raw) return null;
  let u;
  try {
    u = new URL(raw, globalThis.location ? globalThis.location.href : undefined);
  } catch (_) {
    return null;
  }
  return ["http:", "https:", "mailto:"].includes(u.protocol) ? u.href : null;
}

/// Build the clickable link layer for a page.
///
/// WHY THIS EXISTS. The reader only ever built a canvas and a text layer, so a
/// URL in a PDF was rendered as pixels with selectable text on top and nothing
/// else — it looked like a link and did nothing. Link targets live in the
/// page's ANNOTATIONS, which is a separate stream from both the content and
/// the text, so no amount of scanning the text layer recovers them (and
/// regex-detecting URLs in the text would both miss real links whose anchor
/// text is not a URL, and invent links the document never declared).
///
/// Built detached and swapped in, for the same reason the text layer is: a
/// superseded render must never drop a half-built set of anchors on top of the
/// live ones.
async function buildLinkLayer(st, viewport, page) {
  const { host } = st;
  if (!host) return;

  let annots = [];
  try {
    // Reuse the page the caller already has — a second getPage would pin
    // another PDFPageProxy until the next cleanup sweep.
    const src = page || await pdf.getPage(st.page);
    annots = await src.getAnnotations({ intent: "display" });
    if (!page) {
      try { src.cleanup(); } catch (_) {}
    }
  } catch (_) {
    annots = [];
  }

  const layer = document.createElement("div");
  layer.className = "linkLayer";

  for (const a of annots) {
    if (!a || a.subtype !== "Link" || !Array.isArray(a.rect)) continue;

    const url = safeExternalUrl(a.url);
    const page = url ? null : await destToPage(a.dest);
    if (!url && !page) continue; // no usable target (e.g. a JS action)

    // Map PDF user space (y-up, origin bottom-left) to the rendered viewport.
    // convertToViewportPoint applies the viewport transform, so scale AND
    // rotation are handled for us — doing this arithmetic by hand is what
    // usually leaves link boxes offset on rotated pages. (The older
    // convertToViewportRectangle helper is not present in this pdf.js build,
    // so both corners are converted and normalised instead: after a rotation
    // the "bottom-left" corner may no longer be the min corner on screen.)
    const [x1, y1] = viewport.convertToViewportPoint(a.rect[0], a.rect[1]);
    const [x2, y2] = viewport.convertToViewportPoint(a.rect[2], a.rect[3]);
    const x = Math.min(x1, x2);
    const y = Math.min(y1, y2);
    const w = Math.abs(x2 - x1);
    const h = Math.abs(y2 - y1);
    if (!(w > 0) || !(h > 0)) continue;

    const el = document.createElement("a");
    el.className = "pdf-link";
    el.style.left = x + "px";
    el.style.top = y + "px";
    el.style.width = w + "px";
    el.style.height = h + "px";

    if (url) {
      el.href = url;
      el.target = "_blank";
      // noopener is a security requirement, not a nicety: without it the opened
      // page gets a handle on this window via window.opener.
      el.rel = "noopener noreferrer";
      el.title = url;
    } else {
      // Internal jump: no href, so it can never navigate the SPA away. The Rust
      // side owns page navigation, so tell it rather than scrolling from here —
      // that keeps one source of truth for the current page and reuses the
      // existing jump/settle logic.
      el.href = "#";
      el.title = "Go to page " + page;
      el.dataset.page = String(page);
      el.addEventListener("click", (ev) => {
        ev.preventDefault();
        globalThis.dispatchEvent(
          new CustomEvent("pdfreader:navigate", { detail: { page } })
        );
      });
    }
    layer.appendChild(el);
  }

  const live = host.querySelector(".linkLayer");
  if (live && live.parentNode) {
    live.replaceWith(layer);
  } else {
    host.appendChild(layer);
  }
}

async function renderPageInternal(canvasId, scale, renderText) {
  const st = stateByCanvasId.get(canvasId);
  if (!st) return fail("not_registered", "Page not registered: " + canvasId);
  if (!pdf) return fail("no_document", "No document open");

  try { st.renderTask && st.renderTask.cancel(); } catch (_) {}
  try { st.textLayer && st.textLayer.cancel(); } catch (_) {}
  st.renderTask = null;
  st.textLayer = null;

  const page = await pdf.getPage(st.page);
  if (st.dead || !st.canvas) {
    try { page.cleanup(); } catch (_) {}
    releasePageSurfaces(st);
    return fail("cancelled", "Render cancelled");
  }
  const viewport = page.getViewport({ scale });

  // HiDPI backing store. Only the BACKING STORE (width/height attributes) is
  // touched — the canvas's CSS box is owned by the stylesheet
  // (`.pdf-page canvas { inset: 0; width: 100%; height: 100% }`), exactly as it
  // already is for thumbnails in `paintCached`.
  //
  // ZOOM-STRETCH FIX: this used to also pin `style.width/height` to the render's
  // CSS px. An inline style beats the stylesheet, so the canvas was frozen at
  // the size of the LAST COMPLETED render while the host (and its ::before paper
  // texture, which is inset:0 and therefore does track) grew frame by frame
  // during a zoom. Measured on a single `+`: the host animated 1152 -> 1224px
  // while the canvas sat at 1152 the whole way, a 72px divergence that snapped
  // shut only on the final frame — the page bitmap visibly lagging its own
  // texture and border. Dropping the inline size lets the painted bitmap stretch
  // with the host every frame (which is the whole premise of zoom: animate
  // already-painted bitmaps, then land one crisp render), and the crisp render
  // below still resets the backing store to the new scale.
  //
  // PIXEL BUDGET: retina up to 2×, but never more than PAGE_MAX_PIXELS. An
  // uncapped 5× letter page on a 2× display is ~192MB of RGBA; the budget
  // keeps that at 32MB and the CSS box still stretches the bitmap.
  const cssW = Math.floor(viewport.width);
  const cssH = Math.floor(viewport.height);
  const out = pageOutputScale(cssW, cssH);
  const pxW = Math.max(1, Math.floor(viewport.width * out));
  const pxH = Math.max(1, Math.floor(viewport.height * out));

  // THEME BAKING. When the theme is identity (light, untinted) pdf.js paints
  // straight into the live canvas, exactly as before — zero extra work. With
  // any other theme the render goes into a DETACHED raw canvas and the theme
  // pipeline (filter + paper blend) is baked into the live canvas in one
  // pass, so the compositor only ever sees ONE opaque texture per page
  // instead of texture + filter intermediate + blend group. During an
  // appearance scrub the raw raster is shown and the CSS pipeline handles
  // the theme per frame (see `scrubbing`). A side benefit: the live canvas
  // keeps its previous bitmap until the new one is blitted, so re-renders
  // can no longer flash white at all.
  const pipeline = scrubbing ? null : readPipeline();
  const needsBake = !scrubbing && !pipelineIsIdentity(pipeline);
  const target = needsBake ? document.createElement("canvas") : st.canvas;
  target.width = pxW;
  target.height = pxH;
  // OPAQUE CONTEXT. pdf.js paints an opaque white page background before any
  // content, so the alpha channel is a constant 255 that nothing ever reads.
  // Declaring that lets the compositor treat the layer as opaque: it can skip
  // per-pixel blending against whatever is behind the page and can drop the
  // tiles underneath it entirely, which is where the compositor memory goes.
  const ctx = target.getContext("2d", { alpha: false });
  const transform = out !== 1 ? [out, 0, 0, out, 0, 0] : null;

  const task = page.render({ canvasContext: ctx, viewport, transform });
  st.renderTask = task;
  try {
    await task.promise;
  } catch (e) {
    try { page.cleanup(); } catch (_) {}
    if (target !== st.canvas) releaseCanvas(target);
    if (st.dead) releasePageSurfaces(st);
    if (e && e.name === "RenderingCancelledException") return fail("cancelled", "Render cancelled");
    const info = errorInfo(e);
    return fail(info.name, info.message);
  }
  if (st.dead) {
    try { page.cleanup(); } catch (_) {}
    if (target !== st.canvas) releaseCanvas(target);
    releasePageSurfaces(st);
    return fail("cancelled", "Render cancelled");
  }

  // Link the raw raster (the source for theme re-bakes and scrub mode) and
  // put the display bitmap on the live canvas. The intermediate bake buffer
  // is released immediately — it lives only for this blit.
  if (needsBake) {
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
    // Same viewport + same scale as the canvas render -> text aligns perfectly.
    st.host.style.setProperty("--scale-factor", String(scale));

    // --- GHOST-TEXT FIX: build into a DETACHED layer, then swap atomically ---
    // The old code reused the live `.textLayer` node: it cleared it and handed
    // the SAME element to every new TextLayer. `TextLayer.cancel()` only aborts
    // the stream reader — a chunk already read is still appended by the pump's
    // pending microtask, and it appends into that shared node. So a superseded
    // render (scale change, search re-render, fast scroll) could drop a partial
    // set of spans on top of the current one: two overlapping copies of the
    // same text. Invisible normally (spans are transparent), but selecting drew
    // the selection over BOTH copies — the "double / slightly offset text" on
    // selection — and the same duplicates doubled the search highlight boxes.
    //
    // Building into a detached div means a stale pump can only ever write into
    // a node that is never attached and is garbage-collected. The live layer is
    // replaced in ONE mutation, only after this render's text is complete, so
    // the host holds exactly one set of spans at all times.
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
      try { page.cleanup(); } catch (_) {}
      if (st.dead) releasePageSurfaces(st);
      if (e && e.name === "AbortException") return fail("cancelled", "Text render cancelled");
      const info = errorInfo(e);
      return fail(info.name, info.message);
    }
    if (st.dead) {
      try { page.cleanup(); } catch (_) {}
      releasePageSurfaces(st);
      return fail("cancelled", "Render cancelled");
    }

    // Swap in the finished layer. Re-read the current node from the host: an
    // earlier swap may have replaced the one captured at register time.
    const live = st.host.querySelector(".textLayer");
    if (live && live.parentNode) {
      live.replaceWith(layer);
    } else {
      st.host.appendChild(layer);
    }
    st.textLayerEl = layer;

    // Highlights are derived from the spans' live client rects, so they must be
    // applied only once the layer is attached and laid out.
    applyHighlights(st);

    // Links ride along with the text layer: both are per-page overlays that
    // only matter for the pages the reader can actually interact with, and
    // both must be rebuilt at the new geometry after a scale change.
    await buildLinkLayer(st, viewport, page);
  }

  st.viewport = viewport;
  st.scale = scale;
  page.cleanup();

  renderCount += 1;
  if (renderCount % CLEANUP_EVERY === 0) sweepPdf();

  return { ok: true, width: cssW, height: cssH, scale };
}

/// Render with one-frame burst coalescing.
///
/// A burst of navigation (thumbnails clicked in quick succession, a page
/// jump spree) used to start a pdf.js render per step — each one cancelling
/// the last but still queueing expensive worker rasters, text extractions
/// and compositor texture churn. That churn is what froze the webview's
/// GPU/compositor pipeline on Windows after a few rapid thumb jumps. The
/// render START is now deferred one animation frame, and a superseding
/// request (or an unmount) cancels the deferred start BEFORE any worker
/// work happens, so a burst collapses into a single render for the page the
/// reader actually lands on. The one-frame delay is invisible (renders take
/// ~80ms anyway).
async function renderPage(canvasId, scale, renderText) {
  const st = stateByCanvasId.get(canvasId);
  if (!st) return fail("not_registered", "Page not registered: " + canvasId);
  if (!pdf) return fail("no_document", "No document open");

  const gen = (st.queueGen || 0) + 1;
  st.queueGen = gen;
  if (st.queueHandle) {
    cancelAnimationFrame(st.queueHandle);
    st.queueHandle = 0;
  }
  return await new Promise((resolve) => {
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

// --- thumbnails ------------------------------------------------------------
/// Render ONE thumbnail into `canvasId` at `scale`, using (and filling) the
/// bitmap cache. No text layer, no highlights, no registration in
/// `stateByCanvasId` — thumbnails are a separate, cheap lane.
///
/// Returns `{ok, width, height, scale, cached}` where `cached:true` means the
/// bitmap was blitted SYNCHRONOUSLY from the cache before this function ever
/// awaited. That flag is the contract the Rust cell uses to skip its skeleton
/// entirely: a cached thumbnail is already painted on the first frame it is
/// mounted, so showing (and then crossfading out) a loading cover over it is
/// exactly the subtle flicker this lane removes.
async function renderThumb(canvasId, page, scale) {
  // The fast path performs real work now (lazy theme re-bakes) and the
  // engine contract is RESOLVE, never reject — a rejection unwinds the Rust
  // wasm future. Catch anything that escapes the internal error handling.
  try {
    return await renderThumbInternal(canvasId, page, scale);
  } catch (e) {
    const info = errorInfo(e);
    return fail(info.name, info.message);
  }
}

async function renderThumbInternal(canvasId, page, scale) {
  const canvas = el(canvasId);
  if (!canvas) return fail("no_canvas", "No canvas: " + canvasId);
  if (!pdf) return fail("no_document", "No document open");

  // --- fast path: cached bitmap --------------------------------------------
  const hit = thumbCache.get(page);
  if (hit && Math.abs(hit.scale - scale) < 1e-9) {
    if (scrubbing) {
      // Scrub mode blits the RAW raster (the cell canvas is filter-blended
      // by the live CSS pipeline again), synchronously, no bake needed.
      if (blitInto(canvas, thumbRaw(hit))) {
        cachePut(page, hit);
        thumbLive.set(canvasId, { page });
        return { ok: true, width: hit.cssW, height: hit.cssH, scale, cached: true };
      }
    } else if (hit.gen === pipelineCache.gen) {
      // Current theme: blit SYNCHRONOUSLY, before the first composite — the
      // contract `hasThumb`'s skeleton-skip depends on. No await may precede
      // this paint or a `loaded`-already-true cell would flash blank.
      const size = paintCached(canvas, hit);
      if (size) {
        cachePut(page, hit); // refresh LRU position
        thumbLive.set(canvasId, { page });
        return { ok: true, width: size.width, height: size.height, scale, cached: true };
      }
    } else if (await ensureEntryCurrent(hit)) {
      // Stale theme: re-baked above, now painted. Reported as NOT cached so
      // the cell's loading cover crossfades away — the cell mounted a
      // skeleton (hasThumb answered miss) and must reveal, not snap.
      const size = paintCached(canvas, hit);
      if (size) {
        cachePut(page, hit);
        thumbLive.set(canvasId, { page });
        return { ok: true, width: size.width, height: size.height, scale, cached: false };
      }
    }
  }

  // --- slow path: real render, into a DETACHED canvas ----------------------
  // Rendering off-DOM and blitting the finished frame in one statement means
  // the live canvas is never shown mid-render (pdf.js wipes the backing store
  // when it starts, which is what made a remounting row flash).
  try { thumbTasks.get(canvasId)?.cancel(); } catch (_) {}
  thumbTasks.delete(canvasId);
  thumbCancelled.delete(canvasId);

  try {
    const pg = await pdf.getPage(page);
    const viewport = pg.getViewport({ scale });
    // Thumbs are painted into a 120px CSS card. Scale 0.25 already gives
    // ~153 CSS px on a letter page — 1× device pixels is enough, and 2×
    // was 4× the memory for no visible gain.
    const out = 1;
    const cssW = Math.floor(viewport.width);
    const cssH = Math.floor(viewport.height);

    const off = document.createElement("canvas");
    off.width = Math.max(1, Math.floor(viewport.width * out));
    off.height = Math.max(1, Math.floor(viewport.height * out));
    // Opaque: a rendered page has no transparent pixels (see the page raster).
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
      try { pg.cleanup(); } catch (_) {}
      if (e && e.name === "RenderingCancelledException") {
        return fail("cancelled", "Thumb render cancelled");
      }
      const info = errorInfo(e);
      return fail(info.name, info.message);
    }
    thumbTasks.delete(canvasId);
    pg.cleanup();

    // THEME BAKING (see bakeRaster). `raw` is the un-themed raster — the
    // source for theme re-bakes and scrub mode; `display` is what the cache
    // holds and blits. Identity themes skip the bake entirely, and the
    // ImageBitmap conversion (close() frees GPU memory on eviction) applies
    // to whichever raster ends up being the display.
    const raw = off;
    let rawRaster = raw;
    let display = scrubbing ? raw : bakeRaster(raw, readPipeline());
    if (display === raw && typeof createImageBitmap === "function") {
      try {
        const bitmap = await createImageBitmap(raw);
        releaseCanvas(raw);
        display = bitmap;
        rawRaster = bitmap; // the bitmap IS the raw raster now
      } catch (_) { /* keep the canvas */ }
    } else if (display !== raw) {
      display = await cacheDisplay({ display });
    }
    // Stamped with the current pipeline generation. Entries baked during a
    // scrub hold -1 (never equal to any real generation): their display is
    // the RAW raster, and the theme may change every frame until the drag
    // ends, so they must always re-bake on first use after the scrub.
    const entry = {
      raw: rawRaster,
      display,
      cssW,
      cssH,
      scale,
      gen: scrubbing ? -1 : pipelineCache.gen,
    };
    cachePut(page, entry);

    // The cell may have unmounted while the render was in flight. Still
    // cache the frame (so a remount is instant) but do not paint — that
    // would re-allocate a backing store on an element about to die.
    if (!thumbCancelled.has(canvasId)) {
      const live = el(canvasId);
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

/// Render page 1 of `doc` to a small JPEG and return its data URL + CSS size.
function renderCoverFromPdf(doc, maxWidth) {
  return doc.getPage(1).then((page) => {
    const vp1 = page.getViewport({ scale: 1 });
    // Cover art is display-only; cap the scale so an oversized page can't
    // produce a huge JPEG just to sit on the shelf.
    const scale = Math.min((maxWidth || 240) / (vp1.width || 1), 2);
    const viewport = page.getViewport({ scale });

    const off = document.createElement("canvas");
    off.width = Math.max(1, Math.floor(viewport.width));
    off.height = Math.max(1, Math.floor(viewport.height));
    // Opaque: a rendered page has no transparent pixels (see the page raster).
    const ctx = off.getContext("2d", { alpha: false });
    if (!ctx) throw new Error("no_context");
    return page
      .render({ canvasContext: ctx, viewport })
      .promise.then(() => {
        try { page.cleanup(); } catch (_) {}
        // JPEG: the cover is display-only, and a lossy format keeps the
        // persisted data URL small enough to live comfortably in localStorage.
        const dataUrl = off.toDataURL("image/jpeg", 0.82);
        return { dataUrl, width: viewport.width, height: viewport.height };
      });
  });
}

/// Render page 1 of the book at `path` to a small JPEG, for the library shelf.
///
/// Uses the LIVE document when it is the one that was opened from `path`
/// (the common case — the cover is requested right after open), otherwise
/// opens its own throwaway document and tears it down. That second path keeps
/// a cover request from racing a fast A→B open: without it, a late cover task
/// for book A would render whatever document is open NOW and store B's page 1
/// under A's path.
async function coverDataUrl(path, maxWidth = 240) {
  try {
    if (!path) return fail("no_path", "No path");
    let result;
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
        try { await task.destroy(); } catch (_) {}
      }
    }
    return { ok: true, dataUrl: result.dataUrl, width: result.width, height: result.height };
  } catch (e) {
    const info = errorInfo(e);
    return fail(info.name, info.message);
  }
}

/// Cancel an in-flight thumbnail render (called when a cell unmounts). The
/// cache is deliberately NOT touched: a page that scrolls out and back must
/// still repaint instantly from its cached bitmap. The LIVE canvas is
/// zeroed so WKWebView drops its backing store with the cell.
function cancelThumb(canvasId) {  const task = thumbTasks.get(canvasId);
  if (task) {
    try { task.cancel(); } catch (_) {}
    thumbTasks.delete(canvasId);
  }
  thumbCancelled.add(canvasId);
  thumbLive.delete(canvasId);
  releaseCanvas(el(canvasId));
}

/// Engine-owned resource counts. Used by the verify harness to assert that
/// unmount / destroy actually drop canvases rather than just forgetting the
/// Map key. Additive; not part of the render contract.
function stats() {
  return {
    pages: stateByCanvasId.size,
    thumbs: thumbCache.size,
    thumbLimit: THUMB_CACHE_MAX,
    thumbTasks: thumbTasks.size,
  };
}

async function buildSearchIndex() {
  if (!pdf) return 0;
  textIndex.clear();
  let count = 0;
  for (let n = 1; n <= numPages; n += 1) {
    try {
      const page = await pdf.getPage(n);
      const tc = await page.getTextContent();
      const pageH = page.getViewport({ scale: 1 }).height;
      const items = [];
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

/// Every occurrence of `query`, in document order, as a FLAT list.
///
/// One entry per occurrence rather than one per page (and rather than one per
/// matching text item, which is what this used to return and which silently
/// dropped the 2nd..nth hit inside a single item). Navigation steps through
/// this list, so next/prev advances one match at a time — across matches
/// within a page as well as across pages.
///
/// `index` is the occurrence's ordinal WITHIN its page. applyHighlights counts
/// the same way over the same items, so the pair (page, index) identifies one
/// painted box on screen without any geometry matching.
///
/// Rects are approximate: a text item's box is divided across its characters,
/// which is enough to scroll a match into view. The on-page highlight boxes are
/// measured from the real DOM spans instead (see applyHighlights).
async function search(query) {
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
  const matches = [];
  const qlen = q.length;

  // Pages in ascending order: `textIndex` is filled 1..numPages so its
  // iteration order already is document order, but sorting makes that
  // independent of how the index was built.
  for (const page of [...textIndex.keys()].sort((a, b) => a - b)) {
    const items = textIndex.get(page) || [];
    const pageMatches = [];
    let ord = 0;
    for (const item of items) {
      const lower = item.str.toLowerCase();
      const len = lower.length || 1;
      for (let at = lower.indexOf(q); at !== -1; at = lower.indexOf(q, at + qlen)) {
        // Split the item's box across its characters. Monospaced-ish
        // approximation, only ever used to decide where to scroll.
        const rect = {
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

  // A new query invalidates the old cursor; the UI selects the first match.
  activeMatch = null;
  // Paint the pages the reader is already looking at, so results show up as
  // they type rather than on the next render.
  refreshHighlights();
  return { ok: true, query, total: matches.length, matches };
}

/// Context around the occurrence at `from` (defaults to the first one), so two
/// hits inside the same text item get different snippets.
function snippetText(str, q, from) {
  const idx = from === undefined ? str.toLowerCase().indexOf(q) : from;
  const start = Math.max(0, idx - 25);
  const end = Math.min(str.length, idx + q.length + 30);
  const pre = start > 0 ? "…" : "";
  const post = end < str.length ? "…" : "";
  return pre + str.slice(start, end) + post;
}

function clearHighlights() {
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

// --- OS-opened file handoff ------------------------------------------------
/// Collect the pending OS-opened PDF path from the Rust backend (double-click
/// / "Open with" / default-app launch). Consumes it, so a stray double
/// wake-up can never open the same file twice. Never rejects: the Rust side
/// cannot represent a JS exception in a wasm future, so every failure mode
/// resolves to null here.
async function takePendingFile() {
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

// --- localStorage wrappers (used for persisted settings) -------------
// --- expose ----------------------------------------------------------
/// Release every canvas backing store SYNCHRONOUSLY, for teardown.
///
/// WebKit bug 195325: canvas memory is not accounted against the page, so it
/// is NOT reclaimed when the document goes away — a reload starts with the
/// previous document's canvas budget still spent, and enough reloads make
/// `getContext("2d")` start returning null ("Total canvas memory use exceeds
/// the maximum limit"). WebKit's own position is that zeroing the dimensions
/// before the element is dropped is the correct workaround, so we do it on
/// the way out as well as during normal eviction.
///
/// Deliberately synchronous and allocation-free: `pagehide` may be the last
/// callback that runs, and anything awaited here is not guaranteed to
/// complete. `destroy()` still does the full teardown for the in-app path;
/// this is the backstop for a reload or a window close.
function releaseAllSurfaces() {
  for (const st of stateByCanvasId.values()) {
    st.dead = true;
    try { st.renderTask && st.renderTask.cancel(); } catch (_) {}
    try { st.textLayer && st.textLayer.cancel(); } catch (_) {}
    if (st.queueHandle) {
      cancelAnimationFrame(st.queueHandle);
      st.queueHandle = 0;
    }
    releasePageSurfaces(st);
  }
  for (const entry of thumbCache.values()) releaseThumbEntry(entry);
  // Catch anything not owned by the two registries above (e.g. a zoom
  // snapshot mid-swap): at this point nothing is going to be drawn again.
  try {
    document.querySelectorAll("canvas").forEach(releaseCanvas);
  } catch (_) { /* document already torn down */ }
}

// `pagehide` rather than `unload`: it also fires when the page enters the
// back/forward cache, and `unload` is unreliable (and deprecated) in WebKit.
globalThis.addEventListener("pagehide", releaseAllSurfaces);

// --- selection clamp: a drag only ever touches glyph spans -------------
// While the pointer crosses non-text surface (the gap under a heading, the
// inter-page gutter, margins) WebKit resolves the focus against the nearest
// selectable ancestor and the range re-anchors to a content extreme — the
// "selection spread" when dragging from a header into the body. Hold the last
// range whose both ends sat inside glyph spans and restore it whenever a
// degenerate range appears, so the selection only changes while the pointer is
// actually on text. The CSS scoping above already makes non-text surfaces
// non-selectable; this is the belt-and-braces clamp on the RANGE itself.
let selDragging = false;
let lastGoodRange = null;

document.addEventListener("mousedown", (e) => {
  selDragging = !!(e.target.closest && e.target.closest(".textLayer"));
});
window.addEventListener("mouseup", () => {
  selDragging = false;
  lastGoodRange = null;
});
document.addEventListener("selectionchange", () => {
  if (!selDragging) return;
  const sel = document.getSelection();
  if (!sel || sel.rangeCount === 0) return;
  const inSpan = (n) => {
    const el = n && (n.nodeType === Node.TEXT_NODE ? n.parentElement : n);
    return !!(el && el.closest &&
      el.closest(".textLayer > span, .textLayer .markedContent > span"));
  };
  if (inSpan(sel.anchorNode) && inSpan(sel.focusNode)) {
    lastGoodRange = sel.getRangeAt(0).cloneRange();
  } else if (lastGoodRange) {
    // Restoring re-fires selectionchange; the restored range is "good", so
    // the handler records it and terminates instead of looping.
    sel.removeAllRanges();
    sel.addRange(lastGoodRange);
  }
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
};
