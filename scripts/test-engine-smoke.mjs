// Minimal harness to smoke-test pdfEngine.js outside a browser.
// Stubs: pdfjsLib, DOM (canvases), Tauri globals, rAF, getComputedStyle.

import { readFileSync } from "fs";
import vm from "vm";

const engineSrc = readFileSync(new URL("../public/pdfEngine.js", import.meta.url), "utf8");

// ---------- canvas stub ----------
class FakeCtx {
  constructor(canvas) { this.canvas = canvas; this.filter = "none"; this.globalCompositeOperation = "source-over"; this.fillStyle = "#000"; }
  drawImage() {}
  fillRect() {}
  getImageData() { return { data: [255, 255, 255, 255] }; }
}
class FakeCanvas {
  constructor(tag = "canvas") {
    this.tagName = String(tag).toUpperCase();
    this.width = 300; this.height = 150;
    this._ctx = new FakeCtx(this);
    this.style = { setProperty() {}, removeProperty() {}, cssText: "" };
    this.className = "";
    this.classList = { add() {}, remove() {}, toggle() {}, contains() { return false; } };
    this.children = [];
    this.dataset = {};
    this.href = "";
    this.target = "";
    this.rel = "";
    this.title = "";
  }
  getContext() { return this._ctx; }
  toDataURL() { return "data:image/jpeg;base64,xx"; }
  setAttribute() {}
  getAttribute() { return null; }
  appendChild(c) { this.children.push(c); return c; }
  replaceChildren() {}
  remove() {}
  replaceWith() {}
  querySelectorAll() { return []; }
  querySelector() { return null; }
  addEventListener() {}
}
function makeElement(id) {
  return {
    id,
    width: 0, height: 0,
    style: { setProperty() {}, removeProperty() {} },
    className: "",
    classList: { add() {}, remove() {}, toggle() {}, contains() { return false; } },
    children: [],
    querySelectorAll() { return []; },
    querySelector() { return null; },
    appendChild(c) { this.children.push(c); return c; },
    replaceChildren() {},
    remove() {},
    replaceWith() {},
    setAttribute() { this.style = Object.assign({}, arguments); },
    getAttribute() { return null; },
    getBoundingClientRect() { return { x: 0, y: 0, width: 100, height: 100 }; },
    getContext(type, opts) { return this._ctx || (this._ctx = new FakeCtx(this)); },
    parentElement: null,
  };
}
const elements = new Map();
function getEl(id) {
  if (id === "documentElement") return docEl;
  if (!elements.has(id)) {
    const el = makeElement(id);
    if (id.startsWith("thumb-") || id.includes("-cv")) {
      // canvas-like: has width/height/getContext
      el._ctx = new FakeCtx(el);
    }
    elements.set(id, el);
  }
  return elements.get(id);
}
const docEl = {
  id: "documentElement",
  className: "",
  style: { getAttribute: () => null, setProperty() {} },
  classList: { add() {}, remove() {}, toggle() {}, contains() { return false; } },
  getAttribute() { return this._style || null; },
  setAttribute(k, v) { if (k === "style") this._style = v; },
  appendChild() {},
};
const fakeDocument = {
  documentElement: docEl,
  createElement: (tag) => new FakeCanvas(tag),
  getElementById: (id) => getEl(id),
  addEventListener() {},
  getSelection: () => null,
  createRange: () => ({ setStart() {}, setEnd() {}, getClientRects: () => [], detach() {} }),
  querySelectorAll: () => [],
};
const fakeWindow = {
  devicePixelRatio: 2,
  innerWidth: 1280,
  innerHeight: 800,
  localStorage: { getItem: () => null, setItem() {} },
  addEventListener() {},
  dispatchEvent() {},
  getComputedStyle: (el) => ({
    getPropertyValue: (name) => {
      if (name === "--canvas-filter") return fakeComputed["--canvas-filter"] || "none";
      if (name === "--canvas-blend") return fakeComputed["--canvas-blend"] || "normal";
      return "";
    },
    backgroundColor: "#ffffff",
  }),
  requestAnimationFrame: (fn) => { setTimeout(fn, 0); return 1; },
  cancelAnimationFrame() {},
  __TAURI__: {
    core: {
      invoke: async (cmd, args) => {
        // Plain array: cross-realm-safe stand-in for the real ArrayBuffer
        // response (instanceof fails across vm realms, the engine accepts
        // Array.isArray too).
        if (cmd === "read_file_bytes") return [1, 2, 3, 4];
        if (cmd === "take_pending_file") return null;
        throw new Error("unknown command " + cmd);
      },
      convertFileSrc: (p) => "asset://" + p,
    },
  },
};

let fakeComputed = { "--canvas-filter": "none", "--canvas-blend": "normal" };

// ---------- pdf.js stub ----------
function fakePage(n) {
  return {
    n,
    getViewport: ({ scale }) => ({ width: 600 * scale, height: 800 * scale, convertToViewportPoint: (x, y) => [x, y] }),
    render: ({ canvasContext }) => {
      return { promise: Promise.resolve(), cancel() {} };
    },
    getTextContent: async () => ({ items: [] }),
    getAnnotations: async () => [],
    cleanup: async () => {},
  };
}
const fakePdf = {
  numPages: 5,
  getPage: async (n) => fakePage(n),
  getMetadata: async () => ({ info: { Title: "Test", Author: null } }),
  getOutline: async () => [],
  getPageIndex: async () => 0,
  getDestination: async () => null,
  cleanup: async () => {},
};
const fakeLoadingTask = { promise: Promise.resolve(fakePdf), destroy: async () => {} };

const sandbox = {
  console,
  addEventListener() {},
  dispatchEvent() {},
  getComputedStyle: fakeWindow.getComputedStyle,
  globalThis: {},
  document: fakeDocument,
  window: fakeWindow,
  requestAnimationFrame: fakeWindow.requestAnimationFrame,
  cancelAnimationFrame: fakeWindow.cancelAnimationFrame,
  setTimeout,
  clearTimeout,
  Promise,
  Map,
  Set,
  Uint8Array,
  ArrayBuffer,
  Node: { TEXT_NODE: 3 },
  CustomEvent: class CustomEvent { constructor(t, d) { this.type = t; this.detail = d; } },
  URL,
  fetch: async () => { throw new Error("fetch disabled in harness"); },
  createImageBitmap: async (c) => ({ width: c.width, height: c.height, close() { this.closed = true; } }),
  pdfjsLib: {
    getDocument: () => fakeLoadingTask,
    GlobalWorkerOptions: {},
    TextLayer: class { constructor() {} async render() {} cancel() {} },
  },
};
sandbox.globalThis = sandbox;
sandbox.__TAURI__ = fakeWindow.__TAURI__; // engine reads globalThis.__TAURI__
vm.createContext(sandbox);
vm.runInContext(engineSrc, sandbox, { filename: "pdfEngine.js" });

const PDFReader = sandbox.PDFReader;
if (!PDFReader) throw new Error("PDFReader not defined after eval");

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

(async () => {
  // 1. open
  const opened = await PDFReader.open("/fake/book.pdf");
  if (!opened.ok) throw new Error("open failed: " + JSON.stringify(opened));
  console.log("open ok:", opened.numPages, "pages");

  // 2. register + render a page (identity pipeline first)
  PDFReader.registerPage({ canvasId: "cont-0-cv", hostId: "cont-0-pg", page: 1 });
  elements.get("cont-0-pg").querySelector = () => ({ classList: { toggle() {} } });
  const r1 = await PDFReader.renderPage("cont-0-cv", 1.5, true);
  if (!r1.ok) throw new Error("render failed: " + JSON.stringify(r1));
  console.log("render ok (identity):", r1.width, "x", r1.height);

  // 2b. light theme with multiply over PURE WHITE = identity pipeline: a
  // render must allocate ZERO page-sized bake canvases (the default-theme
  // fast path). Small canvases (the 1x1 paper sampler) don't count.
  const created = [];
  const realCreate = fakeDocument.createElement;
  fakeDocument.createElement = (tag) => { const el = realCreate(tag); created.push(el); return el; };
  fakeComputed = { "--canvas-filter": "none", "--canvas-blend": "multiply" };
  PDFReader.registerPage({ canvasId: "cont-0-cv", hostId: "cont-0-pg", page: 1 });
  await PDFReader.renderPage("cont-0-cv", 1.5, true);
  const bakeCanvases = created.filter((el) => el.tagName === "CANVAS" && el.width > 10).length;
  if (bakeCanvases !== 0) throw new Error("identity pipeline allocated bake canvases: " + bakeCanvases);
  fakeDocument.createElement = realCreate;
  console.log("identity fast path ok (0 bake canvases)");

  // 3. switch theme to dark (filter + screen) -> refreshTheme
  fakeComputed = { "--canvas-filter": "invert(0.92) hue-rotate(180deg)", "--canvas-blend": "screen" };
  await PDFReader.refreshTheme();
  console.log("refreshTheme (dark) ok");

  // 4. render another page while dark -> baked path
  PDFReader.registerPage({ canvasId: "cont-1-cv", hostId: "cont-1-pg", page: 2 });
  const r2 = await PDFReader.renderPage("cont-1-cv", 1.5, true);
  if (!r2.ok) throw new Error("render2 failed: " + JSON.stringify(r2));
  console.log("render ok (dark/baked):", r2.width, "x", r2.height);

  // 5. scrub mode on/off
  await PDFReader.setScrubMode(true);
  console.log("scrub on ok");
  await PDFReader.setScrubMode(false);
  console.log("scrub off ok");

  // 6. thumbnails
  const t = await PDFReader.renderThumb("thumb-1", 1, 0.25);
  if (!t.ok) throw new Error("thumb failed: " + JSON.stringify(t));
  console.log("thumb ok:", t.width, t.cached);
  const t2 = await PDFReader.renderThumb("thumb-1", 1, 0.25);
  if (!t2.ok || t2.cached !== true) throw new Error("thumb cache hit failed");
  console.log("thumb cache hit ok");

  // 7. burst coalescing: fire 5 renders for the same canvas, only the last should matter
  const p1 = PDFReader.renderPage("cont-0-cv", 1.0, true);
  const p2 = PDFReader.renderPage("cont-0-cv", 1.2, true);
  const p3 = PDFReader.renderPage("cont-0-cv", 1.4, true);
  const [a, b, c] = await Promise.all([p1, p2, p3]);
  console.log("burst coalesce:", a.ok || a.error.name, b.ok || b.error.name, c.ok || c.error.name);

  // 8. unregister + destroy
  PDFReader.unregisterPage("cont-0-cv");
  PDFReader.unregisterPage("cont-1-cv");
  await PDFReader.destroy();
  const stats = PDFReader.stats();
  console.log("destroy ok, stats:", JSON.stringify(stats));
  if (stats.pages !== 0 || stats.thumbs !== 0) throw new Error("leak after destroy");

  // 9. OS file handoff wrapper: never rejects, null when nothing queued.
  const none = await PDFReader.takePendingFile();
  if (none !== null) throw new Error("takePendingFile should resolve null, got " + none);
  let queuedPath = null;
  const realInvoke = fakeWindow.__TAURI__.core.invoke;
  fakeWindow.__TAURI__.core.invoke = async (cmd) =>
    cmd === "take_pending_file" ? queuedPath : realInvoke(cmd, {});
  queuedPath = "C:/Users/reader/Documents/book.pdf";
  const taken = await PDFReader.takePendingFile();
  if (taken !== queuedPath) throw new Error("takePendingFile did not return the path: " + taken);
  queuedPath = null;
  fakeWindow.__TAURI__.core.invoke = realInvoke;
  console.log("takePendingFile ok");

  console.log("ALL ENGINE TESTS PASSED");
})().catch((e) => {
  console.error("TEST FAILURE:", e);
  process.exit(1);
});
