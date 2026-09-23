// Runs before the Trunk wasm module. Module scripts are ordered, and this
// file top-level-awaits, so the wasm module does not evaluate until the boot
// object exists. A shelf boot never lets that module evaluate: the shelf is
// its own wasm instance, and the reader host must not start beside it. A
// reader boot loads the reader engine only. pdf.js arrives with a PDF format
// mount, not with the page.
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
import {
  HOST_PDF,
  LIBRARY_GLUE,
  LIBRARY_WASM,
  SLOT_KEY,
  artifactFor,
  dropsFor,
  gluePath,
  looksLikeHtml,
  wasmPath,
  type FormatId,
} from "./format-loader";

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
  mount?: (payload?: string) => Promise<void>;
  detach?: () => void;
  dispose?: () => Promise<void>;
  // Clears the module-scope `wasm` binding and the cached memory views.
  // Nulling exports does not. Without this the instance stays reachable
  // from the module map after the blob URL is revoked.
  release?: () => void;
  [key: string]: unknown;
}

interface FormatHandle {
  glue: FormatGlue;
  blobUrl: string;
  format: string;
}

declare global {
  interface Window {
    __MAREADER_BOOT?: Boot;
    __MAREADER_SLOT?: SlotBridge;
  }
}

interface PdfFacade {
  destroy?: () => Promise<void>;
}

function engineLoaded(): boolean {
  return typeof (globalThis as { PDFReader?: unknown }).PDFReader !== "undefined";
}

function pdfFacade(): PdfFacade | null {
  const pdf = (globalThis as { PDFReader?: PdfFacade }).PDFReader;
  return pdf ?? null;
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
// adds keys, it does not add a second owner of this one. `live` is every
// book instance this document created, including one a failed mount left
// out of the slot. Returning to the shelf drops the set, not only the slot.
const handles = new Map<string, FormatHandle>();
const live = new Set<FormatHandle>();
const released = new WeakSet<FormatHandle>();
const modules = new Map<FormatId, WebAssembly.Module>();
let libraryHandle: FormatHandle | null = null;
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
  const format = artifactFor(path);
  report(
    JSON.stringify({
      path,
      status: "error",
      error: message,
      title: "",
      page: 1,
      pageCount: 1,
      format: format === "md" ? "markdown" : format,
      firstPaint: true,
    }),
  );
}

function releaseCanvases(): void {
  try {
    document.querySelectorAll("canvas").forEach((node) => {
      const canvas = node as HTMLCanvasElement;
      canvas.width = 0;
      canvas.height = 0;
    });
  } catch {
    // The document is already going.
  }
}

async function killPdfWorker(): Promise<void> {
  const pdf = pdfFacade();
  if (!pdf || typeof pdf.destroy !== "function") return;
  try {
    await pdf.destroy();
  } catch (err) {
    console.error("[boot] pdf worker destroy failed", err);
  }
}

function forgetPdfReader(): void {
  (globalThis as { PDFReader?: unknown }).PDFReader = undefined;
  engineReady = null;
}

// Dispose while `wasm` is still set, then release the binding. Revoking the
// blob URL does not evict the module map. release() is what makes the linear
// memory unreachable.
async function releaseHandle(handle: FormatHandle | null | undefined): Promise<void> {
  if (!handle || released.has(handle)) return;
  released.add(handle);
  live.delete(handle);
  if (libraryHandle === handle) libraryHandle = null;
  for (const [key, value] of handles) {
    if (value === handle) handles.delete(key);
  }
  try {
    if (typeof handle.glue.dispose === "function") {
      await handle.glue.dispose();
    } else if (typeof handle.glue.detach === "function") {
      handle.glue.detach();
    }
  } catch (err) {
    console.error("[format] dispose failed", err);
    try {
      handle.glue.detach?.();
    } catch {
      // The view is already gone.
    }
  }
  try {
    if (typeof handle.glue.release !== "function") {
      console.error("[format] glue has no release(); the instance stays in the module map");
    } else {
      handle.glue.release();
    }
  } catch (err) {
    console.error("[format] release failed", err);
  }
  nullGlue(handle.glue);
  URL.revokeObjectURL(handle.blobUrl);
  console.info(`[mem] ${handle.format} instance gone`);
}

function assetUrl(path: string): string {
  return new URL(path, document.baseURI).href;
}

// A 200 is not proof of a module. trunk serve, without no_spa, answers a
// missing formats/pdf.js with index.html. That body starts with `<`, and
// importing it is `Unexpected token '<'` in a blob named `source`.
async function fetchAsset(path: string, kindName: string): Promise<{ url: string; bytes: Uint8Array }> {
  const url = assetUrl(path);
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(`${kindName} missing: ${path}`);
  }
  const bytes = new Uint8Array(await response.arrayBuffer());
  const head = new TextDecoder().decode(bytes.subarray(0, 64));
  if (looksLikeHtml(response.headers.get("content-type"), head)) {
    throw new Error(`${kindName} was the app page, not a module: ${path}`);
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

async function importGlue(path: string, label: string): Promise<{ glue: FormatGlue; blobUrl: string }> {
  const { bytes } = await fetchAsset(path, label);
  const source = new TextDecoder().decode(bytes);
  const named = `${source}\n//# sourceURL=${path}\n`;
  const blobUrl = URL.createObjectURL(new Blob([named], { type: "text/javascript" }));
  try {
    const glue = (await import(blobUrl)) as FormatGlue;
    if (typeof glue.default !== "function" || typeof glue.mount !== "function") {
      throw new Error(`${label} has no mount: ${path}`);
    }
    if (typeof glue.release !== "function") {
      throw new Error(`${label} has no release(): ${path}`);
    }
    return { glue, blobUrl };
  } catch (err) {
    URL.revokeObjectURL(blobUrl);
    throw err;
  }
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
    await releaseHandle(previous);
    if (mine !== generation) return;
    const format = artifactFor(path);
    if (format === HOST_PDF) {
      await loadPdfEngine();
    }
    if (mine !== generation) return;
    const gluePathname = gluePath(format);
    const imported = await importGlue(gluePathname, "format glue");
    if (mine !== generation) {
      URL.revokeObjectURL(imported.blobUrl);
      return;
    }
    const compiled = await wasmModule(format);
    if (mine !== generation) {
      URL.revokeObjectURL(imported.blobUrl);
      return;
    }
    let handle: FormatHandle | null = null;
    try {
      await imported.glue.default?.({ module_or_path: compiled });
      handle = { glue: imported.glue, blobUrl: imported.blobUrl, format };
      if (mine !== generation) {
        await releaseHandle(handle);
        return;
      }
      live.add(handle);
      handles.set(SLOT_KEY, handle);
      await imported.glue.mount?.(payload);
    } catch (err) {
      if (handle && !released.has(handle)) {
        await releaseHandle(handle);
      } else {
        try {
          imported.glue.release?.();
        } catch {
          // Init never finished.
        }
        URL.revokeObjectURL(imported.blobUrl);
      }
      throw err;
    }
  } catch (err) {
    const message = err instanceof Error ? err.message : "Could not load the format module";
    reportFailure(path, message);
  }
}

async function dropEveryBook(): Promise<void> {
  generation += 1;
  const pending = [...live];
  handles.clear();
  for (const handle of pending) {
    await releaseHandle(handle);
  }
}

async function dropLibrary(): Promise<void> {
  const handle = libraryHandle;
  libraryHandle = null;
  await releaseHandle(handle);
}

function requireLibraryPlan(): void {
  const plan = dropsFor("to-library");
  for (const id of [HOST_PDF, "text", "md", "pdf-worker", "canvases", "reader-host"]) {
    if (!plan.includes(id)) {
      throw new Error(`library switch does not drop ${id}`);
    }
  }
}

async function leaveToLibrary(): Promise<void> {
  try {
    requireLibraryPlan();
    await dropEveryBook();
    await killPdfWorker();
    releaseCanvases();
    forgetPdfReader();
  } catch (err) {
    console.error("[boot] shelf switch drop failed", err);
    releaseCanvases();
    try {
      await killPdfWorker();
    } catch {
      // Already logged.
    }
    forgetPdfReader();
  }
  console.info("[mem] shelf switch: book modules released");
  // The reader host is Trunk's module. Nothing in this page can drop it.
  // The navigation does, and the next page does not start it.
  go(withSession(location.href, "library"), "replace");
}

async function leaveToReader(): Promise<void> {
  const plan = dropsFor("to-reader");
  if (!plan.includes("library")) {
    throw new Error("reader switch does not drop the library module");
  }
  try {
    await dropLibrary();
    if (plan.includes("pdf-worker")) await killPdfWorker();
    if (plan.includes("canvases")) releaseCanvases();
    forgetPdfReader();
  } catch (err) {
    console.error("[boot] library drop failed", err);
    releaseCanvases();
    forgetPdfReader();
  }
  go(withSession(location.href, "reader"), "push");
}

async function startLibrary(): Promise<void> {
  const imported = await importGlue(LIBRARY_GLUE, "library glue");
  try {
    const { bytes } = await fetchAsset(LIBRARY_WASM, "library wasm");
    if (!isWasm(bytes)) {
      throw new Error(`library wasm is not wasm: ${LIBRARY_WASM}`);
    }
    const compiled = await WebAssembly.compile(bytes);
    await imported.glue.default?.({ module_or_path: compiled });
    const handle: FormatHandle = {
      glue: imported.glue,
      blobUrl: imported.blobUrl,
      format: "library",
    };
    libraryHandle = handle;
    await imported.glue.mount?.();
  } catch (err) {
    if (libraryHandle) {
      await releaseHandle(libraryHandle);
    } else {
      try {
        imported.glue.release?.();
      } catch {
        // Init never finished.
      }
      URL.revokeObjectURL(imported.blobUrl);
    }
    throw err;
  }
  console.info("[mem] library module mounted; reader host not started");
  // Do not resolve. The next module script is the reader host, and starting
  // it on the shelf would keep the book graph in this page.
  await new Promise(() => {});
}

function showBootError(message: string): void {
  if (!document.body) return;
  document.body.textContent = message;
}

// pagehide cannot await. detach and release are sync. destroy() terminates
// the pdf.js worker when it is called, before its promise settles.
function abandonNow(): void {
  generation += 1;
  const pending = [...live];
  if (libraryHandle) pending.push(libraryHandle);
  handles.clear();
  live.clear();
  libraryHandle = null;
  for (const handle of pending) {
    if (released.has(handle)) continue;
    released.add(handle);
    try {
      handle.glue.detach?.();
    } catch {
      // The document is leaving.
    }
    try {
      handle.glue.release?.();
    } catch {
      // Binding already clear.
    }
    nullGlue(handle.glue);
    URL.revokeObjectURL(handle.blobUrl);
  }
  releaseCanvases();
  const pdf = pdfFacade();
  if (pdf && typeof pdf.destroy === "function") {
    try {
      void pdf.destroy();
    } catch {
      // Worker already gone.
    }
  }
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
    void leaveToReader();
  },
  enterLibrary() {
    sessionStorage.removeItem(HANDOFF_KEY);
    this.open = null;
    void leaveToLibrary();
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
    return dropEveryBook();
  },
  flush() {},
};

window.__MAREADER_BOOT = boot;
window.__MAREADER_SLOT = window.__MAREADER_SLOT ?? {};

window.addEventListener("pagehide", () => {
  window.__MAREADER_BOOT?.flush();
  abandonNow();
});
window.addEventListener("unload", () => {
  window.__MAREADER_BOOT?.flush();
  abandonNow();
});

if (kind === "library") {
  try {
    await startLibrary();
  } catch (err) {
    const message = err instanceof Error ? err.message : "Could not load the library module";
    console.error("[boot] library module failed", err);
    showBootError(message);
    // Still do not start the reader host. A failed shelf is not a license
    // to open the book in the old heap.
    await new Promise(() => {});
  }
}
