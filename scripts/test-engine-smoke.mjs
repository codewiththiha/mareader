// Minimal harness to smoke-test pdfEngine.js outside a browser.
// Stubs: pdfjsLib, DOM (canvases), Tauri globals, rAF, getComputedStyle.

import { readFileSync } from "fs";
import vm from "vm";

const engineSrc = readFileSync(new URL("../public/pdfEngine.js", import.meta.url), "utf8");

// ---------- canvas stub (pixel-accurate) ----------
// A real backing store (Uint8ClampedArray) plus the subset of the canvas 2D
// API the engine uses: fillRect with colour parsing, drawImage with the
// compositing blend modes (source-over / multiply / screen / soft-light),
// getImageData / putImageData. This lets the harness assert ACTUAL PIXELS
// for the theme bake, which is the only way the Dark-mode regression is
// detectable (the two previous filter approaches failed silently).
class FakeCtx {
  constructor(canvas) {
    this.canvas = canvas;
    this.filter = "none";
    this.globalCompositeOperation = "source-over";
    this.fillStyle = "#000000";
    this._data = new Uint8ClampedArray(0);
  }
  _ensure() {
    const w = this.canvas.width || 0;
    const h = this.canvas.height || 0;
    const n = w * h * 4;
    if (this._data.length !== n) {
      const d = new Uint8ClampedArray(n);
      d.fill(255); // fresh backing stores start opaque white in this harness
      this._data = d;
    }
    return this._data;
  }
  parseColor(c) {
    let m = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(String(c).trim());
    if (m) return [parseInt(m[1], 16), parseInt(m[2], 16), parseInt(m[3], 16)];
    m = /^rgba?\(([^)]+)\)$/.exec(String(c).trim());
    if (m) {
      const p = m[1].split(",").map((x) => parseFloat(x));
      return [p[0], p[1], p[2]];
    }
    return [0, 0, 0];
  }
  // Source s over backdrop b (both 0..1), per the compositing spec.
  blend(s, b, op) {
    switch (op) {
      case "multiply": return s * b;
      case "screen": return s + b - s * b;
      case "soft-light": {
        const D = (x) => (x <= 0.25 ? ((16 * x - 12) * x + 4) * x : Math.sqrt(x));
        return s <= 0.5
          ? b - (1 - 2 * s) * b * (1 - b)
          : b + (2 * s - 1) * (D(b) - b);
      }
      default: return s; // source-over
    }
  }
  paint(x, y, r, g, b) {
    const w = this.canvas.width || 0;
    const h = this.canvas.height || 0;
    if (x < 0 || y < 0 || x >= w || y >= h) return;
    const d = this._ensure();
    const i = (y * w + x) * 4;
    const op = this.globalCompositeOperation;
    d[i] = Math.round(this.blend(r / 255, d[i] / 255, op) * 255);
    d[i + 1] = Math.round(this.blend(g / 255, d[i + 1] / 255, op) * 255);
    d[i + 2] = Math.round(this.blend(b / 255, d[i + 2] / 255, op) * 255);
    d[i + 3] = 255;
  }
  fillRect(x, y, w, h) {
    const [r, g, b] = this.parseColor(this.fillStyle);
    const fw = Math.min(Math.floor(w), (this.canvas.width || 0) - x);
    const fh = Math.min(Math.floor(h), (this.canvas.height || 0) - y);
    for (let yy = 0; yy < fh; yy += 1)
      for (let xx = 0; xx < fw; xx += 1) this.paint(x + xx, y + yy, r, g, b);
  }
  drawImage(src, x, y) {
    const sw = src && src.width;
    const sh = src && src.height;
    if (!sw || !sh) return;
    let sdata = null;
    if (src._ctx && src._ctx._data) sdata = src._ctx._ensure();
    else if (src.data) sdata = src.data;
    if (!sdata) return; // e.g. a stub ImageBitmap: nothing to copy
    for (let yy = 0; yy < sh; yy += 1)
      for (let xx = 0; xx < sw; xx += 1) {
        const si = (yy * sw + xx) * 4;
        this.paint(x + xx, y + yy, sdata[si], sdata[si + 1], sdata[si + 2]);
      }
  }
  getImageData(x, y, w, h) {
    const d = this._ensure();
    const out = new Uint8ClampedArray(w * h * 4);
    for (let yy = 0; yy < h; yy += 1) {
      const row = (y + yy) * (this.canvas.width || 0) + x;
      out.set(d.subarray(row * 4, (row + w) * 4), yy * w * 4);
    }
    return { data: out, width: w, height: h };
  }
  putImageData(img, x, y) {
    const d = this._ensure();
    for (let yy = 0; yy < img.height; yy += 1) {
      const row = (y + yy) * (this.canvas.width || 0) + x;
      d.set(img.data.subarray(yy * img.width * 4, (yy + 1) * img.width * 4), row * 4);
    }
  }
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
    backgroundColor: fakeComputed.paper || "#ffffff",
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
      // Real pdf.js paints an opaque white page before any content.
      canvasContext.fillStyle = "#ffffff";
      canvasContext.fillRect(0, 0, canvasContext.canvas.width, canvasContext.canvas.height);
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

// Independent re-implementation of the CSS Filter Effects math, used to
// compute the pixel the bake MUST produce. Deliberately separate from the
// engine's own code so the assertion is a real cross-check.
function expectedBakePixel(rgb, filter, blend, paper) {
  let [r, g, b] = rgb.map((v) => v / 255);
  for (const tok of filter.split(/\s+/)) {
    const m = /^([a-z-]+)\(([^)]*)\)$/.exec(tok);
    const a = parseFloat(m[2]);
    const [nr, ng, nb] = [r, g, b];
    switch (m[1]) {
      case "invert":
        r = a + (1 - 2 * a) * nr; g = a + (1 - 2 * a) * ng; b = a + (1 - 2 * a) * nb; break;
      case "brightness":
        r = a * nr; g = a * ng; b = a * nb; break;
      case "contrast":
        r = a * (nr - 0.5) + 0.5; g = a * (ng - 0.5) + 0.5; b = a * (nb - 0.5) + 0.5; break;
      case "saturate": {
        const t = 1 - a;
        const gr = 0.213 * nr + 0.715 * ng + 0.072 * nb;
        r = gr * t + nr * a; g = gr * t + ng * a; b = gr * t + nb * a; break;
      }
      case "sepia": {
        const t = a;
        r = (1 - t) * nr + t * (0.393 * nr + 0.769 * ng + 0.189 * nb);
        g = (1 - t) * ng + t * (0.349 * nr + 0.686 * ng + 0.168 * nb);
        b = (1 - t) * nb + t * (0.272 * nr + 0.534 * ng + 0.131 * nb);
        break;
      }
      case "hue-rotate": {
        const th = (a * Math.PI) / 180;
        const c = Math.cos(th), s = Math.sin(th);
        const M = [
          0.213 + 0.787 * c - 0.213 * s, 0.715 - 0.715 * c - 0.715 * s, 0.072 - 0.072 * c + 0.928 * s,
          0.213 - 0.213 * c + 0.143 * s, 0.715 + 0.285 * c + 0.140 * s, 0.072 - 0.072 * c - 0.283 * s,
          0.213 - 0.213 * c - 0.787 * s, 0.715 - 0.715 * c + 0.715 * s, 0.072 + 0.928 * c + 0.072 * s,
        ];
        r = M[0] * nr + M[1] * ng + M[2] * nb;
        g = M[3] * nr + M[4] * ng + M[5] * nb;
        b = M[6] * nr + M[7] * ng + M[8] * nb;
        break;
      }
    }
    [r, g, b] = [r, g, b].map((v) => Math.min(1, Math.max(0, v)));
  }
  const D = (x) => (x <= 0.25 ? ((16 * x - 12) * x + 4) * x : Math.sqrt(x));
  const out = [r, g, b].map((s, i) => {
    const bb = paper[i] / 255;
    let v;
    if (blend === "multiply") v = s * bb;
    else if (blend === "screen") v = s + bb - s * bb;
    else if (blend === "soft-light") v = s <= 0.5 ? bb - (1 - 2 * s) * bb * (1 - bb) : bb + (2 * s - 1) * (D(bb) - bb);
    else v = s;
    return Math.round(Math.min(1, Math.max(0, v)) * 255);
  });
  return out;
}

function assertClose(actual, expected, label, tol = 3) {
  for (let i = 0; i < 3; i += 1) {
    if (Math.abs(actual[i] - expected[i]) > tol) {
      throw new Error(
        label + " pixel mismatch: got [" + Array.from(actual).slice(0, 3) +
        "] expected [" + expected + "]"
      );
    }
  }
}

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
  // fast path). Small canvases (the 1x1 paper sampler) don't count. The
  // counting wrapper stays installed so later steps can assert on dark-bake
  // allocations.
  const created = [];
  const realCreate = fakeDocument.createElement;
  fakeDocument.createElement = (tag) => { const el = realCreate(tag); created.push(el); return el; };
  fakeComputed = { "--canvas-filter": "none", "--canvas-blend": "multiply" };
  PDFReader.registerPage({ canvasId: "cont-0-cv", hostId: "cont-0-pg", page: 1 });
  await PDFReader.renderPage("cont-0-cv", 1.5, true);
  const bakeCanvases = created.filter((el) => el.tagName === "CANVAS" && el.width > 10).length;
  if (bakeCanvases !== 0) throw new Error("identity pipeline allocated bake canvases: " + bakeCanvases);
  console.log("identity fast path ok (0 bake canvases)");

  // 3. DARK MODE REGRESSION TEST. The bake must actually invert the page:
  // the chain is exactly what Appearance::canvas_filter generates for Dark,
  // the paper is the dark base's --color-paper (#131316), and the live
  // canvas pixel must equal the spec math (screen over paper) — NOT white.
  const beforeDark = created.length;
  fakeComputed = {
    "--canvas-filter": "invert(0.92) hue-rotate(180deg) saturate(0.85) brightness(1.02)",
    "--canvas-blend": "screen",
    paper: "#131316",
  };
  await PDFReader.refreshTheme();
  const darkExpect = expectedBakePixel([255, 255, 255], fakeComputed["--canvas-filter"], "screen", [19, 19, 22]);
  const darkPx = elements.get("cont-0-cv")._ctx.getImageData(0, 0, 1, 1).data;
  assertClose(darkPx, darkExpect, "dark refreshTheme bake");
  console.log("refreshTheme (dark) ok: page pixel", Array.from(darkPx).slice(0, 3), "expected", darkExpect);

  // 4. render another page while dark -> the render-time bake path must
  // produce the same pixel.
  PDFReader.registerPage({ canvasId: "cont-1-cv", hostId: "cont-1-pg", page: 2 });
  const r2 = await PDFReader.renderPage("cont-1-cv", 1.5, true);
  if (!r2.ok) throw new Error("render2 failed: " + JSON.stringify(r2));
  const darkAllocs = created.length - beforeDark;
  if (darkAllocs < 4) throw new Error("dark bake should allocate filter+blend canvases, got " + darkAllocs);
  const darkPx2 = elements.get("cont-1-cv")._ctx.getImageData(0, 0, 1, 1).data;
  assertClose(darkPx2, darkExpect, "dark render bake");
  console.log("render ok (dark/baked):", r2.width, "x", r2.height, `(${darkAllocs} canvases)`);

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

  // 7. theme change marks cached thumbs STALE: unmounted entries re-bake
  // lazily. hasThumb goes miss (so the cell keeps its skeleton), the next
  // renderThumb lazily re-bakes (cached:false, cover crossfades), the one
  // after blits synchronously (true). Unmount the cell first: LIVE cells are
  // re-baked eagerly by refreshTheme itself.
  PDFReader.cancelThumb("thumb-1");
  fakeComputed = { "--canvas-filter": "brightness(0.8) saturate(0.75) contrast(0.9)", "--canvas-blend": "soft-light" };
  await PDFReader.refreshTheme();
  if (PDFReader.hasThumb(1, 0.25)) throw new Error("theme change must mark cached thumbs stale");
  const t3 = await PDFReader.renderThumb("thumb-1", 1, 0.25);
  if (!t3.ok || t3.cached !== false) throw new Error("stale thumb should re-bake (cached:false), got " + JSON.stringify(t3));
  const t4 = await PDFReader.renderThumb("thumb-1", 1, 0.25);
  if (!t4.ok || t4.cached !== true) throw new Error("re-baked thumb should now hit, got " + JSON.stringify(t4));
  console.log("lazy thumb re-bake ok");

  // 11. DIM check: the dim chain (brightness/saturate/contrast + soft-light
  // over #1a1c1f) must bake to the spec pixel on a freshly rendered page.
  fakeComputed = {
    "--canvas-filter": "brightness(0.8) saturate(0.75) contrast(0.9)",
    "--canvas-blend": "soft-light",
    paper: "#1a1c1f",
  };
  // A theme change always flows through refreshTheme in the app (the theme
  // applier calls it after writing the CSS variables) — it is what
  // invalidates the pipeline cache, including the paper colour.
  await PDFReader.refreshTheme();
  PDFReader.registerPage({ canvasId: "cont-2-cv", hostId: "cont-2-pg", page: 3 });
  const r3 = await PDFReader.renderPage("cont-2-cv", 1.5, true);
  if (!r3.ok) throw new Error("render3 failed: " + JSON.stringify(r3));
  const dimExpect = expectedBakePixel([255, 255, 255], fakeComputed["--canvas-filter"], "soft-light", [26, 28, 31]);
  const dimPx = elements.get("cont-2-cv")._ctx.getImageData(0, 0, 1, 1).data;
  assertClose(dimPx, dimExpect, "dim bake");
  console.log("dim bake ok: page pixel", Array.from(dimPx).slice(0, 3), "expected", dimExpect);

  // 12. A DARK PRESET WITH A TINT (the Night family) must bake too: the
  // sepia/saturate/hue-rotate tail composes with the dark base chain.
  fakeComputed = {
    "--canvas-filter": "invert(0.92) hue-rotate(180deg) saturate(0.85) brightness(1.02) sepia(0.193) saturate(1.21) hue-rotate(76deg)",
    "--canvas-blend": "screen",
    paper: "#131316",
  };
  await PDFReader.refreshTheme();
  PDFReader.registerPage({ canvasId: "cont-3-cv", hostId: "cont-3-pg", page: 4 });
  const r4 = await PDFReader.renderPage("cont-3-cv", 1.5, true);
  if (!r4.ok) throw new Error("render4 failed: " + JSON.stringify(r4));
  const nightExpect = expectedBakePixel([255, 255, 255], fakeComputed["--canvas-filter"], "screen", [19, 19, 22]);
  const nightPx = elements.get("cont-3-cv")._ctx.getImageData(0, 0, 1, 1).data;
  assertClose(nightPx, nightExpect, "dark+tint bake");
  console.log("dark+tint bake ok: page pixel", Array.from(nightPx).slice(0, 3), "expected", nightExpect);

  // 8. burst coalescing: fire 5 renders for the same canvas, only the last should matter
  const p1 = PDFReader.renderPage("cont-0-cv", 1.0, true);
  const p2 = PDFReader.renderPage("cont-0-cv", 1.2, true);
  const p3 = PDFReader.renderPage("cont-0-cv", 1.4, true);
  const [a, b, c] = await Promise.all([p1, p2, p3]);
  console.log("burst coalesce:", a.ok || a.error.name, b.ok || b.error.name, c.ok || c.error.name);

  // 9. unregister + destroy
  PDFReader.unregisterPage("cont-0-cv");
  PDFReader.unregisterPage("cont-1-cv");
  await PDFReader.destroy();
  const stats = PDFReader.stats();
  console.log("destroy ok, stats:", JSON.stringify(stats));
  if (stats.pages !== 0 || stats.thumbs !== 0) throw new Error("leak after destroy");

  // 10. OS file handoff wrapper: never rejects, null when nothing queued.
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
