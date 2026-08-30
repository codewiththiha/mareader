import { readFileSync } from "node:fs";
import vm from "node:vm";
// Minimal harness to smoke-test pdfEngine outside a browser.
// Stubs: pdfjsLib, DOM (canvases), Tauri globals, rAF, getComputedStyle.
//
// Reads the COMPILED `public/pdfEngine.js` (the same IIFE artifact the
// browser loads) and evaluates it in a vm sandbox. The bundle carries no
// module syntax (esbuild IIFE), so it is evaluated as-is — no source
// rewriting.
const engineSrc = readFileSync(new URL("../../public/pdfEngine.js", import.meta.url), "utf8");
// ---------- canvas stub (pixel-accurate) ----------
export class FakeCtx {
    canvas;
    filter = "none";
    globalCompositeOperation = "source-over";
    fillStyle = "#000000";
    _data = new Uint8ClampedArray(0);
    constructor(canvas) {
        this.canvas = canvas;
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
        if (m) {
            return [parseInt(m[1], 16), parseInt(m[2], 16), parseInt(m[3], 16)];
        }
        m = /^rgba?\(([^)]+)\)$/.exec(String(c).trim());
        if (m) {
            const p = m[1].split(",").map((x) => parseFloat(x));
            return [p[0] ?? 0, p[1] ?? 0, p[2] ?? 0];
        }
        return [0, 0, 0];
    }
    blend(s, b, op) {
        switch (op) {
            case "multiply": return s * b;
            case "screen": return s + b - s * b;
            case "soft-light": {
                const D = (x) => x <= 0.25 ? ((16 * x - 12) * x + 4) * x : Math.sqrt(x);
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
        if (x < 0 || y < 0 || x >= w || y >= h)
            return;
        const d = this._ensure();
        const i = (y * w + x) * 4;
        const op = this.globalCompositeOperation;
        d[i] = Math.round(this.blend(r / 255, d[i] / 255, op) * 255);
        d[i + 1] = Math.round(this.blend(g / 255, d[i + 1] / 255, op) * 255);
        d[i + 2] = Math.round(this.blend(b / 255, d[i + 2] / 255, op) * 255);
        d[i + 3] = 255;
    }
    fillRect(x, y, w, h) {
        const [r, g, b] = this.parseColor(String(this.fillStyle));
        const fw = Math.min(Math.floor(w), (this.canvas.width || 0) - x);
        const fh = Math.min(Math.floor(h), (this.canvas.height || 0) - y);
        for (let yy = 0; yy < fh; yy += 1) {
            for (let xx = 0; xx < fw; xx += 1) {
                this.paint(x + xx, y + yy, r, g, b);
            }
        }
    }
    drawImage(src, x, y) {
        const sw = src.width ?? src.width;
        const sh = src.height ?? src.height;
        if (!sw || !sh)
            return;
        let sdata = null;
        const fc = src;
        if (fc._ctx && fc._ctx._data)
            sdata = fc._ctx._ensure();
        else if (fc.data)
            sdata = fc.data;
        if (!sdata)
            return; // e.g. a stub ImageBitmap: nothing to copy
        for (let yy = 0; yy < sh; yy += 1) {
            for (let xx = 0; xx < sw; xx += 1) {
                const si = (yy * sw + xx) * 4;
                this.paint(x + xx, y + yy, sdata[si], sdata[si + 1], sdata[si + 2]);
            }
        }
    }
    getImageData(x, y, w, h) {
        const d = this._ensure();
        const out = new Uint8ClampedArray(w * h * 4);
        for (let yy = 0; yy < h; yy += 1) {
            const row = (y + yy) * (this.canvas.width || 0) + x;
            out.set(d.subarray(row * 4, (row + w) * 4), yy * w * 4);
        }
        return { data: out, width: w, height: h, colorSpace: "srgb" };
    }
    putImageData(img, x, y) {
        const d = this._ensure();
        for (let yy = 0; yy < img.height; yy += 1) {
            const row = (y + yy) * (this.canvas.width || 0) + x;
            d.set(img.data.subarray(yy * img.width * 4, (yy + 1) * img.width * 4), row * 4);
        }
    }
}
export class FakeCanvas {
    tagName;
    width = 300;
    height = 150;
    _ctx;
    style = { setProperty() { }, removeProperty() { } };
    className = "";
    classList = { add() { }, remove() { }, toggle() { }, contains() { return false; } };
    children = [];
    dataset = {};
    href = "";
    target = "";
    rel = "";
    title = "";
    parentElement = null;
    _style = null;
    constructor(tag = "canvas") {
        this.tagName = String(tag).toUpperCase();
        this._ctx = new FakeCtx(this);
    }
    getContext() { return this._ctx; }
    toDataURL() { return "data:image/jpeg;base64,xx"; }
    setAttribute() { }
    getAttribute() { return null; }
    appendChild(c) { this.children.push(c); return c; }
    replaceChildren() { }
    remove() { }
    replaceWith() { }
    querySelectorAll() { return []; }
    querySelector() { return null; }
    addEventListener() { }
    getBoundingClientRect() {
        return { x: 0, y: 0, width: 100, height: 100 };
    }
}
function makeElement(id) {
    const el = {
        id,
        width: 0,
        height: 0,
        style: { setProperty() { }, removeProperty() { } },
        className: "",
        classList: { add() { }, remove() { }, toggle() { }, contains() { return false; } },
        children: [],
        querySelectorAll() { return []; },
        querySelector() { return null; },
        appendChild(c) { el.children.push(c); return c; },
        replaceChildren() { },
        remove() { },
        replaceWith() { },
        setAttribute() { },
        getAttribute() { return null; },
        getBoundingClientRect() { return { x: 0, y: 0, width: 100, height: 100 }; },
        getContext() {
            if (!el._ctx)
                el._ctx = new FakeCtx(el);
            return el._ctx;
        },
        parentElement: null,
    };
    return el;
}
const elements = new Map();
export function getEl(id) {
    if (id === "documentElement")
        return docEl;
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
const docEl = (() => {
    let _style = null;
    // Inline custom properties written by the engine (e.g. --pdf-paper).
    // Recorded, not ignored: the render test asserts on them.
    const props = new Map();
    const el = {
        id: "documentElement",
        width: 0,
        height: 0,
        className: "",
        style: {
            getAttribute: () => _style,
            setProperty: (name, value) => { props.set(name, value); },
            removeProperty: (name) => { props.delete(name); },
            getPropertyValue: (name) => props.get(name) ?? "",
        },
        classList: { add() { }, remove() { }, toggle() { }, contains() { return false; } },
        getAttribute() { return _style; },
        setAttribute(k, v) { if (k === "style")
            _style = v; },
        appendChild() { },
        getBoundingClientRect() { return { x: 0, y: 0, width: 0, height: 0 }; },
        children: [],
        querySelectorAll() { return []; },
        querySelector() { return null; },
        replaceChildren() { },
        remove() { },
        replaceWith() { },
    };
    return el;
})();
export const fakeDocument = {
    documentElement: docEl,
    createElement: (tag) => new FakeCanvas(tag),
    getElementById: (id) => getEl(id),
    addEventListener() { },
    getSelection: () => null,
    createRange: () => ({ setStart() { }, setEnd() { }, getClientRects: () => [], detach() { } }),
    querySelectorAll: (sel) => {
        if (sel === "canvas.thumb-canvas" || sel === "canvas") {
            return [...elements.values()].filter((e) => String(e.id).startsWith("thumb-") || String(e.id).includes("-cv"));
        }
        return [];
    },
};
export let fakeComputed = {
    "--canvas-filter": "none",
    "--canvas-blend": "normal",
};
/** The store behind the harness localStorage stub. */
export const fakeLocalStorage = new Map();
export const fakeWindow = {
    devicePixelRatio: 2,
    innerWidth: 1280,
    innerHeight: 800,
    // Real-enough storage: the engine's per-document paper cache
    // (pdfreader.blend-paper.v2) reads and writes through globalThis.localStorage.
    localStorage: {
        getItem: (k) => fakeLocalStorage.get(k) ?? null,
        setItem: (k, v) => { fakeLocalStorage.set(k, v); },
    },
    addEventListener() { },
    dispatchEvent() { return true; },
    getComputedStyle: (_el) => ({
        getPropertyValue: (name) => {
            if (name === "--canvas-filter")
                return fakeComputed["--canvas-filter"] || "none";
            if (name === "--canvas-blend")
                return fakeComputed["--canvas-blend"] || "normal";
            return "";
        },
        backgroundColor: fakeComputed.paper || "#ffffff",
    }),
    requestAnimationFrame: (fn) => { setTimeout(fn, 0); return 1; },
    cancelAnimationFrame() { },
    __TAURI__: {
        core: {
            invoke: async (cmd) => {
                if (cmd === "read_file_bytes")
                    return [1, 2, 3, 4];
                if (cmd === "take_pending_file")
                    return null;
                throw new Error("unknown command " + cmd);
            },
            convertFileSrc: (p) => "asset://" + p,
        },
    },
};
// ---------- pdf.js stub ----------
// Per-page paint colours, defaulting to paper white. The blend-scope test
// paints distinct pages so detection, the document scan and the continuous
// interpolation have something to tell apart; every other scenario sees the
// same all-white book it always did.
const fakePageColors = new Map();
export function setFakePageColors(colors) {
    fakePageColors.clear();
    for (const [page, color] of Object.entries(colors)) {
        fakePageColors.set(Number(page), color);
    }
}
function fakePage(n) {
    return {
        n,
        getViewport: ({ scale }) => ({
            width: 600 * scale,
            height: 800 * scale,
            convertToViewportPoint: (x, y) => [x, y],
        }),
        render: ({ canvasContext }) => {
            canvasContext.fillStyle = fakePageColors.get(n) ?? "#ffffff";
            canvasContext.fillRect(0, 0, canvasContext.canvas.width, canvasContext.canvas.height);
            return { promise: Promise.resolve(), cancel() { } };
        },
        getTextContent: async () => ({ items: [] }),
        getAnnotations: async () => [],
        cleanup: async () => { },
    };
}
const fakePdf = {
    numPages: 5,
    // Range-strict like the real pdf.js: an out-of-range page rejects, which
    // the paper sampler must swallow into a frameless {ok:true} skip.
    getPage: async (n) => {
        if (n < 1 || n > fakePdf.numPages) {
            throw new Error("page out of range: " + n);
        }
        return fakePage(n);
    },
    getMetadata: async () => ({ info: { Title: "Test", Author: null } }),
    getOutline: async () => [],
    getPageIndex: async () => 0,
    getDestination: async () => null,
    cleanup: async () => { },
};
const fakeLoadingTask = { promise: Promise.resolve(fakePdf), destroy: async () => { } };
const sandbox = {
    console,
    addEventListener() { },
    dispatchEvent() { return true; },
    getComputedStyle: fakeWindow.getComputedStyle,
    globalThis: {},
    document: fakeDocument,
    window: fakeWindow,
    localStorage: fakeWindow.localStorage,
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
    CustomEvent: class CustomEvent {
        type;
        detail;
        constructor(t, d) { this.type = t; this.detail = d; }
    },
    URL,
    fetch: async () => { throw new Error("fetch disabled in harness"); },
    createImageBitmap: async (c) => {
        const ctx = c._ctx;
        const data = ctx ? ctx._ensure().slice() : new Uint8ClampedArray(0);
        return {
            width: c.width,
            height: c.height,
            data,
            close() { },
        };
    },
    pdfjsLib: {
        getDocument: () => fakeLoadingTask,
        GlobalWorkerOptions: {},
        TextLayer: class {
            constructor() { }
            async render() { }
            cancel() { }
        },
    },
};
sandbox.globalThis = sandbox;
sandbox.__TAURI__ = fakeWindow.__TAURI__;
vm.createContext(sandbox);
vm.runInContext(engineSrc, sandbox, { filename: "pdfEngine.js" });
export const PDFReader = sandbox.PDFReader;
if (!PDFReader)
    throw new Error("PDFReader not defined after eval");
// Independent re-implementation of the CSS Filter Effects math, used to
// compute the pixel the bake MUST produce. Deliberately separate from the
// engine's own code so the assertion is a real cross-check.
export function expectedBakePixel(rgb, filter, blend, paper) {
    const [r0, g0, b0] = rgb.map((v) => v / 255);
    let r = r0, g = g0, b = b0;
    for (const tok of filter.split(/\s+/)) {
        const m = /^([a-z-]+)\(([^)]*)\)$/.exec(tok);
        if (!m || !m[1] || !m[2])
            continue;
        const a = parseFloat(m[2]);
        const nr = r, ng = g, nb = b;
        switch (m[1]) {
            case "invert":
                r = a + (1 - 2 * a) * nr;
                g = a + (1 - 2 * a) * ng;
                b = a + (1 - 2 * a) * nb;
                break;
            case "brightness":
                r = a * nr;
                g = a * ng;
                b = a * nb;
                break;
            case "contrast":
                r = a * (nr - 0.5) + 0.5;
                g = a * (ng - 0.5) + 0.5;
                b = a * (nb - 0.5) + 0.5;
                break;
            case "saturate": {
                const t = 1 - a;
                const gr = 0.213 * nr + 0.715 * ng + 0.072 * nb;
                r = gr * t + nr * a;
                g = gr * t + ng * a;
                b = gr * t + nb * a;
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
                r = M[0] * nr + M[1] * ng + M[2] * nb;
                g = M[3] * nr + M[4] * ng + M[5] * nb;
                b = M[6] * nr + M[7] * ng + M[8] * nb;
                break;
            }
        }
        const clamped = [r, g, b].map((v) => Math.min(1, Math.max(0, v)));
        r = clamped[0];
        g = clamped[1];
        b = clamped[2];
    }
    const D = (x) => (x <= 0.25 ? ((16 * x - 12) * x + 4) * x : Math.sqrt(x));
    const out = [r, g, b].map((s, i) => {
        const bb = paper[i] / 255;
        let v;
        if (blend === "multiply")
            v = s * bb;
        else if (blend === "screen")
            v = s + bb - s * bb;
        else if (blend === "soft-light")
            v = s <= 0.5 ? bb - (1 - 2 * s) * bb * (1 - bb) : bb + (2 * s - 1) * (D(bb) - bb);
        else
            v = s;
        return Math.round(Math.min(1, Math.max(0, v)) * 255);
    });
    return out;
}
export function assertClose(actual, expected, label, tol = 3) {
    for (let i = 0; i < 3; i += 1) {
        if (Math.abs(actual[i] - expected[i]) > tol) {
            throw new Error(label + " pixel mismatch: got [" + Array.from(actual).slice(0, 3).join(",") +
                "] expected [" + expected.join(",") + "]");
        }
    }
}
// Canvas allocation tracking: render.test.ts turns this on; theme.test.ts
// asserts against it (the identity-pipeline fast path must allocate zero
// page-sized bake canvases).
export const created = [];
export function trackCreatedCanvases() {
    const realCreate = fakeDocument.createElement;
    fakeDocument.createElement = (tag) => {
        const el = realCreate(tag);
        created.push(el);
        return el;
    };
}
export function setFakeComputed(v) {
    fakeComputed = v;
}
