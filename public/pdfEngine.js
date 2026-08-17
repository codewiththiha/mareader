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
// and the oldest are dropped past THUMB_CACHE_MAX. 64 is ~6 screens of the
// 2-column grid — enough that a remount inside normal browsing is still a
// sync blit — without pinning hundreds of bitmaps. Evicted entries are
// released immediately (ImageBitmap.close / canvas width=0); just deleting
// the Map key is not enough in WKWebView.
//
// Stored as ImageBitmap when the browser allows it so eviction can free GPU
// memory synchronously. Falls back to a detached canvas.
const thumbCache = new Map();
const THUMB_CACHE_MAX = 64;
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

/// Drop a cached thumbnail's GPU buffer. ImageBitmap.close() is synchronous;
/// a leftover canvas is zeroed the same way as a live page.
function releaseThumbEntry(entry) {
  if (!entry) return;
  try {
    if (entry.bitmap && typeof entry.bitmap.close === "function") {
      entry.bitmap.close();
    }
  } catch (_) { /* already closed */ }
  entry.bitmap = null;
  releaseCanvas(entry.canvas);
  entry.canvas = null;
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
  if (entry.bitmap && entry.bitmap.width > 0) return entry.bitmap;
  if (entry.canvas && entry.canvas.width > 0) return entry.canvas;
  return null;
}

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
function hasThumb(page, scale) {
  const hit = thumbCache.get(page);
  return !!hit && Math.abs(hit.scale - scale) < 1e-9;
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
  const src = thumbSource(thumbCache.get(page));
  if (!dst || !src) return false;
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
/// reading order, WITHOUT disturbing line / column structure.
///
/// pdf.js returns text items in CONTENT-STREAM (paint) order. Most producers
/// paint in reading order, but some emit each font run (an italic/bold phrase,
/// a list number, a caption label) as a separate show-text op AFTER the run it
/// visually precedes. The layer then holds that run at the wrong DOM position,
/// so:
///   - copy/paste concatenates spans in DOM order and comes out scrambled, and
///   - one contiguous drag selection paints as several visually disjoint
///     islands (the highlight "flickers" across style boundaries).
///
/// The fix is LOCAL: bubble adjacent items that sit on the same visual line
/// but are horizontally inverted. It only ever swaps two items that are
/// already neighbours in the stream AND vertically aligned, so it corrects
/// within-line inversions while leaving line order, column order (two-column
/// layouts keep their column-major stream order) and table row order exactly
/// as the producer laid them out. A global top-then-left sort would be WRONG
/// here: it merges the two columns of a two-column page into one "line" and
/// interleaves them, scrambling copy that is currently correct.
///
/// pdf.js already synthesises the inter-word spaces the PDF stream omits (as
/// separate `str: " "` items positioned in the gap), so this only PERMUTES
/// items — it never rewrites their text or transforms, which is what keeps the
/// span `--scale-x` math and the highlight getBoundingClientRect measurement
/// intact.
function normalizeTextOrder(items) {
  if (!items || items.length < 2) return items;
  const arr = items.slice(); // pure permutation of the input
  const top = (it) => -(it.transform?.[5] ?? 0);
  const left = (it) => (it.transform?.[4] ?? 0);
  const height = (it) => Math.max(it.height || 0, 1);

  let swapped = true;
  let guard = 0;
  while (swapped && guard < 1000) {
    swapped = false;
    guard += 1;
    for (let i = 0; i < arr.length - 1; i += 1) {
      const a = arr[i];
      const b = arr[i + 1];
      // Same line: baselines within half a line height of each other.
      const tol = 0.5 * Math.max(height(a), height(b));
      if (Math.abs(top(a) - top(b)) <= tol && left(b) < left(a)) {
        arr[i] = b;
        arr[i + 1] = a;
        swapped = true;
      }
    }
  }
  return arr;
}

// --- bytes -----------------------------------------------------------
async function doFetch(src) {
  const res = await fetch(src);
  if (!res.ok) {
    throw Object.assign(new Error("HTTP " + res.status), { name: "UnexpectedResponseException" });
  }
  return new Uint8Array(await res.arrayBuffer());
}

async function fetchBytes(path) {
  // http(s) URLs are always fetched directly.
  if (/^https?:\/\//i.test(path)) return doFetch(path);

  const tauri = globalThis.__TAURI__;
  if (tauri && tauri.core && typeof tauri.core.convertFileSrc === "function") {
    // Inside Tauri, prefer the asset protocol (real filesystem path picked from
    // the dialog). Fall back to a web fetch for bundled assets (/samples/, /vendor/)
    // which don't exist on the filesystem at that path. Note: absolute macOS paths
    // start with "/" just like web paths, so we cannot branch on path shape alone.
    try {
      return await doFetch(tauri.core.convertFileSrc(path));
    } catch (_) {
      // not a real file on disk -> try as a web path below
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
  const s = String(raw == null ? "" : raw)
    // Zero-width and BOM-ish characters render as nothing but are not
    // whitespace, so `trim()` keeps them and the row still looks blank.
    .replace(/[\u200b-\u200f\u2028\u2029\ufeff\u00ad]/g, "")
    // Any run of real whitespace (incl. newlines/tabs) becomes one space.
    .replace(/\s+/g, " ")
    .trim();
  return s.length > 0 ? s : "(untitled)";
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

function pageCount() {
  return pdf ? pdf.numPages : 0;
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
  });
}

function unregisterPage(canvasId) {
  const st = stateByCanvasId.get(canvasId);
  if (st) {
    st.dead = true;
    try { st.renderTask && st.renderTask.cancel(); } catch (_) {}
    try { st.textLayer && st.textLayer.cancel(); } catch (_) {}
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
  st.canvas.width = Math.max(1, Math.floor(viewport.width * out));
  st.canvas.height = Math.max(1, Math.floor(viewport.height * out));
  // OPAQUE CONTEXT. pdf.js paints an opaque white page background before any
  // content, so the alpha channel is a constant 255 that nothing ever reads.
  // Declaring that lets the compositor treat the layer as opaque: it can skip
  // per-pixel blending against whatever is behind the page and can drop the
  // tiles underneath it entirely, which is where the compositor memory goes.
  const ctx = st.canvas.getContext("2d", { alpha: false });
  const transform = out !== 1 ? [out, 0, 0, out, 0, 0] : null;

  const task = page.render({ canvasContext: ctx, viewport, transform });
  st.renderTask = task;
  try {
    await task.promise;
  } catch (e) {
    try { page.cleanup(); } catch (_) {}
    if (st.dead) releasePageSurfaces(st);
    if (e && e.name === "RenderingCancelledException") return fail("cancelled", "Render cancelled");
    const info = errorInfo(e);
    return fail(info.name, info.message);
  }
  if (st.dead) {
    try { page.cleanup(); } catch (_) {}
    releasePageSurfaces(st);
    return fail("cancelled", "Render cancelled");
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

    // READING-ORDER FIX: fetch the full text content and reorder it before
    // handing it to pdf.js's TextLayer. `streamTextContent()` streams in paint
    // order, which cannot be reordered; `getTextContent()` collects the same
    // items so normalizeTextOrder can permute them first (see above).
    const textContent = await page.getTextContent();
    textContent.items = normalizeTextOrder(textContent.items);

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

async function renderPage(canvasId, scale, renderText) {
  try {
    return await renderPageInternal(canvasId, scale, !!renderText);
  } catch (e) {
    const info = errorInfo(e);
    return fail(info.name, info.message);
  }
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
  const canvas = el(canvasId);
  if (!canvas) return fail("no_canvas", "No canvas: " + canvasId);
  if (!pdf) return fail("no_document", "No document open");

  // --- fast path: cached bitmap, painted before the first composite --------
  const hit = thumbCache.get(page);
  if (hit && Math.abs(hit.scale - scale) < 1e-9) {
    const size = paintCached(canvas, hit);
    if (size) {
      // Refresh LRU position.
      cachePut(page, hit);
      return { ok: true, width: size.width, height: size.height, scale, cached: true };
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

    // Prefer ImageBitmap: close() frees GPU memory on eviction. Fall back
    // to keeping the detached canvas when createImageBitmap is missing.
    let entry = { canvas: off, cssW, cssH, scale };
    if (typeof createImageBitmap === "function") {
      try {
        const bitmap = await createImageBitmap(off);
        releaseCanvas(off);
        entry = { bitmap, cssW, cssH, scale };
      } catch (_) { /* keep the canvas */ }
    }
    cachePut(page, entry);

    // The cell may have unmounted while the render was in flight. Still
    // cache the frame (so a remount is instant) but do not paint — that
    // would re-allocate a backing store on an element about to die.
    if (!thumbCancelled.has(canvasId)) {
      const live = el(canvasId);
      if (live) paintCached(live, entry);
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

async function renderPages(entries, scale) {
  const results = [];
  for (const entry of entries) {
    const renderText = entry.renderText !== undefined ? entry.renderText : !!entry.hostId;
    try {
      results.push(await renderPageInternal(entry.canvasId, scale, renderText));
    } catch (e) {
      const info = errorInfo(e);
      results.push(fail(info.name, info.message));
    }
  }
  return results;
}

async function updatePage(canvasId, scale) {
  const st = stateByCanvasId.get(canvasId);
  const renderText = !!(st && st.host && st.textLayerEl);
  return renderPage(canvasId, scale, renderText);
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
      // Same reading-order permutation the text layer uses, so snippet text,
      // match order and what the user copies are one source of truth.
      for (const item of normalizeTextOrder(tc.items) || []) {
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

// --- localStorage wrappers (used for persisted settings) -------------
function storageGet(key) {
  try {
    return window.localStorage.getItem(key);
  } catch (_) {
    return null;
  }
}

function storageSet(key, value) {
  try {
    window.localStorage.setItem(key, value);
  } catch (_) { /* ignore */ }
}

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
  storageGet,
  storageSet,
  open,
  destroy,
  pageCount,
  registerPage,
  unregisterPage,
  cancelPage,
  renderPage,
  renderPages,
  renderThumb,
  cancelThumb,
  hasThumb,
  blitThumb,
  coverDataUrl,
  stats,
  updatePage,
  buildSearchIndex,
  search,
  setActiveMatch,
  setHighlightMode,
  clearHighlights,
};
