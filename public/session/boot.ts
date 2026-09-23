// Runs before the Trunk wasm module. Module scripts are ordered, and this
// file top-level-awaits, so the wasm module does not evaluate until the boot
// object exists. A reader boot loads the reader engine only. pdf.js arrives
// with a PDF format mount, not with the page.
//
// Specifiers are variables on purpose. A literal import would let the bundler
// inline pdf.js or a format wasm into this file.

import {
  HANDOFF_KEY,
  decideSession,
  navigation,
  parseHandoff,
  withSession,
  type Handoff,
  type HistoryMode,
} from "./handoff";
import { HOST_PDF, SLOT_KEY, artifactFor, gluePath, looksLikeHtml, wasmPath, type FormatId } from "./format-loader";

const PDFJS = "/vendor/pdfjs/pdf.min.mjs";
const PDF_ENGINE = "/pdfEngine.js";
const READER_ENGINE = "/readerEngine.js";

interface Boot {
  kind: "library" | "reader";
  open: Handoff | null;
  takeOpen(): Handoff | null;
  enterReader(payload: Handoff): void;
  enterLibrary(): void;
  clearHandoff(): void;
  ensureEngine(): Promise<void>;
  mountFormat(payload: string): Promise<void>;
  dropFormat(): Promise<void>;
  flush(): void;
}

interface SlotBridge {
  report?: (snapshot: string) => void;
  command?: (command: string) => void;
}

interface FormatGlue {
  default?: (input: { module_or_path: WebAssembly.Module }) => Promise<unknown>;
  mount?: (payload: string) => Promise<void>;
  dispose?: () => Promise<void>;
  [key: string]: unknown;
}

interface FormatHandle {
  glue: FormatGlue;
  blobUrl: string;
}

declare global {
  interface Window {
    __MAREADER_BOOT?: Boot;
    __MAREADER_SLOT?: SlotBridge;
  }
}

function engineLoaded(): boolean {
  return typeof (globalThis as { PDFReader?: unknown }).PDFReader !== "undefined";
}

async function load(url: string): Promise<void> {
  await import(url);
}

let engineReady: Promise<void> | null = null;

function loadPdfEngine(): Promise<void> {
  if (engineLoaded()) return Promise.resolve();
  if (!engineReady) {
    engineReady = (async () => {
      await load(PDFJS);
      await load(PDF_ENGINE);
    })();
  }
  return engineReady;
}

function go(next: string, prefer: "push" | "replace"): void {
  const mode: HistoryMode = navigation(location.href, next, prefer);
  if (mode === "reload") location.reload();
  else if (mode === "push") location.assign(next);
  else location.replace(next);
}

const raw = sessionStorage.getItem(HANDOFF_KEY);
const kind = decideSession(location.search, raw);

// A reader URL with nothing to open is not a reader. Hang this module so the
// wasm script behind it never starts on a page that is already leaving.
if (new URLSearchParams(location.search).get("session") === "reader" && kind !== "reader") {
  location.replace(withSession(location.href, "library"));
  await new Promise(() => {});
}

if (kind === "reader") {
  try {
    await load(READER_ENGINE);
  } catch (err) {
    console.error("[boot] reader engine failed to load", err);
  }
}

// One slot. The map exists so a second mount has a key to clear; phase 3
// adds keys, it does not add a second owner of this one.
const handles = new Map<string, FormatHandle>();
const modules = new Map<FormatId, WebAssembly.Module>();
let generation = 0;

function nullGlue(glue: FormatGlue): void {
  for (const key of Object.keys(glue)) {
    glue[key] = undefined;
  }
}

function reportFailure(path: string, message: string): void {
  console.error("[format]", message);
  const report = window.__MAREADER_SLOT?.report;
  if (typeof report !== "function") return;
  report(
    JSON.stringify({
      path,
      status: "error",
      error: message,
      title: "",
      page: 1,
      pageCount: 1,
      format: artifactFor(path) === "md" ? "markdown" : artifactFor(path),
      firstPaint: true,
    }),
  );
}

// Drop order is DROP_ORDER. Flush already happened on the host side before
// it asked. Dispose runs the format's owner cleanup and its engine teardown.
// Nulling the glue drops the exports and any memory view it closed over.
// The Module cache is code and stays. The instance does not.
async function release(handle: FormatHandle | undefined): Promise<void> {
  if (!handle) return;
  try {
    if (typeof handle.glue.dispose === "function") {
      await handle.glue.dispose();
    }
  } catch (err) {
    console.error("[format] dispose failed", err);
  }
  nullGlue(handle.glue);
  URL.revokeObjectURL(handle.blobUrl);
  console.info("[mem] format instance gone");
}

function assetUrl(path: string): string {
  return new URL(path, document.baseURI).href;
}

// A 200 is not proof of a module. trunk serve, without no_spa, answers a
// missing formats/pdf.js with index.html. That body starts with `<`, and
// importing it is `Unexpected token '<'` in a blob named `source`.
async function fetchAsset(path: string, kind: string): Promise<{ url: string; bytes: Uint8Array }> {
  const url = assetUrl(path);
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(`${kind} missing: ${path}`);
  }
  const bytes = new Uint8Array(await response.arrayBuffer());
  const head = new TextDecoder().decode(bytes.subarray(0, 64));
  if (looksLikeHtml(response.headers.get("content-type"), head)) {
    throw new Error(`${kind} was the app page, not a module: ${path}`);
  }
  return { url, bytes };
}

function isWasm(bytes: Uint8Array): boolean {
  return bytes.length >= 4 && bytes[0] === 0 && bytes[1] === 0x61 && bytes[2] === 0x73 && bytes[3] === 0x6d;
}

async function wasmModule(format: FormatId): Promise<WebAssembly.Module> {
  const cached = modules.get(format);
  if (cached) return cached;
  const path = wasmPath(format);
  const { bytes } = await fetchAsset(path, "format wasm");
  if (!isWasm(bytes)) {
    throw new Error(`format wasm is not wasm: ${path}`);
  }
  const compiled = await WebAssembly.compile(bytes);
  modules.set(format, compiled);
  return compiled;
}

async function mountFormatNow(payload: string): Promise<void> {
  let path = "";
  try {
    const parsed = JSON.parse(payload) as { path?: unknown };
    path = typeof parsed.path === "string" ? parsed.path : "";
    if (path.length === 0) return;
    const mine = ++generation;
    const previous = handles.get(SLOT_KEY);
    handles.delete(SLOT_KEY);
    await release(previous);
    if (mine !== generation) return;
    const format = artifactFor(path);
    if (format === HOST_PDF) {
      await loadPdfEngine();
    }
    if (mine !== generation) return;
    const gluePathname = gluePath(format);
    const { url: glueUrl, bytes } = await fetchAsset(gluePathname, "format glue");
    const source = new TextDecoder().decode(bytes);
    // sourceURL is the name the debugger shows. Without it a blob is `source`,
    // which is not the book on the title bar.
    const named = `${source}\n//# sourceURL=${gluePathname}\n`;
    const blobUrl = URL.createObjectURL(new Blob([named], { type: "text/javascript" }));
    // A variable, so the bundler cannot inline the glue or the wasm it loads.
    const glue = (await import(blobUrl)) as FormatGlue;
    if (mine !== generation) {
      URL.revokeObjectURL(blobUrl);
      return;
    }
    if (typeof glue.default !== "function" || typeof glue.mount !== "function") {
      URL.revokeObjectURL(blobUrl);
      throw new Error(`format glue has no mount: ${glueUrl}`);
    }
    const compiled = await wasmModule(format);
    if (mine !== generation) {
      URL.revokeObjectURL(blobUrl);
      return;
    }
    // A Module, not an Instance. init builds a new heap from it.
    await glue.default({ module_or_path: compiled });
    if (mine !== generation) {
      nullGlue(glue);
      URL.revokeObjectURL(blobUrl);
      return;
    }
    handles.set(SLOT_KEY, { glue, blobUrl });
    await glue.mount(payload);
  } catch (err) {
    const message = err instanceof Error ? err.message : "Could not load the format module";
    reportFailure(path, message);
  }
}

async function dropFormatNow(): Promise<void> {
  generation += 1;
  const handle = handles.get(SLOT_KEY);
  handles.delete(SLOT_KEY);
  await release(handle);
}

function abandonNow(): void {
  generation += 1;
  const handle = handles.get(SLOT_KEY);
  handles.delete(SLOT_KEY);
  if (!handle) return;
  const dispose = handle.glue.dispose;
  if (typeof dispose === "function") {
    try {
      void dispose();
    } catch {
      // The document is leaving. The reclaim is that death.
    }
  }
  nullGlue(handle.glue);
  URL.revokeObjectURL(handle.blobUrl);
}

const boot: Boot = {
  kind,
  open: kind === "reader" ? parseHandoff(raw) : null,
  takeOpen() {
    const value = this.open;
    this.open = null;
    sessionStorage.removeItem(HANDOFF_KEY);
    return value;
  },
  enterReader(payload) {
    if (!payload || typeof payload.path !== "string" || payload.path.length === 0) return;
    const stored: Handoff =
      typeof payload.bookId === "string" && payload.bookId.length > 0
        ? { path: payload.path, bookId: payload.bookId }
        : { path: payload.path };
    sessionStorage.setItem(HANDOFF_KEY, JSON.stringify(stored));
    // Push, so Back from the reader loads the shelf as a new document.
    go(withSession(location.href, "reader"), "push");
  },
  enterLibrary() {
    sessionStorage.removeItem(HANDOFF_KEY);
    this.open = null;
    // Replace, so Back from the shelf does not restore a reader URL.
    go(withSession(location.href, "library"), "replace");
  },
  clearHandoff() {
    sessionStorage.removeItem(HANDOFF_KEY);
    this.open = null;
  },
  ensureEngine() {
    return loadPdfEngine();
  },
  mountFormat(payload) {
    return mountFormatNow(payload);
  },
  dropFormat() {
    return dropFormatNow();
  },
  flush() {},
};

window.__MAREADER_BOOT = boot;
window.__MAREADER_SLOT = window.__MAREADER_SLOT ?? {};

// pagehide is the flush, then the format handle. unload is the
// back-forward-cache opt-out: a cached document is still alive, and a living
// document still holds the wasm heap.
window.addEventListener("pagehide", () => {
  window.__MAREADER_BOOT?.flush();
  abandonNow();
});
window.addEventListener("unload", () => {
  window.__MAREADER_BOOT?.flush();
  abandonNow();
});
