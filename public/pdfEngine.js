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

let renderCount = 0;
// pdf.js keeps operator lists / decoded images per page. Once a page has
// left the window we want those gone; every-N is a backstop for pages that
// stay mounted across many re-renders (zoom).
const CLEANUP_EVERY = 5;
// Cap one page's RGBA backing store. At 5× zoom on a retina display an
// uncapped letter page is ~48MP / 192MB; 8MP is 32MB and still sharp.
const PAGE_MAX_PIXELS = 8 * 1024 * 1024;

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

function sweepPdf() {
  if (!pdf) return;
  try { pdf.cleanup(); } catch (_) {}
}

/// Device-pixel scale for a page raster. Retina up to 2× while the page is
/// small; shrinks below 1× when the CSS box itself would blow the pixel budget
/// (a 5× zoom of a letter page is already 12MP before any DPR multiply).
function pageOutputScale(cssW, cssH) {
  const dpr = Math.min(globalThis.devicePixelRatio || 1, 2);
  if (!(cssW > 0) || !(cssH > 0)) return dpr;
  const capped = Math.sqrt(PAGE_MAX_PIXELS / (cssW * cssH));
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
  const ctx = dst.getContext("2d");
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
    pageHeights[0] = vp.height;
    for (let n = 2; n <= numPages; n += 1) {
      try {
        const pg = await pdf.getPage(n);
        pageHeights[n - 1] = pg.getViewport({ scale: 1 }).height;
        pg.cleanup();
      } catch (_) {
        pageHeights[n - 1] = vp.height; // unreadable page: fall back to page 1
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
  if (loadingTask) {
    try { await loadingTask.destroy(); } catch (_) {}
    loadingTask = null;
  }
  pdf = null;
  numPages = 0;
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
  const origin = host.getBoundingClientRect();
  // Collect first, mutate after: appending into the layer while iterating a
  // live query would force a re-layout per insertion (and could re-measure
  // against shifted geometry).
  const boxes = [];
  for (const span of textLayerEl.querySelectorAll("span")) {
    if (!span.textContent || !span.textContent.toLowerCase().includes(searchQuery)) {
      continue;
    }
    const r = span.getBoundingClientRect();
    if (r.width <= 0 || r.height <= 0) continue;
    boxes.push(r);
  }
  for (const r of boxes) {
    const d = document.createElement("div");
    d.className = "highlight";
    d.style.left = r.x - origin.x + "px";
    d.style.top = r.y - origin.y + "px";
    d.style.width = Math.max(1, r.width) + "px";
    d.style.height = Math.max(1, r.height) + "px";
    textLayerEl.appendChild(d);
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
  const ctx = st.canvas.getContext("2d");
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

    const tl = new TextLayer({
      textContentSource: page.streamTextContent(),
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
    const ctx = off.getContext("2d");
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

/// Cancel an in-flight thumbnail render (called when a cell unmounts). The
/// cache is deliberately NOT touched: a page that scrolls out and back must
/// still repaint instantly from its cached bitmap. The LIVE canvas is
/// zeroed so WKWebView drops its backing store with the cell.
function cancelThumb(canvasId) {
  const task = thumbTasks.get(canvasId);
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
      for (const item of tc.items || []) {
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

async function search(query) {
  if (!pdf) return fail("no_document", "No document open");
  const q = String(query || "").toLowerCase().trim();
  if (!q) return { ok: true, query: "", total: 0, results: [] };

  searchQuery = q;
  highlightsByPage.clear();
  const results = [];
  let total = 0;

  for (const [page, items] of textIndex.entries()) {
    const pageMatches = [];
    for (const item of items) {
      const lower = item.str.toLowerCase();
      const idx = lower.indexOf(q);
      if (idx !== -1) {
        pageMatches.push({ x: item.x, y: item.y, w: item.w, h: item.h });
        total += 1;
      }
    }
    if (pageMatches.length) {
      highlightsByPage.set(page, pageMatches);
      const first = items.find((it) => it.str.toLowerCase().includes(q));
      const snippet = first ? snippetText(first.str, q) : "";
      results.push({ page, text: snippet, matches: pageMatches });
    }
  }

  return { ok: true, query, total, results };
}

function snippetText(str, q) {
  const idx = str.toLowerCase().indexOf(q);
  const start = Math.max(0, idx - 25);
  const end = Math.min(str.length, idx + q.length + 30);
  const pre = start > 0 ? "…" : "";
  const post = end < str.length ? "…" : "";
  return pre + str.slice(start, end) + post;
}

function clearHighlights() {
  highlightsByPage.clear();
  searchQuery = "";
  for (const st of stateByCanvasId.values()) {
    if (st.host) {
      st.host.querySelectorAll(".highlight").forEach((n) => n.remove());
    }
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
globalThis.PDFReader = {
  version: () => ENGINE_VERSION,
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
  stats,
  updatePage,
  buildSearchIndex,
  search,
  clearHighlights,
};
