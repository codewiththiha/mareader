// Runs before the Trunk wasm module and never resolves, so that binary never
// evaluates. It has no release(). The shelf, the reader chrome, and each
// book format are modules this file activates and deactivates. Opening a
// book deactivates the shelf completely and activates a new host, then a
// pdf, text, or markdown module. Returning deactivates those and activates
// a new shelf. The URL is not changed: a history update in this webview
// unloads the document and leaves the empty window.
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
  HOST_PDF,
  SLOT_KEY,
  aliveAfter,
  artifactFor,
  dropsFor,
  isFormatModule,
  isSessionModuleId,
  looksLikeHtml,
  moduleGlue,
  moduleWasm,
  releaseBefore,
  type SessionModuleId,
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
  activate(module: string, payload?: string): Promise<void>;
  deactivate(module: string): Promise<void>;
  flush(): void;
}

interface SlotBridge {
  report?: (snapshot: string) => void;
  command?: (command: string) => void;
}

interface FormatGlue {
  default?: (
    input: WebAssembly.Module | { module_or_path: WebAssembly.Module },
  ) => Promise<unknown>;
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
let libraryHandle: FormatHandle | null = null;
let hostHandle: FormatHandle | null = null;
const active = new Map<SessionModuleId, FormatHandle>();
let liveKind: "library" | "reader" = kind;
let swapping = false;
let generation = 0;
let blobNonce = 0;

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
  // The facade is frozen, and pdf.js does not re-evaluate, so deleting the
  // binding would leave the next book unable to open. destroy() already
  // dropped the worker. Do not assign through the facade: that is the Safari
  // readonly error, and it used to abort the shelf before the new module
  // mounted.
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
  if (hostHandle === handle) hostHandle = null;
  for (const [id, value] of active) {
    if (value === handle) active.delete(id);
  }
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

async function importGlue(path: string, label: string): Promise<{ glue: FormatGlue; blobUrl: string }> {
  const { bytes } = await fetchAsset(path, label);
  const source = new TextDecoder().decode(bytes);
  // A fresh source each import. A webview that keys the module map by text
  // would otherwise hand back the module whose bindings release() just cleared,
  // and the next init assigns into that sealed environment.
  const instance = ++blobNonce;
  // A stable sourceURL is a module-map key in this webview, so the second
  // import came back as the module release() had just cleared and the next
  // init assigned into that sealed environment.
  const named = `${source}\n// mareader-instance ${instance}\n//# sourceURL=${path}?instance=${instance}\n`;
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
    const format = artifactFor(path);
    const mine = ++generation;
    // Compile the next book before dropping the one on screen, so a missing
    // artifact leaves the current page up.
    const prepared = await prepareModule(format);
    if (mine !== generation) {
      discardPrepared(prepared);
      return;
    }
    for (const id of releaseBefore(format)) {
      await settle(id, () => deactivateModule(id));
    }
    if (mine !== generation) {
      discardPrepared(prepared);
      return;
    }
    if (format === HOST_PDF) await loadPdfEngine();
    if (mine !== generation) {
      discardPrepared(prepared);
      return;
    }
    const handle = await activateSession(prepared, format, payload);
    if (mine !== generation) {
      await releaseHandle(handle);
      return;
    }
    remember(format, handle);
    console.info(`[mem] ${format} module mounted`);
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
  const body = ensureBody();
  if (!body) return;
  try {
    body.replaceChildren();
  } catch {
    // The next mount creates its own nodes.
  }
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

async function startGlue(glue: FormatGlue, compiled: WebAssembly.Module): Promise<void> {
  const init = glue.default;
  if (typeof init !== "function") throw new Error("module has no init");
  // Prefer the compiled module. The object form assigns back into the
  // parameter, and WKWebView throws "Attempted to assign to readonly
  // property" when that glue is entered again. Fall back once for a glue
  // that only accepts the object.
  try {
    await init(compiled);
  } catch (directErr) {
    try {
      await init({ module_or_path: compiled });
    } catch {
      throw directErr;
    }
  }
}

async function activateSession(
  prepared: PreparedSession,
  label: string,
  payload?: string,
): Promise<FormatHandle> {
  const glue = prepared.imported.glue;
  try {
    await startGlue(glue, prepared.compiled);
    const handle: FormatHandle = {
      glue,
      blobUrl: prepared.imported.blobUrl,
      format: label,
    };
    if (payload === undefined) await glue.mount?.();
    else await glue.mount?.(payload);
    return handle;
  } catch (err) {
    try {
      if (typeof glue.dispose === "function") await glue.dispose();
      else glue.detach?.();
    } catch {
      // Init never finished, or the view is already gone.
    }
    try {
      glue.release?.();
    } catch {
      // Binding already clear.
    }
    try {
      URL.revokeObjectURL(prepared.imported.blobUrl);
    } catch {
      // Already revoked.
    }
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

function remember(id: SessionModuleId, handle: FormatHandle): void {
  active.set(id, handle);
  if (id === "library") libraryHandle = handle;
  else if (id === "reader-host") hostHandle = handle;
  else {
    live.add(handle);
    handles.set(SLOT_KEY, handle);
  }
}

function prepareModule(id: SessionModuleId): Promise<PreparedSession> {
  return prepareSession(moduleGlue(id), moduleWasm(id), id);
}

function discardPrepared(prepared: PreparedSession): void {
  try {
    prepared.imported.glue.release?.();
  } catch {
    // Init never finished.
  }
  try {
    URL.revokeObjectURL(prepared.imported.blobUrl);
  } catch {
    // Already revoked.
  }
}

async function deactivateModule(id: SessionModuleId): Promise<void> {
  const pending: FormatHandle[] = [];
  const known = active.get(id);
  if (known) pending.push(known);
  active.delete(id);
  if (id === "library" && libraryHandle && !pending.includes(libraryHandle)) pending.push(libraryHandle);
  if (id === "reader-host" && hostHandle && !pending.includes(hostHandle)) pending.push(hostHandle);
  if (isFormatModule(id)) {
    for (const handle of live) {
      if (handle.format === id && !pending.includes(handle)) pending.push(handle);
    }
  }
  if (id === "library") libraryHandle = null;
  if (id === "reader-host") hostHandle = null;
  for (const handle of pending) await releaseHandle(handle);
  if (id === HOST_PDF) {
    await killPdfWorker();
    forgetPdfReader();
    releaseCanvases();
  }
  console.info(`[mem] ${id} deactivated`);
}

function ensureBody(): HTMLElement | null {
  const root = document.documentElement;
  if (!root) return null;
  if (document.body && document.body.isConnected) return document.body;
  try {
    const body = document.createElement("body");
    root.appendChild(body);
    return body;
  } catch {
    return document.body;
  }
}

function pageHasView(): boolean {
  return !!document.body && document.body.childElementCount > 0;
}

function markPage(next: "library" | "reader", open: Handoff | null): void {
  try {
    boot.kind = next;
    boot.open = open;
  } catch (err) {
    console.error("[boot] could not mark the page", err);
  }
}

async function mountFresh(
  id: SessionModuleId,
  first: PreparedSession,
  payload?: string,
): Promise<FormatHandle> {
  let prepared = first;
  let last: unknown;
  for (let attempt = 0; attempt < 2; attempt++) {
    try {
      const handle = await activateSession(prepared, id, payload);
      if ((id === "library" || id === "reader-host") && !pageHasView()) {
        await releaseHandle(handle);
        throw new Error(`${id} mounted without a view`);
      }
      try {
        void document.body?.offsetHeight;
      } catch {
        // A failed reflow must not hide a view that is already mounted.
      }
      return handle;
    } catch (err) {
      last = err;
      console.error(`[boot] ${id} activate failed`, err);
      if (attempt === 1) break;
      prepared = await prepareModule(id);
    }
  }
  throw last instanceof Error ? last : new Error(`could not activate ${id}`);
}

// In-page. A query navigation does not drop the previous wasm in this
// webview, which is why each open/close kept another instance. The leaving
// modules are deactivated — disposed and released — before the arriving one
// is created. The next module is fetched first, so a missing artifact does
// not clear the page.
async function showLibrary(fromHistory: boolean): Promise<void> {
  if (swapping) {
    queued = { kind: "library", fromHistory };
    return;
  }
  if (liveKind === "library" && libraryHandle && live.size === 0 && !hostHandle) return;
  swapping = true;
  let tornDown = false;
  try {
    requireLibraryPlan();
    if (aliveAfter("to-library") !== "library") {
      throw new Error("library switch left the reader alive");
    }
    const prepared = await prepareModule("library");
    // Cancel a book mount that is still in flight, then drop every module
    // the shelf must not share the page with.
    generation += 1;
    sessionStorage.removeItem(HANDOFF_KEY);
    resetBridge();
    for (const id of releaseBefore("library")) {
      await settle(id, () => deactivateModule(id));
    }
    await settle("stale shelf", () => deactivateModule("library"));
    await settle("pdf worker", () => killPdfWorker());
    await settle("pdf facade", () => forgetPdfReader());
    await settle("clear body", () => {
      ensureBody();
      clearBody();
    });
    tornDown = true;
    paintShell();
    markPage("library", null);
    const handle = await mountFresh("library", prepared);
    remember("library", handle);
    liveKind = "library";
    console.info("[mem] library module mounted; reader host released");
  } catch (err) {
    console.error("[boot] shelf switch failed", err);
    if (tornDown) {
      const message = err instanceof Error ? err.message : "Could not return to the library";
      showBootError(message);
    }
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
  let tornDown = false;
  try {
    const plan = dropsFor("to-reader");
    if (!plan.includes("library") || aliveAfter("to-reader") !== "reader-host") {
      throw new Error("reader switch does not drop the library module");
    }
    sessionStorage.setItem(HANDOFF_KEY, JSON.stringify(payload));
    const prepared = await prepareModule("reader-host");
    generation += 1;
    resetBridge();
    for (const id of releaseBefore("reader-host")) {
      await settle(id, () => deactivateModule(id));
    }
    await settle("stale host", () => deactivateModule("reader-host"));
    if (plan.includes("pdf-worker")) await settle("pdf worker", () => killPdfWorker());
    await settle("clear body", () => {
      ensureBody();
      clearBody();
    });
    tornDown = true;
    paintShell();
    markPage("reader", payload);
    try {
      await load(READER_ENGINE);
    } catch (err) {
      console.error("[boot] reader engine failed to load", err);
    }
    const handle = await mountFresh("reader-host", prepared);
    remember("reader-host", handle);
    liveKind = "reader";
    console.info("[mem] reader host mounted; library module released");
  } catch (err) {
    const message = err instanceof Error ? err.message : "Could not open the book";
    console.error("[boot] reader switch failed", err);
    markPage("library", null);
    hostHandle = null;
    liveKind = "library";
    if (tornDown && !libraryHandle) {
      try {
        const again = await prepareModule("library");
        remember("library", await activateSession(again, "library"));
      } catch (restoreErr) {
        console.error("[boot] shelf restore failed", restoreErr);
      }
    }
    if (tornDown || !libraryHandle) showBootError(message);
  } finally {
    swapping = false;
    pumpQueue();
  }
}

function resetBridge(): void {
  // Dead closures from the instance we are about to release. The next mount
  // installs its own. Assigning onto the old slot can hit a sealed field.
  try {
    window.__MAREADER_SLOT = {};
  } catch {
    const slot = window.__MAREADER_SLOT;
    if (slot) {
      try {
        slot.report = undefined;
      } catch {
        // Sealed.
      }
      try {
        slot.command = undefined;
      } catch {
        // Sealed.
      }
    }
  }
  try {
    boot.flush = () => {};
  } catch {
    // The host sealed the callback. pagehide no-ops if that closure is already dead.
  }
}

async function settle(label: string, step: () => Promise<void> | void): Promise<void> {
  try {
    await step();
  } catch (err) {
    console.error(`[boot] ${label} failed`, err);
  }
}

function safeStyle(node: HTMLElement, prop: string, value: string): void {
  try {
    node.style.setProperty(prop, value);
  } catch {
    try {
      const prev = node.getAttribute("style") ?? "";
      node.setAttribute("style", `${prev};${prop}:${value}`);
    } catch {
      // A failed paint must not become a blank window.
    }
  }
}

function paintShell(): void {
  const root = document.documentElement;
  const body = ensureBody();
  if (!root || !body) return;
  // The shelf view is what paints the window. Once it is gone, a transparent
  // document shows the webview's own background — the empty Tauri window.
  // These writes must not throw: the error overlay calls this, and a throw
  // here is how a failed return became a blank page.
  safeStyle(root, "background", "#1c1917");
  safeStyle(body, "background", "#1c1917");
  safeStyle(body, "color", "#fafaf9");
}

function showBootError(message: string): void {
  try {
    paintShell();
    const body = ensureBody();
    if (!body) return;
    const node = document.createElement("div");
    node.setAttribute("role", "alert");
    const css = [
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
    try {
      node.setAttribute("style", css);
    } catch {
      // The text is still readable if the stylesheet has a color.
    }
    node.textContent = message || "Could not open the page";
    body.appendChild(node);
  } catch (err) {
    console.error("[boot] could not show the failure", err);
  }
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
  active.clear();
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
  activate(module, payload) {
    if (!isSessionModuleId(module)) {
      return Promise.reject(new Error(`unknown module ${module}`));
    }
    if (module === "library") return showLibrary(false);
    if (module === "reader-host") {
      const open = this.open ?? parseHandoff(sessionStorage.getItem(HANDOFF_KEY));
      if (!open) return Promise.reject(new Error("reader host needs a book"));
      return showReader(open, false);
    }
    return mountFormatNow(payload ?? "");
  },
  deactivate(module) {
    if (!isSessionModuleId(module)) {
      return Promise.reject(new Error(`unknown module ${module}`));
    }
    return deactivateModule(module);
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
    remember("reader-host", await startSession(moduleGlue("reader-host"), moduleWasm("reader-host"), "reader-host"));
    liveKind = "reader";
    console.info("[mem] reader host mounted; library module not started");
  } else {
    remember("library", await startSession(moduleGlue("library"), moduleWasm("library"), "library"));
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
