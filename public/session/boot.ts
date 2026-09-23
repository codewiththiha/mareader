// Runs before the Trunk wasm module and never resolves, so that binary never
// evaluates. It has no release(). The shelf and the reader chrome are
// separate instances this file starts and drops. Opening a book releases
// the shelf and starts a new host. Returning releases that host and every
// book module, then starts a new shelf. The URL is not changed: a history
// update in this webview unloads the document and leaves the empty window.
//
// Specifiers are variables on purpose. A literal import would let the bundler
// inline pdf.js or a format wasm into this file.

import {
  HANDOFF_KEY,
  decideSession,
  parseHandoff,
  type Handoff,
} from "./handoff";
import {
  HOST_GLUE,
  HOST_PDF,
  HOST_WASM,
  LIBRARY_GLUE,
  LIBRARY_WASM,
  SLOT_KEY,
  aliveAfter,
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
  // Drops `wasmInstance`, `wasm`, and the cached memory views inside the
  // glue module. Nulling exports does not. Without this the module map
  // keeps the linear memory after the blob URL is revoked.
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

const raw = sessionStorage.getItem(HANDOFF_KEY);
const kind = decideSession(location.search, raw);

// A reader URL with nothing to open is not a reader. Do not navigate: a URL
// change in this webview unloads the document and leaves the empty window.

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
let hostHandle: FormatHandle | null = null;
let liveKind: "library" | "reader" = kind;
let swapping = false;
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

function clearBody(): void {
  releaseCanvases();
  document.body.replaceChildren();
}

async function dropHost(): Promise<void> {
  const handle = hostHandle;
  hostHandle = null;
  await releaseHandle(handle);
}

type PendingSwap =
  | { kind: "library"; fromHistory: boolean }
  | { kind: "reader"; payload: Handoff; fromHistory: boolean };

let queued: PendingSwap | null = null;

function pumpQueue(): void {
  const next = queued;
  queued = null;
  if (!next) return;
  if (next.kind === "library") void showLibrary(next.fromHistory);
  else void showReader(next.payload, next.fromHistory);
}

interface PreparedSession {
  imported: { glue: FormatGlue; blobUrl: string };
  compiled: WebAssembly.Module;
}

// Fetch and compile only. No instance yet, so the shelf can stay on screen
// while this fails. Instantiating here would put two heaps in the page.
async function prepareSession(
  gluePathname: string,
  wasmFile: string,
  label: string,
): Promise<PreparedSession> {
  const imported = await importGlue(gluePathname, label);
  try {
    const { bytes } = await fetchAsset(wasmFile, label);
    if (!isWasm(bytes)) {
      throw new Error(`${label} is not wasm: ${wasmFile}`);
    }
    const compiled = await WebAssembly.compile(bytes);
    return { imported, compiled };
  } catch (err) {
    try {
      imported.glue.release?.();
    } catch {
      // Init never finished.
    }
    URL.revokeObjectURL(imported.blobUrl);
    throw err;
  }
}

async function activateSession(prepared: PreparedSession, label: string): Promise<FormatHandle> {
  try {
    await prepared.imported.glue.default?.({ module_or_path: prepared.compiled });
    const handle: FormatHandle = {
      glue: prepared.imported.glue,
      blobUrl: prepared.imported.blobUrl,
      format: label,
    };
    await prepared.imported.glue.mount?.();
    return handle;
  } catch (err) {
    try {
      prepared.imported.glue.release?.();
    } catch {
      // Init never finished.
    }
    URL.revokeObjectURL(prepared.imported.blobUrl);
    throw err;
  }
}

async function startSession(
  gluePathname: string,
  wasmFile: string,
  label: string,
): Promise<FormatHandle> {
  const prepared = await prepareSession(gluePathname, wasmFile, label);
  return activateSession(prepared, label);
}

// In-page. A query navigation does not drop the previous wasm in this
// webview, which is why each open/close kept another instance. The leaving
// module is disposed and released before the arriving one is created.
async function showLibrary(fromHistory: boolean): Promise<void> {
  if (swapping) {
    queued = { kind: "library", fromHistory };
    return;
  }
  if (liveKind === "library" && libraryHandle && live.size === 0 && !hostHandle) return;
  swapping = true;
  try {
    requireLibraryPlan();
    if (aliveAfter("to-library") !== "library") {
      throw new Error("library switch left the reader alive");
    }
    sessionStorage.removeItem(HANDOFF_KEY);
    await dropEveryBook();
    await dropHost();
    await dropLibrary();
    await killPdfWorker();
    forgetPdfReader();
    clearBody();
    boot.kind = "library";
    boot.open = null;
    libraryHandle = await startSession(LIBRARY_GLUE, LIBRARY_WASM, "library");
    liveKind = "library";
    console.info("[mem] library module mounted; reader host released");
  } catch (err) {
    console.error("[boot] shelf switch failed", err);
    showBootError(err instanceof Error ? err.message : "Could not return to the library");
  } finally {
    swapping = false;
    pumpQueue();
  }
}

async function showReader(payload: Handoff, fromHistory: boolean): Promise<void> {
  if (swapping) {
    queued = { kind: "reader", payload, fromHistory };
    return;
  }
  swapping = true;
  try {
    const plan = dropsFor("to-reader");
    if (!plan.includes("library") || aliveAfter("to-reader") !== "reader-host") {
      throw new Error("reader switch does not drop the library module");
    }
    // Do not touch the URL. This webview treats a history change as a
    // navigation and replaces the document with the empty window.
    // Fetch the host before dropping the shelf, so a missing artifact
    // leaves the library on screen.
    sessionStorage.setItem(HANDOFF_KEY, JSON.stringify(payload));
    const prepared = await prepareSession(HOST_GLUE, HOST_WASM, "reader-host");
    await dropEveryBook();
    await dropLibrary();
    await dropHost();
    if (plan.includes("pdf-worker")) await killPdfWorker();
    forgetPdfReader();
    clearBody();
    paintShell();
    boot.kind = "reader";
    boot.open = payload;
    try {
      await load(READER_ENGINE);
    } catch (err) {
      console.error("[boot] reader engine failed to load", err);
    }
    hostHandle = await activateSession(prepared, "reader-host");
    liveKind = "reader";
    console.info("[mem] reader host mounted; library module released");
  } catch (err) {
    const message = err instanceof Error ? err.message : "Could not open the book";
    console.error("[boot] reader switch failed", err);
    boot.kind = "library";
    boot.open = null;
    hostHandle = null;
    liveKind = "library";
    if (!libraryHandle) {
      try {
        libraryHandle = await startSession(LIBRARY_GLUE, LIBRARY_WASM, "library");
      } catch (restoreErr) {
        console.error("[boot] shelf restore failed", restoreErr);
      }
    }
    showBootError(message);
  } finally {
    swapping = false;
    pumpQueue();
  }
}

function paintShell(): void {
  const root = document.documentElement;
  const body = document.body;
  if (!root || !body) return;
  // The shelf view is what paints the window. Once it is gone, a transparent
  // document shows the webview's own background — the empty Tauri window.
  root.style.background = "#1c1917";
  body.style.background = "#1c1917";
  body.style.color = "#fafaf9";
}

function showBootError(message: string): void {
  paintShell();
  if (!document.body) return;
  const node = document.createElement("div");
  node.setAttribute("role", "alert");
  node.style.cssText = [
    "position:fixed",
    "inset:0",
    "z-index:2147483647",
    "box-sizing:border-box",
    "background:#1c1917",
    "color:#fafaf9",
    "font:16px/1.45 ui-sans-serif,system-ui,sans-serif",
    "padding:28px",
    "white-space:pre",
  ].join(";");
  node.textContent = message;
  document.body.appendChild(node);
}

// pagehide cannot await. detach and release are sync. destroy() terminates
// the pdf.js worker when it is called, before its promise settles.
function abandonNow(): void {
  generation += 1;
  const pending = [...live];
  if (libraryHandle) pending.push(libraryHandle);
  if (hostHandle) pending.push(hostHandle);
  handles.clear();
  live.clear();
  libraryHandle = null;
  hostHandle = null;
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
    void showReader(stored, false);
  },
  enterLibrary() {
    sessionStorage.removeItem(HANDOFF_KEY);
    this.open = null;
    void showLibrary(false);
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
window.addEventListener("popstate", () => {
  if (swapping) return;
  const next = decideSession(location.search, sessionStorage.getItem(HANDOFF_KEY));
  if (next === liveKind) return;
  if (next === "library") {
    void showLibrary(true);
    return;
  }
  const open = boot.open ?? parseHandoff(sessionStorage.getItem(HANDOFF_KEY));
  if (open) void showReader(open, true);
});

try {
  if (kind === "reader") {
    hostHandle = await startSession(HOST_GLUE, HOST_WASM, "reader-host");
    liveKind = "reader";
    console.info("[mem] reader host mounted; library module not started");
  } else {
    libraryHandle = await startSession(LIBRARY_GLUE, LIBRARY_WASM, "library");
    liveKind = "library";
    console.info("[mem] library module mounted; reader host not started");
  }
} catch (err) {
  const message = err instanceof Error ? err.message : "Could not load the session module";
  console.error("[boot] session module failed", err);
  showBootError(message);
}
// The next module script is Trunk's host. It must not evaluate: that binary
// cannot be released, and a second copy is the leak.
await new Promise(() => {});
