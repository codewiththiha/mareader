import { readFileSync } from "node:fs";
import vm from "node:vm";

// Minimal harness to smoke-test pdfEngine outside a browser.
// Stubs: pdfjsLib, DOM (canvases), Tauri globals, rAF, getComputedStyle.
//
// Reads the COMPILED `public/pdfEngine.js` (the same IIFE artifact the
// browser loads) and evaluates it in a vm sandbox. The bundle carries no
// module syntax (esbuild IIFE), so it is evaluated as-is — no source
// rewriting.


const engineSrc = readFileSync(
  new URL("../../public/pdfEngine.js", import.meta.url),
  "utf8"
);

// ---------- canvas stub (pixel-accurate) ----------
export class FakeCtx {
  canvas: FakeCanvas;
  filter = "none";
  globalCompositeOperation: string = "source-over";
  fillStyle: string | CanvasGradient | CanvasPattern = "#000000";
  _data: Uint8ClampedArray = new Uint8ClampedArray(0);

  constructor(canvas: FakeCanvas) {
    this.canvas = canvas;
  }

  _ensure(): Uint8ClampedArray {
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

  parseColor(c: string): [number, number, number] {
    let m = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i.exec(String(c).trim());
    if (m) {
      return [parseInt(m[1]!, 16), parseInt(m[2]!, 16), parseInt(m[3]!, 16)];
    }
    m = /^rgba?\(([^)]+)\)$/.exec(String(c).trim());
    if (m) {
      const p = m[1]!.split(",").map((x) => parseFloat(x));
      return [p[0] ?? 0, p[1] ?? 0, p[2] ?? 0];
    }
    return [0, 0, 0];
  }

  blend(s: number, b: number, op: string): number {
    switch (op) {
      case "multiply": return s * b;
      case "screen": return s + b - s * b;
      case "soft-light": {
        const D = (x: number): number =>
          x <= 0.25 ? ((16 * x - 12) * x + 4) * x : Math.sqrt(x);
        return s <= 0.5
          ? b - (1 - 2 * s) * b * (1 - b)
          : b + (2 * s - 1) * (D(b) - b);
      }
      default: return s; // source-over
    }
  }

  paint(x: number, y: number, r: number, g: number, b: number): void {
    const w = this.canvas.width || 0;
    const h = this.canvas.height || 0;
    if (x < 0 || y < 0 || x >= w || y >= h) return;
    const d = this._ensure();
    const i = (y * w + x) * 4;
    const op = this.globalCompositeOperation;
    d[i] = Math.round(this.blend(r / 255, d[i]! / 255, op) * 255);
    d[i + 1] = Math.round(this.blend(g / 255, d[i + 1]! / 255, op) * 255);
    d[i + 2] = Math.round(this.blend(b / 255, d[i + 2]! / 255, op) * 255);
    d[i + 3] = 255;
  }

  fillRect(x: number, y: number, w: number, h: number): void {
    const [r, g, b] = this.parseColor(String(this.fillStyle));
    const fw = Math.min(Math.floor(w), (this.canvas.width || 0) - x);
    const fh = Math.min(Math.floor(h), (this.canvas.height || 0) - y);
    for (let yy = 0; yy < fh; yy += 1) {
      for (let xx = 0; xx < fw; xx += 1) {
        this.paint(x + xx, y + yy, r, g, b);
      }
    }
  }

  drawImage(src: CanvasLike, x: number, y: number): void {
    const sw = (src as FakeCanvas).width ?? (src as { width?: number }).width;
    const sh = (src as FakeCanvas).height ?? (src as { height?: number }).height;
    if (!sw || !sh) return;
    let sdata: Uint8ClampedArray | null = null;
    const fc = src as FakeCanvas & { _ctx?: FakeCtx; data?: Uint8ClampedArray };
    if (fc._ctx && fc._ctx._data) sdata = fc._ctx._ensure();
    else if (fc.data) sdata = fc.data;
    if (!sdata) return; // e.g. a stub ImageBitmap: nothing to copy
    for (let yy = 0; yy < sh; yy += 1) {
      for (let xx = 0; xx < sw; xx += 1) {
        const si = (yy * sw + xx) * 4;
        this.paint(x + xx, y + yy, sdata[si]!, sdata[si + 1]!, sdata[si + 2]!);
      }
    }
  }

  getImageData(x: number, y: number, w: number, h: number): ImageData {
    const d = this._ensure();
    const out = new Uint8ClampedArray(w * h * 4);
    for (let yy = 0; yy < h; yy += 1) {
      const row = (y + yy) * (this.canvas.width || 0) + x;
      out.set(d.subarray(row * 4, (row + w) * 4), yy * w * 4);
    }
    return { data: out, width: w, height: h, colorSpace: "srgb" } as unknown as ImageData;
  }

  putImageData(img: ImageData, x: number, y: number): void {
    const d = this._ensure();
    for (let yy = 0; yy < img.height; yy += 1) {
      const row = (y + yy) * (this.canvas.width || 0) + x;
      d.set(img.data.subarray(yy * img.width * 4, (yy + 1) * img.width * 4), row * 4);
    }
  }
}

interface FakeStyle {
  setProperty: () => void;
  removeProperty: () => void;
  cssText?: string;
  getAttribute?: () => string | null;
}

type CanvasLike = FakeCanvas & { width: number; height: number; data?: Uint8ClampedArray };

export class FakeCanvas {
  tagName: string;
  width = 300;
  height = 150;
  _ctx: FakeCtx;
  style: FakeStyle = { setProperty() {}, removeProperty() {} };
  className = "";
  classList = { add() {}, remove() {}, toggle() {}, contains() { return false; } };
  children: unknown[] = [];
  dataset: Record<string, string> = {};
  href = "";
  target = "";
  rel = "";
  title = "";
  parentElement: unknown = null;
  _style: string | null = null;

  constructor(tag = "canvas") {
    this.tagName = String(tag).toUpperCase();
    this._ctx = new FakeCtx(this);
  }

  getContext(): FakeCtx { return this._ctx; }
  toDataURL(): string { return "data:image/jpeg;base64,xx"; }
  setAttribute(): void {}
  getAttribute(): string | null { return null; }
  appendChild<T>(c: T): T { this.children.push(c); return c; }
  replaceChildren(): void {}
  remove(): void {}
  replaceWith(): void {}
  querySelectorAll(): never[] { return []; }
  querySelector(): null { return null; }
  addEventListener(): void {}
  getBoundingClientRect(): { x: number; y: number; width: number; height: number } {
    return { x: 0, y: 0, width: 100, height: 100 };
  }
}

function makeElement(id: string): FakeCanvas & { id: string; width: number; height: number } {
  const el: FakeCanvas & { id: string; width: number; height: number } = {
    id,
    width: 0,
    height: 0,
    style: { setProperty() {}, removeProperty() {} },
    className: "",
    classList: { add() {}, remove() {}, toggle() {}, contains() { return false; } },
    children: [],
    querySelectorAll() { return []; },
    querySelector() { return null; },
    appendChild<T>(c: T): T { (el.children as unknown[]).push(c); return c; },
    replaceChildren() {},
    remove() {},
    replaceWith() {},
    setAttribute() {},
    getAttribute() { return null; },
    getBoundingClientRect() { return { x: 0, y: 0, width: 100, height: 100 }; },
    getContext(): FakeCtx {
      if (!el._ctx) el._ctx = new FakeCtx(el);
      return el._ctx;
    },
    parentElement: null,
  } as unknown as FakeCanvas & { id: string; width: number; height: number };
  return el;
}

const elements = new Map<string, ReturnType<typeof makeElement>>();
export function getEl(id: string): ReturnType<typeof makeElement> {
  if (id === "documentElement") return docEl as ReturnType<typeof makeElement>;
  if (!elements.has(id)) {
    const el = makeElement(id);
    if (id.startsWith("thumb-") || id.includes("-cv")) {
      // canvas-like: has width/height/getContext
      (el as unknown as { _ctx: FakeCtx })._ctx = new FakeCtx(el as unknown as FakeCanvas);
    }
    elements.set(id, el);
  }
  return elements.get(id)!;
}

const docEl: FakeCanvas & { id: string; width: number; height: number } = (() => {
  let _style: string | null = null;
  const el: FakeCanvas & { id: string; width: number; height: number } = {
    id: "documentElement",
    width: 0,
    height: 0,
    className: "",
    style: { getAttribute: () => _style, setProperty() {} },
    classList: { add() {}, remove() {}, toggle() {}, contains() { return false; } },
    getAttribute() { return _style; },
    setAttribute(k: string, v: string) { if (k === "style") _style = v; },
    appendChild() {},
    getBoundingClientRect() { return { x: 0, y: 0, width: 0, height: 0 }; },
    children: [],
    querySelectorAll() { return []; },
    querySelector() { return null; },
    replaceChildren() {},
    remove() {},
    replaceWith() {},
  } as unknown as FakeCanvas & { id: string; width: number; height: number };
  return el;
})();

export const fakeDocument = {
  documentElement: docEl,
  createElement: (tag: string) => new FakeCanvas(tag) as unknown as ReturnType<typeof makeElement>,
  getElementById: (id: string) => getEl(id),
  addEventListener() {},
  getSelection: () => null,
  createRange: () => ({ setStart() {}, setEnd() {}, getClientRects: () => [], detach() {} }),
  querySelectorAll: (sel?: string) => {
    if (sel === "canvas.thumb-canvas" || sel === "canvas") {
      return [...elements.values()].filter((e) => String(e.id).startsWith("thumb-") || String(e.id).includes("-cv"));
    }
    return [];
  },
};

interface FakeWindow {
  devicePixelRatio: number;
  innerWidth: number;
  innerHeight: number;
  localStorage: { getItem(): string | null; setItem(): void };
  addEventListener(): void;
  dispatchEvent(): boolean;
  getComputedStyle: (el: { _style?: string }) => {
    getPropertyValue: (name: string) => string;
    backgroundColor: string;
  };
  requestAnimationFrame: (fn: () => void) => number;
  cancelAnimationFrame(): void;
  __TAURI__?: {
    core: {
      invoke: (cmd: string) => Promise<unknown>;
      convertFileSrc: (p: string) => string;
    };
  };
}

export let fakeComputed: { "--canvas-filter": string; "--canvas-blend": string; paper?: string } = {
  "--canvas-filter": "none",
  "--canvas-blend": "normal",
};

export const fakeWindow: FakeWindow = {
  devicePixelRatio: 2,
  innerWidth: 1280,
  innerHeight: 800,
  localStorage: { getItem: () => null, setItem() {} },
  addEventListener() {},
  dispatchEvent() { return true; },
  getComputedStyle: (_el) => ({
    getPropertyValue: (name: string) => {
      if (name === "--canvas-filter") return fakeComputed["--canvas-filter"] || "none";
      if (name === "--canvas-blend") return fakeComputed["--canvas-blend"] || "normal";
      return "";
    },
    backgroundColor: fakeComputed.paper || "#ffffff",
  }),
  requestAnimationFrame: (fn: () => void) => { setTimeout(fn, 0); return 1; },
  cancelAnimationFrame() {},
  __TAURI__: {
    core: {
      invoke: async (cmd: string): Promise<unknown> => {
        if (cmd === "read_file_bytes") return [1, 2, 3, 4];
        if (cmd === "take_pending_file") return null;
        throw new Error("unknown command " + cmd);
      },
      convertFileSrc: (p: string) => "asset://" + p,
    },
  },
};

// ---------- pdf.js stub ----------
function fakePage(n: number) {
  return {
    n,
    getViewport: ({ scale }: { scale: number }) => ({
      width: 600 * scale,
      height: 800 * scale,
      convertToViewportPoint: (x: number, y: number): [number, number] => [x, y],
    }),
    render: ({ canvasContext }: { canvasContext: FakeCtx }) => {
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
  getPage: async (n: number) => fakePage(n),
  getMetadata: async () => ({ info: { Title: "Test", Author: null } }),
  getOutline: async () => [],
  getPageIndex: async () => 0,
  getDestination: async () => null,
  cleanup: async () => {},
};
const fakeLoadingTask = { promise: Promise.resolve(fakePdf), destroy: async () => {} };

const sandbox: Record<string, unknown> = {
  console,
  addEventListener() {},
  dispatchEvent() { return true; },
  getComputedStyle: fakeWindow.getComputedStyle,
  globalThis: {} as Record<string, unknown>,
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
  CustomEvent: class CustomEvent<T> {
    type: string;
    detail: T;
    constructor(t: string, d: T) { this.type = t; this.detail = d; }
  },
  URL,
  fetch: async () => { throw new Error("fetch disabled in harness"); },
  createImageBitmap: async (c: FakeCanvas) => {
    const ctx = (c as FakeCanvas & { _ctx?: FakeCtx })._ctx;
    const data = ctx ? ctx._ensure().slice() : new Uint8ClampedArray(0);
    return {
      width: c.width,
      height: c.height,
      data,
      close() { /* stub */ },
    };
  },
  pdfjsLib: {
    getDocument: () => fakeLoadingTask,
    GlobalWorkerOptions: {},
    TextLayer: class {
      constructor() {}
      async render() {}
      cancel() {}
    },
  },
};
sandbox.globalThis = sandbox;
(sandbox as { __TAURI__: typeof fakeWindow.__TAURI__ }).__TAURI__ = fakeWindow.__TAURI__;
vm.createContext(sandbox);
vm.runInContext(engineSrc, sandbox, { filename: "pdfEngine.js" });
interface EngineError { ok: false; error: { name: string; message: string } }
type EngineOk<T> = T & { ok: true };
type EngineResult<T> = EngineOk<T> | EngineError;

interface OpenPayload {
  numPages: number;
  title: string | null;
  author: string | null;
  outline: unknown[];
  page1Size: { width: number; height: number };
  pageHeights: number[];
  pageWidths: number[];
}
interface RenderPayload { width: number; height: number; scale: number }
interface ThumbPayload { width: number; height: number; scale: number; cached: boolean }
interface StatsPayload { pages: number; thumbs: number; thumbLimit: number; thumbTasks: number }

interface PDFReaderHandle {
  open(path: string): Promise<EngineResult<OpenPayload>>;
  registerPage(p: { canvasId: string; hostId: string; page: number }): void;
  renderPage(canvasId: string, scale: number, renderText: boolean): Promise<EngineResult<RenderPayload>>;
  renderThumb(canvasId: string, page: number, scale: number): Promise<EngineResult<ThumbPayload>>;
  cancelThumb(canvasId: string): void;
  hasThumb(page: number, scale: number): boolean;
  refreshTheme(): Promise<void>;
  setScrubMode(on: boolean): Promise<void>;
  unregisterPage(canvasId: string): void;
  destroy(): Promise<void>;
  stats(): StatsPayload;
  takePendingFile(): Promise<string | null>;
}

export const PDFReader = sandbox.PDFReader as PDFReaderHandle;
if (!PDFReader) throw new Error("PDFReader not defined after eval");

// Independent re-implementation of the CSS Filter Effects math, used to
// compute the pixel the bake MUST produce. Deliberately separate from the
// engine's own code so the assertion is a real cross-check.
export function expectedBakePixel(
  rgb: number[],
  filter: string,
  blend: string,
  paper: number[]
): number[] {
  const [r0, g0, b0] = rgb.map((v) => v / 255);
  let r = r0, g = g0, b = b0;
  for (const tok of filter.split(/\s+/)) {
    const m = /^([a-z-]+)\(([^)]*)\)$/.exec(tok);
    if (!m || !m[1] || !m[2]) continue;
    const a = parseFloat(m[2]);
    const nr = r, ng = g, nb = b;
    switch (m[1]) {
      case "invert":
        r = a + (1 - 2 * a) * nr; g = a + (1 - 2 * a) * ng; b = a + (1 - 2 * a) * nb;
        break;
      case "brightness":
        r = a * nr; g = a * ng; b = a * nb;
        break;
      case "contrast":
        r = a * (nr - 0.5) + 0.5; g = a * (ng - 0.5) + 0.5; b = a * (nb - 0.5) + 0.5;
        break;
      case "saturate": {
        const t = 1 - a;
        const gr = 0.213 * nr + 0.715 * ng + 0.072 * nb;
        r = gr * t + nr * a; g = gr * t + ng * a; b = gr * t + nb * a;
        break;
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
        r = M[0]! * nr + M[1]! * ng + M[2]! * nb;
        g = M[3]! * nr + M[4]! * ng + M[5]! * nb;
        b = M[6]! * nr + M[7]! * ng + M[8]! * nb;
        break;
      }
    }
    const clamped = [r, g, b].map((v) => Math.min(1, Math.max(0, v)));
    r = clamped[0]!; g = clamped[1]!; b = clamped[2]!;
  }
  const D = (x: number): number => (x <= 0.25 ? ((16 * x - 12) * x + 4) * x : Math.sqrt(x));
  const out = [r, g, b].map((s, i) => {
    const bb = paper[i]! / 255;
    let v: number;
    if (blend === "multiply") v = s * bb;
    else if (blend === "screen") v = s + bb - s * bb;
    else if (blend === "soft-light") v = s <= 0.5 ? bb - (1 - 2 * s) * bb * (1 - bb) : bb + (2 * s - 1) * (D(bb) - bb);
    else v = s;
    return Math.round(Math.min(1, Math.max(0, v)) * 255);
  });
  return out;
}

export function assertClose(actual: Uint8ClampedArray, expected: number[], label: string, tol = 3): void {
  for (let i = 0; i < 3; i += 1) {
    if (Math.abs(actual[i]! - expected[i]!) > tol) {
      throw new Error(
        label + " pixel mismatch: got [" + Array.from(actual).slice(0, 3).join(",") +
        "] expected [" + expected.join(",") + "]"
      );
    }
  }
}


// Canvas allocation tracking: render.test.ts turns this on; theme.test.ts
// asserts against it (the identity-pipeline fast path must allocate zero
// page-sized bake canvases).
export const created: FakeCanvas[] = [];

export function trackCreatedCanvases(): void {
  const realCreate = fakeDocument.createElement;
  fakeDocument.createElement = (tag: string) => {
    const el = realCreate(tag);
    created.push(el as unknown as FakeCanvas);
    return el;
  };
}

export function setFakeComputed(v: {
  "--canvas-filter": string;
  "--canvas-blend": string;
  paper?: string;
}): void {
  fakeComputed = v;
}


export type {
  EngineResult,
  OpenPayload,
  PDFReaderHandle,
  RenderPayload,
  StatsPayload,
  ThumbPayload,
};
