// Runs before the Trunk wasm module and never resolves, so that binary never
// evaluates. It has no release(). The shelf is a module in this document.
// Opening a book deactivates that module completely, then activates the
// reader host and one format module in a child frame. Returning disposes
// that frame — host, format, and the pdf.js worker live there, so removing
// it is what makes them unreachable — and activates a new shelf here. The
// URL is not changed: a history update in this webview unloads the document
// and leaves the empty window.
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
// The open book. Null on the shelf. The parent must not keep the frame's
// window, glue, or worker after this is cleared.
let bookFrame: HTMLIFrameElement | null = null;
let returnedFromBook = false;

interface FrameWindow extends Window {
  Blob: typeof Blob;
  URL: typeof URL;
  Function: typeof Function;
  WebAssembly: typeof WebAssembly;
  __MAREADER_UNLISTEN?: Array<() => void>;
  __MAREADER_SILENCE?: () => void;
  __MAREADER_HANDLES?: FormatHandle[];
  __mareaderEngine?: Promise<void>;
  PDFReader?: PdfFacade;
  __TAURI__?: unknown;
  __TAURI_INTERNALS__?: unknown;
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
  const mount = document.getElementById("mareader-root");
  if (mount) return mount.childElementCount > 0;
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

function viewportBox(): { w: number; h: number } {
  const w = window.innerWidth || document.documentElement?.clientWidth || screen.width || 1280;
  const h = window.innerHeight || document.documentElement?.clientHeight || screen.height || 800;
  return { w: Math.max(w, 320), h: Math.max(h, 240) };
}

function sizeRoot(root: HTMLElement): void {
  const box = viewportBox();
  safeStyle(root, "position", "fixed");
  safeStyle(root, "top", "0");
  safeStyle(root, "left", "0");
  safeStyle(root, "width", `${box.w}px`);
  safeStyle(root, "height", `${box.h}px`);
  safeStyle(root, "overflow", "hidden");
  // Paper and ink come from the appearance tokens. A literal fill here is
  // what kept a dark shelf looking light after the module mounted.
  safeStyle(root, "background", "var(--color-paper)");
  safeStyle(root, "color", "var(--color-ink)");
  // Below the shelf. A higher layer here paints over the books: the title
  // bar escapes on its own z-index, and the grid does not.
  safeStyle(root, "z-index", "0");
}

// Percentage height on the body collapses after a large canvas layer leaves.
// The shelf then mounts and paints nothing. This box has a real height.
function ensureMountRoot(doc: Document = document): HTMLElement {
  const existing = doc.getElementById("mareader-root");
  if (existing instanceof HTMLElement) {
    sizeRoot(existing);
    return existing;
  }
  const body = doc.body ?? ensureBody();
  if (!body) throw new Error("document.body is missing");
  const root = doc.createElement("div");
  root.id = "mareader-root";
  sizeRoot(root);
  body.appendChild(root);
  return root;
}


function buryBookFrames(): void {
  bookFrame = null;
  document.querySelectorAll("iframe").forEach((frame) => {
    if (!(frame instanceof HTMLElement)) return;
    safeStyle(frame, "display", "none");
    safeStyle(frame, "visibility", "hidden");
    safeStyle(frame, "pointer-events", "none");
    safeStyle(frame, "z-index", "0");
    try {
      frame.remove();
    } catch {
      // Hidden, so it cannot cover the shelf even if removal is refused.
    }
  });
}

function kickDocumentPaint(): void {
  // display:none on body can stick in this webview and leave the return blank.
  // A zoom nudge rebuilds the layer tree without hiding the document.
  const html = document.documentElement;
  safeStyle(html, "zoom", "1.001");
  try {
    void html.offsetHeight;
  } catch {
    // The reflow is the point; a thrown read still leaves the zoom set.
  }
  try {
    html.style.removeProperty("zoom");
  } catch {
    safeStyle(html, "zoom", "1");
  }
}

function revealShelf(): void {
  const box = viewportBox();
  const root = document.getElementById("mareader-root");
  if (root instanceof HTMLElement) safeStyle(root, "z-index", "0");
  const level = document.getElementById("library-level");
  let surface: HTMLElement | null = level;
  while (
    surface &&
    surface.parentElement &&
    surface.parentElement !== document.body &&
    surface.parentElement !== root
  ) {
    surface = surface.parentElement;
  }
  if (surface instanceof HTMLElement) {
    safeStyle(surface, "position", "absolute");
    safeStyle(surface, "top", "0");
    safeStyle(surface, "left", "0");
    safeStyle(surface, "width", `${box.w}px`);
    safeStyle(surface, "height", `${box.h}px`);
    safeStyle(surface, "z-index", "2");
    safeStyle(surface, "overflow", "auto");
    safeStyle(surface, "box-sizing", "border-box");
    safeStyle(surface, "background", "var(--color-paper)");
    safeStyle(surface, "color", "var(--color-ink)");
  }
  if (level instanceof HTMLElement) {
    safeStyle(level, "display", "block");
    safeStyle(level, "width", "100%");
    safeStyle(level, "min-height", `${Math.max(box.h - 48, 200)}px`);
    safeStyle(level, "height", "auto");
    safeStyle(level, "overflow", "visible");
  }
  document.querySelectorAll(".lib-grid").forEach((node) => {
    if (!(node instanceof HTMLElement)) return;
    const token = node.style.getPropertyValue("--lib-cols").trim();
    const cols = /^[0-9]+$/.test(token) || token === "auto-fill" || token === "auto-fit" ? token : "auto-fill";
    safeStyle(node, "display", "grid");
    safeStyle(node, "width", "100%");
    safeStyle(node, "grid-template-columns", `repeat(${cols}, minmax(9.5rem, 1fr))`);
  });
}


function applyReturnLayout(): void {
  // The book frame is position:fixed at z-index 2, the same layer as the shelf.
  // After it is removed this webview keeps that layer and the new shelf, still
  // fixed, paints as an empty window. Put the page back in flow and above it.
  buryBookFrames();
  const box = viewportBox();
  const html = document.documentElement;
  const body = document.body;
  if (html) {
    safeStyle(html, "height", `${box.h}px`);
    safeStyle(html, "min-height", `${box.h}px`);
  }
  if (body) {
    safeStyle(body, "position", "relative");
    safeStyle(body, "display", "block");
    safeStyle(body, "width", `${box.w}px`);
    safeStyle(body, "height", `${box.h}px`);
    safeStyle(body, "min-height", `${box.h}px`);
    safeStyle(body, "overflow", "hidden");
    safeStyle(body, "background", "var(--color-paper)");
    safeStyle(body, "color", "var(--color-ink)");
  }
  const root = document.getElementById("mareader-root");
  if (root instanceof HTMLElement) {
    safeStyle(root, "position", "relative");
    safeStyle(root, "top", "auto");
    safeStyle(root, "left", "auto");
    safeStyle(root, "z-index", "3");
    safeStyle(root, "width", `${box.w}px`);
    safeStyle(root, "height", `${box.h}px`);
    safeStyle(root, "min-height", `${box.h}px`);
    safeStyle(root, "overflow", "auto");
    safeStyle(root, "background", "var(--color-paper)");
    safeStyle(root, "color", "var(--color-ink)");
  }
  revealShelf();
  if (root instanceof HTMLElement) {
    // revealShelf puts the fill back under the shelf. On return the fill is
    // the page, and it has to stay above a book frame that failed to detach.
    safeStyle(root, "position", "relative");
    safeStyle(root, "z-index", "3");
  }
  const level = document.getElementById("library-level");
  let surface: HTMLElement | null = level;
  while (
    surface &&
    surface.parentElement &&
    surface.parentElement !== document.body &&
    surface.parentElement !== root
  ) {
    surface = surface.parentElement;
  }
  if (!(surface instanceof HTMLElement) && root instanceof HTMLElement) {
    for (const child of Array.from(root.children)) {
      if (child instanceof HTMLElement && !child.classList.contains("noise-overlay")) {
        surface = child;
        break;
      }
    }
  }
  if (surface instanceof HTMLElement) {
    safeStyle(surface, "position", "relative");
    safeStyle(surface, "top", "auto");
    safeStyle(surface, "left", "auto");
    safeStyle(surface, "width", `${box.w}px`);
    safeStyle(surface, "height", `${box.h}px`);
    safeStyle(surface, "min-height", `${box.h}px`);
    safeStyle(surface, "z-index", "3");
    safeStyle(surface, "overflow", "auto");
  }
}

function paintReturn(): void {
  applyReturnLayout();
  kickDocumentPaint();
}

function fillMounted(root: HTMLElement): void {
  for (const child of Array.from(root.children)) {
    if (!(child instanceof HTMLElement)) continue;
    if (child.classList.contains("noise-overlay")) continue;
    // The title band is absolute and only a few pixels tall. Stretching it
    // covers the shelf and eats every click.
    if (child.classList.contains("absolute") || child.classList.contains("fixed")) continue;
    safeStyle(child, "min-height", "100%");
    safeStyle(child, "height", "100%");
    safeStyle(child, "box-sizing", "border-box");
  }
}

function forceRepaint(node: HTMLElement): void {
  safeStyle(node, "transform", "translateZ(0)");
  try {
    void node.offsetHeight;
  } catch {
    // A failed reflow must not hide a view that is already mounted.
  }
  requestAnimationFrame(() => {
    try {
      node.style.removeProperty("transform");
    } catch {
      // The layer can keep the hint.
    }
    try {
      void node.offsetHeight;
    } catch {
      // Already painted.
    }
  });
}

function resetChrome(): void {
  const root = document.documentElement;
  const body = document.body;
  const hiding = ["filter", "opacity", "visibility", "content-visibility", "transform", "zoom"];
  if (root) {
    try {
      root.classList.remove("appearance-scrubbing", "theme-switching");
    } catch {
      // classList can be sealed after a reader session.
    }
    safeStyle(root, "height", "100%");
    for (const prop of hiding) {
      try {
        root.style.removeProperty(prop);
      } catch {
        // Leave the declaration.
      }
    }
  }
  if (body) {
    safeStyle(body, "height", "100%");
    safeStyle(body, "margin", "0");
    safeStyle(body, "overflow", "hidden");
    for (const prop of [...hiding, "display"]) {
      try {
        body.style.removeProperty(prop);
      } catch {
        // Leave the declaration.
      }
    }
  }
}

function libraryVisible(): boolean {
  const root = document.getElementById("mareader-root");
  if (!(root instanceof HTMLElement) || root.childElementCount === 0) return false;
  const box = root.getBoundingClientRect();
  return box.width > 40 && box.height > 40;
}

interface FetchedModule {
  source: string;
  bytes: Uint8Array;
}

async function fetchModule(id: SessionModuleId): Promise<FetchedModule> {
  const glue = await fetchAsset(moduleGlue(id), id);
  const wasm = await fetchAsset(moduleWasm(id), id);
  if (!isWasm(wasm.bytes)) throw new Error(`${id} is not wasm`);
  return { source: new TextDecoder().decode(glue.bytes), bytes: wasm.bytes };
}

function frameWindow(frame: HTMLIFrameElement | null): FrameWindow | null {
  const win = frame?.contentWindow as FrameWindow | null;
  return win ?? null;
}

function frameHandles(win: FrameWindow): FormatHandle[] {
  if (!win.__MAREADER_HANDLES) win.__MAREADER_HANDLES = [];
  return win.__MAREADER_HANDLES;
}

function rememberFrame(win: FrameWindow, handle: FormatHandle): void {
  frameHandles(win).push(handle);
}

async function releaseFrameFormats(win: FrameWindow): Promise<void> {
  const handles = frameHandles(win);
  const formats = handles.filter((handle) => handle.format !== "reader-host" && handle.format !== "library");
  for (const handle of formats) await releaseHandle(handle);
  win.__MAREADER_HANDLES = handles.filter(
    (handle) => handle.format === "reader-host" || handle.format === "library",
  );
}

function installSilence(win: FrameWindow): void {
  const unlistens: Array<() => void> = [];
  win.__MAREADER_UNLISTEN = unlistens;
  win.__MAREADER_SILENCE = () => {
    for (const fn of unlistens.splice(0)) {
      try {
        fn();
      } catch {
        // Already removed.
      }
    }
  };
}

function shareHostGlobals(win: FrameWindow): void {
  const parent = window as FrameWindow;
  if (parent.__TAURI__) win.__TAURI__ = parent.__TAURI__;
  if (parent.__TAURI_INTERNALS__) win.__TAURI_INTERNALS__ = parent.__TAURI_INTERNALS__;
}

function copyStyles(idoc: Document): void {
  const base = idoc.createElement("base");
  base.href = document.baseURI;
  idoc.head.appendChild(base);
  for (const node of document.querySelectorAll('link[rel="stylesheet"], style')) {
    if (node instanceof HTMLLinkElement) {
      const link = idoc.createElement("link");
      link.rel = "stylesheet";
      link.href = node.href;
      idoc.head.appendChild(link);
    } else if (node instanceof HTMLStyleElement) {
      const style = idoc.createElement("style");
      style.textContent = node.textContent;
      idoc.head.appendChild(style);
    }
  }
  try {
    idoc.documentElement.className = document.documentElement.className;
  } catch {
    // The frame starts unthemed; the host paints its own.
  }
}

async function importIn(
  win: FrameWindow,
  source: string,
  label: string,
): Promise<{ glue: FormatGlue; blobUrl: string }> {
  const instance = ++blobNonce;
  const named = `${source}\n// mareader-instance ${instance}\n//# sourceURL=mareader-${label}-${instance}.mjs\n`;
  const blob = new win.Blob([named], { type: "text/javascript" });
  const blobUrl = win.URL.createObjectURL(blob);
  const importer = win.Function("u", "return import(u)") as (this: unknown, u: string) => Promise<FormatGlue>;
  try {
    const glue = await importer.call(win, blobUrl);
    if (typeof glue.default !== "function" || typeof glue.mount !== "function") {
      throw new Error(`${label} has no mount`);
    }
    if (typeof glue.release !== "function") {
      throw new Error(`${label} has no release()`);
    }
    return { glue, blobUrl };
  } catch (err) {
    try {
      win.URL.revokeObjectURL(blobUrl);
    } catch {
      // Already revoked.
    }
    throw err;
  }
}

async function startIn(win: FrameWindow, glue: FormatGlue, bytes: Uint8Array): Promise<void> {
  const copy = new Uint8Array(bytes.byteLength);
  copy.set(bytes);
  const compiled = await win.WebAssembly.compile(copy);
  await startGlue(glue, compiled);
}

async function loadScriptsIn(win: FrameWindow, urls: string[]): Promise<void> {
  const importer = win.Function("u", "return import(u)") as (this: unknown, u: string) => Promise<unknown>;
  for (const url of urls) await importer.call(win, assetUrl(url));
}

async function loadEngineIn(win: FrameWindow): Promise<void> {
  if (win.PDFReader) return;
  if (!win.__mareaderEngine) {
    win.__mareaderEngine = loadScriptsIn(win, [PDFJS, PDF_ENGINE]);
  }
  await win.__mareaderEngine;
  const pdfjs = (win as unknown as { pdfjsLib?: { GlobalWorkerOptions?: { workerSrc: string } } }).pdfjsLib;
  if (pdfjs?.GlobalWorkerOptions) {
    pdfjs.GlobalWorkerOptions.workerSrc = assetUrl("/vendor/pdfjs/pdf.worker.min.mjs");
  }
}

function installFrameBoot(win: FrameWindow, payload: Handoff): void {
  const frameBoot: Boot = {
    kind: "reader",
    open: payload,
    takeOpen() {
      const value = this.open;
      this.open = null;
      return value;
    },
    enterReader(next) {
      if (!next || typeof next.path !== "string" || next.path.length === 0) return;
      setTimeout(() => {
        void showReader(next, false);
      }, 0);
    },
    enterLibrary() {
      // The caller is inside this frame. Destroying it on this stack aborts
      // the call before the shelf can mount.
      setTimeout(() => {
        void showLibrary(false);
      }, 0);
    },
    clearHandoff() {
      this.open = null;
      sessionStorage.removeItem(HANDOFF_KEY);
    },
    ensureEngine() {
      return loadEngineIn(win);
    },
    mountFormat(body) {
      return mountFormatIn(win, body);
    },
    dropFormat() {
      return releaseFrameFormats(win);
    },
    activate(module, body) {
      if (!isSessionModuleId(module)) return Promise.reject(new Error(`unknown module ${module}`));
      if (module === "library") {
        setTimeout(() => {
          void showLibrary(false);
        }, 0);
        return Promise.resolve();
      }
      if (isFormatModule(module)) return mountFormatIn(win, body ?? "");
      return Promise.reject(new Error(`unknown module ${module}`));
    },
    deactivate(module) {
      if (!isSessionModuleId(module)) return Promise.reject(new Error(`unknown module ${module}`));
      if (isFormatModule(module)) return releaseFrameFormats(win);
      if (module === "reader-host") return destroyBookFrame();
      return Promise.resolve();
    },
    flush() {},
  };
  win.__MAREADER_BOOT = frameBoot;
  win.__MAREADER_SLOT = {};
}

async function mountFormatIn(win: FrameWindow, payload: string): Promise<void> {
  let path = "";
  try {
    const parsed = JSON.parse(payload) as { path?: unknown };
    path = typeof parsed.path === "string" ? parsed.path : "";
    if (path.length === 0) return;
    const format = artifactFor(path);
    const mine = ++generation;
    const fetched = await fetchModule(format);
    if (mine !== generation) return;
    if (format === HOST_PDF) await loadEngineIn(win);
    if (mine !== generation) return;
    await releaseFrameFormats(win);
    if (mine !== generation) return;
    const imported = await importIn(win, fetched.source, format);
    const handle: FormatHandle = { glue: imported.glue, blobUrl: imported.blobUrl, format };
    try {
      await startIn(win, imported.glue, fetched.bytes);
      await imported.glue.mount?.(payload);
    } catch (err) {
      try {
        if (typeof imported.glue.dispose === "function") await imported.glue.dispose();
        else imported.glue.detach?.();
      } catch {
        // Init never finished.
      }
      try {
        imported.glue.release?.();
      } catch {
        // Binding already clear.
      }
      try {
        win.URL.revokeObjectURL(imported.blobUrl);
      } catch {
        // Already revoked.
      }
      throw err;
    }
    if (mine !== generation) {
      await releaseHandle(handle);
      return;
    }
    rememberFrame(win, handle);
    console.info(`[mem] ${format} module mounted in the book frame`);
  } catch (err) {
    const message = err instanceof Error ? err.message : "Could not load the format module";
    const report = win.__MAREADER_SLOT?.report;
    console.error("[format]", message);
    if (typeof report === "function" && path.length > 0) {
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
  }
}

async function createBookFrame(payload: Handoff, host: FetchedModule): Promise<HTMLIFrameElement> {
  const frame = document.createElement("iframe");
  frame.setAttribute("title", "Book");
  const box = viewportBox();
  safeStyle(frame, "position", "fixed");
  safeStyle(frame, "top", "0");
  safeStyle(frame, "left", "0");
  safeStyle(frame, "width", `${box.w}px`);
  safeStyle(frame, "height", `${box.h}px`);
  safeStyle(frame, "border", "0");
  safeStyle(frame, "background", "#1c1917");
  safeStyle(frame, "z-index", "2");
  const body = ensureBody();
  if (!body) throw new Error("document.body is missing");
  body.appendChild(frame);
  // WKWebView creates the child document on the next frame, not in the
  // append itself. Writing before that throws and the shelf comes back.
  await new Promise<void>((resolve) => {
    requestAnimationFrame(() => resolve());
  });
  try {
    let idoc = frame.contentDocument;
    let win = frame.contentWindow as FrameWindow | null;
    if (!idoc || !win) throw new Error("book frame is not same-origin");
    idoc.open();
    idoc.write("<!doctype html><html><head></head><body></body></html>");
    idoc.close();
    idoc = frame.contentDocument;
    win = frame.contentWindow as FrameWindow | null;
    if (!idoc || !win) throw new Error("book frame lost its document");
    copyStyles(idoc);
    shareHostGlobals(win);
    installSilence(win);
    installFrameBoot(win, payload);
    ensureMountRoot(idoc);
    try {
      await loadScriptsIn(win, [READER_ENGINE]);
    } catch (err) {
      console.error("[boot] reader engine failed to load", err);
    }
    const imported = await importIn(win, host.source, "reader-host");
    const handle: FormatHandle = {
      glue: imported.glue,
      blobUrl: imported.blobUrl,
      format: "reader-host",
    };
    try {
      await startIn(win, imported.glue, host.bytes);
      await imported.glue.mount?.();
    } catch (err) {
      try {
        if (typeof imported.glue.dispose === "function") await imported.glue.dispose();
        else imported.glue.detach?.();
      } catch {
        // Init never finished.
      }
      try {
        imported.glue.release?.();
      } catch {
        // Binding already clear.
      }
      throw err;
    }
    rememberFrame(win, handle);
    const inner = idoc.getElementById("mareader-root");
    if (inner instanceof HTMLElement) fillMounted(inner);
    return frame;
  } catch (err) {
    try {
      frame.remove();
    } catch {
      // Already gone.
    }
    throw err;
  }
}

async function destroyBookFrame(): Promise<void> {
  const frame = bookFrame;
  bookFrame = null;
  if (!frame) return;
  const win = frameWindow(frame);
  try {
    win?.__MAREADER_BOOT?.flush?.();
  } catch {
    // The host already flushed before asking to leave.
  }
  try {
    win?.__MAREADER_SILENCE?.();
  } catch {
    // No listeners parked.
  }
  const handles = win?.__MAREADER_HANDLES ? [...win.__MAREADER_HANDLES] : [];
  if (win) win.__MAREADER_HANDLES = [];
  for (const handle of handles) {
    try {
      if (typeof handle.glue.dispose === "function") await handle.glue.dispose();
      else handle.glue.detach?.();
    } catch {
      // The view is already gone.
    }
    try {
      handle.glue.release?.();
    } catch {
      // Binding already clear.
    }
    try {
      URL.revokeObjectURL(handle.blobUrl);
    } catch {
      // Already revoked.
    }
  }
  try {
    const pdf = win?.PDFReader;
    if (pdf && typeof pdf.destroy === "function") {
      await Promise.race([
        pdf.destroy(),
        new Promise((resolve) => {
          setTimeout(resolve, 1200);
        }),
      ]);
    }
  } catch {
    // The frame removal ends the worker if destroy did not.
  }
  try {
    frame.remove();
  } catch {
    // Already gone.
  }
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
  if (liveKind === "library" && libraryHandle && !bookFrame && live.size === 0 && !hostHandle && libraryVisible()) {
    return;
  }
  swapping = true;
  let tornDown = false;
  let prepared: PreparedSession | null = null;
  try {
    requireLibraryPlan();
    if (aliveAfter("to-library") !== "library") {
      throw new Error("library switch left the reader alive");
    }
    prepared = await prepareModule("library");
    generation += 1;
    sessionStorage.removeItem(HANDOFF_KEY);
    resetBridge();
    // The book frame still holds the host, the format, and the worker. Drop
    // it before the new shelf instance exists. The two must not be alive
    // together.
    await destroyBookFrame();
    buryBookFrames();
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
    resetChrome();
    paintShell();
    const root = ensureMountRoot();
    markPage("library", null);
    const handed = prepared;
    prepared = null;
    const handle = await mountFresh("library", handed);
    remember("library", handle);
    liveKind = "library";
    fillMounted(root);
    returnedFromBook = true;
    paintReturn();
    requestAnimationFrame(() => applyReturnLayout());
    setTimeout(() => applyReturnLayout(), 50);
    setTimeout(() => applyReturnLayout(), 250);
    console.info("[mem] library module mounted; book frame released");
  } catch (err) {
    if (prepared) discardPrepared(prepared);
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
    const fetched = await fetchModule("reader-host");
    generation += 1;
    resetBridge();
    for (const id of releaseBefore("reader-host")) {
      await settle(id, () => deactivateModule(id));
    }
    await settle("stale host", () => deactivateModule("reader-host"));
    if (plan.includes("pdf-worker")) await settle("pdf worker", () => killPdfWorker());
    await destroyBookFrame();
    await settle("clear body", () => {
      ensureBody();
      clearBody();
    });
    tornDown = true;
    paintShell();
    markPage("reader", payload);
    bookFrame = await createBookFrame(payload, fetched);
    hostHandle = null;
    liveKind = "reader";
    returnedFromBook = false;
    console.info("[mem] book frame mounted; library module released");
  } catch (err) {
    const message = err instanceof Error ? err.message : "Could not open the book";
    console.error("[boot] reader switch failed", err);
    await destroyBookFrame();
    markPage("library", null);
    hostHandle = null;
    liveKind = "library";
    if (tornDown && !libraryHandle) {
      try {
        resetChrome();
        ensureMountRoot();
        const again = await prepareModule("library");
        const handle = await mountFresh("library", again);
        remember("library", handle);
        const root = document.getElementById("mareader-root");
        if (root instanceof HTMLElement) {
          fillMounted(root);
          revealShelf();
          forceRepaint(root);
        }
      } catch (restoreErr) {
        console.error("[boot] shelf restore failed", restoreErr);
      }
    }
    if (!libraryHandle) showBootError(message);
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
  safeStyle(root, "background", "var(--color-paper)");
  safeStyle(body, "background", "var(--color-paper)");
  safeStyle(body, "color", "var(--color-ink)");
}

function showBootError(message: string): void {
  try {
    paintShell();
    resetChrome();
    const root = ensureMountRoot();
    root.replaceChildren();
    const node = document.createElement("div");
    node.setAttribute("role", "alert");
    safeStyle(node, "position", "fixed");
    safeStyle(node, "inset", "0");
    safeStyle(node, "z-index", "2147483647");
    safeStyle(node, "box-sizing", "border-box");
    safeStyle(node, "background", "#1c1917");
    safeStyle(node, "color", "#fafaf9");
    safeStyle(node, "font", "16px/1.45 ui-sans-serif, system-ui, sans-serif");
    safeStyle(node, "padding", "28px");
    node.textContent = message || "Could not open the page";
    root.appendChild(node);
    forceRepaint(root);
  } catch (err) {
    console.error("[boot] could not show the failure", err);
  }
}

// pagehide cannot await. detach and release are sync. destroy() terminates
// the pdf.js worker when it is called, before its promise settles.
function abandonNow(): void {
  generation += 1;
  const frame = bookFrame;
  bookFrame = null;
  if (frame) {
    try {
      frame.remove();
    } catch {
      // The document is leaving.
    }
  }
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
    const win = frameWindow(bookFrame);
    if (win?.__MAREADER_BOOT && win.__MAREADER_BOOT !== boot) {
      return win.__MAREADER_BOOT.mountFormat(payload);
    }
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
    const win = frameWindow(bookFrame);
    if (win?.__MAREADER_BOOT && win.__MAREADER_BOOT !== boot && isFormatModule(module)) {
      return win.__MAREADER_BOOT.mountFormat(payload ?? "");
    }
    return mountFormatNow(payload ?? "");
  },
  deactivate(module) {
    if (!isSessionModuleId(module)) {
      return Promise.reject(new Error(`unknown module ${module}`));
    }
    const win = frameWindow(bookFrame);
    if (win && isFormatModule(module)) return releaseFrameFormats(win);
    if (module === "reader-host" && bookFrame) return destroyBookFrame();
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

window.addEventListener("resize", () => {
  if (!bookFrame && returnedFromBook) {
    applyReturnLayout();
    return;
  }
  const root = document.getElementById("mareader-root");
  if (root instanceof HTMLElement) sizeRoot(root);
  revealShelf();
  if (!bookFrame) return;
  const box = viewportBox();
  safeStyle(bookFrame, "width", `${box.w}px`);
  safeStyle(bookFrame, "height", `${box.h}px`);
  const inner = bookFrame.contentDocument?.getElementById("mareader-root");
  if (inner instanceof HTMLElement) sizeRoot(inner);
});

try {
  if (kind === "reader") {
    const open = boot.open ?? parseHandoff(sessionStorage.getItem(HANDOFF_KEY));
    if (open) {
      await showReader(open, false);
    } else {
      const root = ensureMountRoot();
      remember("library", await startSession(moduleGlue("library"), moduleWasm("library"), "library"));
      liveKind = "library";
      fillMounted(root);
      revealShelf();
      forceRepaint(root);
      requestAnimationFrame(() => revealShelf());
      console.info("[mem] library module mounted; reader host not started");
    }
  } else {
    const root = ensureMountRoot();
    remember("library", await startSession(moduleGlue("library"), moduleWasm("library"), "library"));
    liveKind = "library";
    fillMounted(root);
    revealShelf();
    forceRepaint(root);
    requestAnimationFrame(() => revealShelf());
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
