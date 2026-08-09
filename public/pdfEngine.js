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
// page (1-based) -> [{ x, y, w, h, str }] rects in scale-1 CSS px (search index)
const textIndex = new Map();
// page (1-based) -> [{ x, y, w, h }] current search highlights in scale-1 CSS px
const highlightsByPage = new Map();

let renderCount = 0;
const CLEANUP_EVERY = 5;

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

function itemRect(item) {
  // Approximate CSS-px bounding box of a text item from its transform.
  const t = item.transform || [1, 0, 0, 1, 0, 0];
  return {
    x: t[4],
    y: t[5] - (item.height || 0),
    w: item.width || 0,
    h: item.height || 0,
  };
}

// --- bytes -----------------------------------------------------------
async function fetchBytes(path) {
  let src = path;
  if (!/^https?:\/\//i.test(path) && !path.startsWith("/")) {
    // Absolute filesystem path inside Tauri -> asset protocol URL.
    const tauri = globalThis.__TAURI__;
    if (tauri && tauri.core && typeof tauri.core.convertFileSrc === "function") {
      src = tauri.core.convertFileSrc(path);
    } else {
      throw Object.assign(new Error("No Tauri asset bridge available"), { name: "MissingPDFException" });
    }
  }
  const res = await fetch(src);
  if (!res.ok) {
    throw Object.assign(new Error("HTTP " + res.status), { name: "UnexpectedResponseException" });
  }
  return new Uint8Array(await res.arrayBuffer());
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
    if (page) acc.push({ title: it.title || "(untitled)", page, depth });
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

    return {
      ok: true,
      numPages,
      title,
      author,
      outline,
      page1Size: { width: vp.width, height: vp.height },
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
      return fail("corrupt", "Could not read this PDF.");
    }
    const info = errorInfo(e);
    return fail(info.name, info.message);
  }
}

async function destroy() {
  for (const st of stateByCanvasId.values()) {
    try { st.renderTask && st.renderTask.cancel(); } catch (_) {}
    try { st.textLayer && st.textLayer.cancel(); } catch (_) {}
  }
  stateByCanvasId.clear();
  textIndex.clear();
  highlightsByPage.clear();
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
  });
}

function unregisterPage(canvasId) {
  const st = stateByCanvasId.get(canvasId);
  if (st) {
    try { st.renderTask && st.renderTask.cancel(); } catch (_) {}
    try { st.textLayer && st.textLayer.cancel(); } catch (_) {}
  }
  stateByCanvasId.delete(canvasId);
}

function cancelPage(canvasId) {
  const st = stateByCanvasId.get(canvasId);
  if (st && st.renderTask) {
    try { st.renderTask.cancel(); } catch (_) {}
    st.renderTask = null;
  }
}

function applyHighlights(host, page, scale) {
  const rects = highlightsByPage.get(page);
  if (!rects) return;
  for (const r of rects) {
    const d = document.createElement("div");
    d.className = "highlight";
    d.style.left = r.x * scale + "px";
    d.style.top = r.y * scale + "px";
    d.style.width = Math.max(1, r.w * scale) + "px";
    d.style.height = Math.max(1, r.h * scale) + "px";
    host.appendChild(d);
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
  const viewport = page.getViewport({ scale });

  // HiDPI backing store; CSS size stays CSS px.
  const out = Math.min(globalThis.devicePixelRatio || 1, 2);
  const cssW = Math.floor(viewport.width);
  const cssH = Math.floor(viewport.height);
  st.canvas.width = Math.floor(viewport.width * out);
  st.canvas.height = Math.floor(viewport.height * out);
  st.canvas.style.width = cssW + "px";
  st.canvas.style.height = cssH + "px";
  const ctx = st.canvas.getContext("2d");
  const transform = out !== 1 ? [out, 0, 0, out, 0, 0] : null;

  const task = page.render({ canvasContext: ctx, viewport, transform });
  st.renderTask = task;
  try {
    await task.promise;
  } catch (e) {
    if (e && e.name === "RenderingCancelledException") return fail("cancelled", "Render cancelled");
    const info = errorInfo(e);
    return fail(info.name, info.message);
  }

  if (renderText && st.host && st.textLayerEl) {
    // Same viewport + same scale as the canvas render -> text aligns perfectly.
    st.host.style.setProperty("--scale-factor", String(scale));
    st.textLayerEl.textContent = "";
    const tl = new TextLayer({
      textContentSource: page.streamTextContent(),
      container: st.textLayerEl,
      viewport,
    });
    st.textLayer = tl;
    try {
      await tl.render();
    } catch (e) {
      if (e && e.name === "AbortException") return fail("cancelled", "Text render cancelled");
      const info = errorInfo(e);
      return fail(info.name, info.message);
    }
    applyHighlights(st.host, st.page, scale);
  }

  st.viewport = viewport;
  st.scale = scale;
  page.cleanup();

  renderCount += 1;
  if (renderCount % CLEANUP_EVERY === 0) {
    try { pdf.cleanup(); } catch (_) {}
  }

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
      const items = [];
      for (const item of tc.items || []) {
        if (!item.str) continue;
        const r = itemRect(item);
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
  updatePage,
  buildSearchIndex,
  search,
  clearHighlights,
};
