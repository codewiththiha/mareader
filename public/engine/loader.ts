// Document loading: local files via Tauri IPC, web samples via fetch.

import type {
  CoverResult,
  LoadingTask,
  OpenResult,
  OutlineItem,
  PDFDocumentProxy,
} from "./types";
import { offscreenFor, releaseCanvas } from "./canvas";
import { errorInfo, fail, failFrom } from "./errors";
import { resetPaperForDocument } from "./paper";
import { beginThumbLane } from "./thumbnails";
import { lifecycleEvent, noteWorkerCreated } from "./state";
import type { EngineSession } from "./state";

type PdfjsLib = {
  getDocument: (params: Record<string, unknown>) => {
    promise: Promise<PDFDocumentProxy>;
    destroy: () => Promise<void>;
  };
  GlobalWorkerOptions: { workerSrc: string };
  TextLayer: unknown;
};

type TextLayerCtor = {
  new (opts: {
    textContentSource: { items: unknown[] };
    container: HTMLElement;
    viewport: { width: number; height: number };
  }): { render: () => Promise<void>; cancel: () => void };
};

const WEB_PATH_RE = /^(https?:\/\/|blob:)/i;

function isWebServedPath(path: string): boolean {
  if (WEB_PATH_RE.test(path)) return true;
  if (path.startsWith("/samples/") || path.startsWith("samples/")) return true;
  return false;
}

function withTimeout<T>(
  p: Promise<T>,
  ms: number,
  message: string,
  onTimeout?: () => void,
): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const t = setTimeout(() => {
      try {
        onTimeout?.();
      } catch (_) {
        /* best-effort abort */
      }
      reject(Object.assign(new Error(message), { name: "TimeoutError" }));
    }, ms);
    p.then(
      (v) => {
        clearTimeout(t);
        resolve(v);
      },
      (e) => {
        clearTimeout(t);
        reject(e);
      },
    );
  });
}

// The worker URL is resolved once; the src never changes.
let workerSrcConfigured = false;

// Where pdf.js lives; a runtime `import()`, so the specifier stays a
// `string`.
const PDFJS_MODULE_URL: string = "/vendor/pdfjs/pdf.min.mjs";

/** The one in-flight load of pdf.js, so concurrent first opens share it. */
let pdfjsLoading: Promise<PdfjsLib> | null = null;

// pdf.js, if something already put it on `globalThis`.
function presentPdfjs(): PdfjsLib | null {
  const l = globalThis.pdfjsLib as PdfjsLib | undefined;
  return l && typeof l.getDocument === "function" ? l : null;
}

function configureWorker(l: PdfjsLib): PdfjsLib {
  // Absolute worker URL so Tauri's Worker resolves against the webview origin.
  if (l.GlobalWorkerOptions && !workerSrcConfigured) {
    workerSrcConfigured = true;
    try {
      l.GlobalWorkerOptions.workerSrc = new URL(
        "/vendor/pdfjs/pdf.worker.min.mjs",
        globalThis.location?.href || "http://localhost/",
      ).href;
    } catch (_) {
      l.GlobalWorkerOptions.workerSrc = "/vendor/pdfjs/pdf.worker.min.mjs";
    }
  }
  return l;
}

// pdf.js, loaded on first use; a failed load is not cached.
function ensurePdfjs(): Promise<PdfjsLib> {
  const present = presentPdfjs();
  if (present) return Promise.resolve(configureWorker(present));
  if (!pdfjsLoading) {
    pdfjsLoading = import(PDFJS_MODULE_URL)
      .then((mod: unknown) => {
        // pdf.js publishes itself on `globalThis.pdfjsLib`; the namespace is a
        // fallback.
        const lib = presentPdfjs() ?? (mod as PdfjsLib);
        if (!lib || typeof lib.getDocument !== "function") {
          throw new Error("pdf.js is not loaded");
        }
        globalThis.pdfjsLib = lib;
        return configureWorker(lib);
      })
      .catch((e: unknown) => {
        pdfjsLoading = null;
        throw e;
      });
  }
  return pdfjsLoading;
}

// Resolve pdf.js off globalThis at call time, never at evaluate.
function getPdfjs(): PdfjsLib {
  const l = presentPdfjs();
  if (!l) {
    throw new Error("pdf.js is not loaded");
  }
  return configureWorker(l);
}

export async function getDocument(params: Record<string, unknown>) {
  return (await ensurePdfjs()).getDocument(params);
}

// A LoadingTask is destroyed at most once; the WeakSet keeps both paths
// idempotent.
const destroyedTasks = new WeakSet<LoadingTask>();

export async function destroyTask(
  s: EngineSession,
  task: LoadingTask | null | undefined
): Promise<void> {
  if (!task) return;
  if (destroyedTasks.has(task)) return;
  destroyedTasks.add(task);
  // Only THE task registered on the session is detached.
  if (s.loadingTask === task) {
    s.setLoadingTask(null);
  }
  try {
    await task.destroy();
  } catch (_) {
    /* best-effort teardown */
  } finally {
    // Counted after the shutdown resolves, so "terminated" is true.
    s.workersTerminated += 1;
    lifecycleEvent("pdf_worker:terminate");
  }
}

export function TextLayer(opts: ConstructorParameters<TextLayerCtor>[0]) {
  const Ctor = getPdfjs().TextLayer as TextLayerCtor;
  return new Ctor(opts);
}

// The open parameters: the CSP pair, the memory pair, the c-map pair.
const BASE_PARAMS = {
  cMapUrl: "/vendor/pdfjs/cmaps/",
  cMapPacked: true,
  disableAutoFetch: true,
  disableStream: true,
  isEvalSupported: false,
};

// The worker's deadline for handing back a document.
const OPEN_TIMEOUT_MS = 8000;
const OPEN_TIMEOUT_MSG = "Timed out opening this PDF (pdf.js worker failed to initialize)";

// The whole of "open a document": one task, one ceiling, one cleanup.
async function openTask(
  s: EngineSession,
  source: Record<string, unknown>
): Promise<PDFDocumentProxy> {
  const task = await getDocument({ ...BASE_PARAMS, ...source });
  noteWorkerCreated(s);
  // The session may have died while pdf.js loaded: kill the worker here.
  if (s.disposed) {
    await destroyTask(s, task);
    throw Object.assign(new Error("Session destroyed during open"), { name: "SessionGone" });
  }
  s.setLoadingTask(task);
  return await withTimeout(task.promise, OPEN_TIMEOUT_MS, OPEN_TIMEOUT_MSG, () => {
    void destroyTask(s, task);
  });
}

async function doFetch(src: string): Promise<Uint8Array> {
  const res = await fetch(src);
  if (!res.ok) {
    throw Object.assign(new Error("HTTP " + res.status), {
      name: "UnexpectedResponseException",
    });
  }
  return new Uint8Array(await res.arrayBuffer());
}

function toUint8(bytes: unknown): Uint8Array {
  if (bytes instanceof Uint8Array) return bytes;
  if (bytes instanceof ArrayBuffer) return new Uint8Array(bytes);
  if (ArrayBuffer.isView(bytes)) {
    const v = bytes as ArrayBufferView;
    return new Uint8Array(v.buffer, v.byteOffset, v.byteLength);
  }
  // The invoke may have run on the parent window; ask the object tag.
  if (Object.prototype.toString.call(bytes) === "[object ArrayBuffer]") {
    return new Uint8Array(bytes as ArrayBuffer);
  }
  if (Array.isArray(bytes)) return Uint8Array.from(bytes as number[]);
  throw Object.assign(new Error("read_file_bytes returned an unexpected type"), {
    name: "UnexpectedResponseException",
  });
}

// Fetch a document's bytes, via fetch or Tauri's `read_file_bytes`.
async function fetchBytes(path: string): Promise<Uint8Array> {
  if (isWebServedPath(path)) {
    const url = path.startsWith("samples/") ? "/" + path : path;
    return doFetch(url);
  }
  const tauri = globalThis.__TAURI__;
  if (tauri && tauri.core && typeof tauri.core.invoke === "function") {
    const bytes = await tauri.core.invoke("read_file_bytes", { path });
    return toUint8(bytes);
  }
  // A filesystem path over plain fetch is not a URL.
  throw Object.assign(
    new Error(
      "no Tauri IPC in this frame — cannot read " +
        path +
        " (tauri-relay.js did not publish the parent's API here)",
    ),
    { name: "IpcUnavailableError" },
  );
}

// Tauri `convertFileSrc` URL only, never a raw path.
function localAssetUrl(path: string): string | null {
  const tauri = globalThis.__TAURI__;
  if (!tauri || !tauri.core || typeof tauri.core.convertFileSrc !== "function") {
    return null;
  }
  let url = "";
  try {
    url = String(tauri.core.convertFileSrc(path) || "");
  } catch (_) {
    return null;
  }
  if (!url) return null;
  if (url.startsWith("/") || /^[A-Za-z]:[\\/]/.test(url)) return null;
  if (!/^(asset:|https?:|tauri:|http:\/\/asset\.localhost|https:\/\/asset\.localhost)/i.test(url)) {
    return null;
  }
  return url;
}

async function probeAssetUrl(url: string): Promise<boolean> {
  if (typeof fetch !== "function") return false;
  try {
    const ctrl = new AbortController();
    const t = setTimeout(() => ctrl.abort(), 1200);
    const res = await fetch(url, {
      method: "GET",
      headers: { Range: "bytes=0-1" },
      signal: ctrl.signal,
    });
    clearTimeout(t);
    return res.ok || res.status === 206;
  } catch (_) {
    return false;
  }
}

/** A URL pdf.js can Range-request, so the file is never held whole in V8. */
async function openFromUrl(s: EngineSession, url: string): Promise<PDFDocumentProxy> {
  return openTask(s, { url, disableRange: false, rangeChunkSize: 65536 });
}

// Bytes already in memory: nothing to range over.
async function openFromBytes(s: EngineSession, bytes: Uint8Array): Promise<PDFDocumentProxy> {
  return openTask(s, { data: bytes });
}

async function openDocument(s: EngineSession, path: string): Promise<PDFDocumentProxy> {
  if (isWebServedPath(path)) {
    const url = path.startsWith("samples/") ? "/" + path : path;
    return await openFromUrl(s, url);
  }

  // Prefer the asset protocol; the Windows form fails — probe and fall
  // back to IPC.
  const asset = localAssetUrl(path);
  if (asset && (await probeAssetUrl(asset))) {
    try {
      return await openFromUrl(s, asset);
    } catch (_) {
      /* bytes fallback */
    }
  }

  return await openFromBytes(s, await fetchBytes(path));
}

// Open `path` into `s`; a session holds exactly one document.
export async function open(s: EngineSession, path: string): Promise<OpenResult> {
  try {
    const doc = await openDocument(s, path);
    // Destroyed while the worker produced the document: do not land on a
    // retired session.
    if (s.disposed) {
      await destroyTask(s, s.loadingTask);
      return fail("no_session", "Session destroyed during open");
    }
    s.setPdf(doc);
    s.setNumPages(doc.numPages);
    // The probed page boxes belong to the document that just died.
    s.clearIntrinsicSizes();
    // The new document's thumbnail lane is open.
    beginThumbLane(s);
    s.setCurrentPath(path);
    // Count a session only once the document proxy is in place.
    s.sessionsOpened += 1;
    lifecycleEvent("pdf_session:create");
    // A fresh paper budget, and the cache's colours when it knows this book.
    resetPaperForDocument(s);

    // Metadata and page 1 are independent round trips; page 1 is the open.
    const [meta, page1] = await Promise.all([
      doc.getMetadata().catch(() => null),
      doc.getPage(1),
    ]);
    const title: string | null = (meta && meta.info && meta.info.Title) || null;
    const author: string | null = (meta && meta.info && meta.info.Author) || null;

    const vp = page1.getViewport({ scale: 1 });
    try { page1.cleanup(); } catch (_) { /* ignore */ }

    // Seed every page with page 1's size so open returns immediately.
    const pageHeights: number[] = new Array(s.numPages);
    const pageWidths: number[] = new Array(s.numPages);
    for (let i = 0; i < s.numPages; i += 1) {
      pageHeights[i] = vp.height;
      pageWidths[i] = vp.width;
    }

    return {
      ok: true,
      numPages: s.numPages,
      title,
      author,
      // The content fingerprint the search index caches under.
      fingerprint: doc.fingerprints?.[0] ?? null,
      // The outline is deliberately NOT resolved here — see resolveOutline.
      outline: [],
      page1Size: { width: vp.width, height: vp.height },
      pageHeights,
      pageWidths,
    };
  } catch (e) {
    const er = e as { name?: string };
    if (er && er.name === "SessionGone") {
      return fail("no_session", "Session destroyed during open");
    }
    if (er && er.name === "PasswordException") {
      return fail("encrypted", "This PDF is password-protected.");
    }
    if (
      er &&
      (er.name === "InvalidPDFException" ||
        er.name === "MissingPDFException" ||
        er.name === "UnexpectedResponseException" ||
        er.name === "TimeoutError")
    ) {
      const d = errorInfo(e);
      return fail("corrupt", `Could not read this PDF. (${d.name}: ${d.message})`);
    }
    return failFrom(e);
  }
}

function outlineTitle(raw: string | null | undefined): string {
  return String(raw == null ? "" : raw).trim() || "(untitled)";
}

// Run `fn` over `items` with at most `limit` in flight, keeping order.
async function mapWithLimit<T, R>(
  items: T[],
  limit: number,
  fn: (item: T, index: number) => Promise<R>,
): Promise<R[]> {
  const out: R[] = new Array<R>(items.length);
  let next = 0;
  const workers = Array.from(
    { length: Math.min(Math.max(limit, 1), Math.max(items.length, 1)) },
    async () => {
      while (next < items.length) {
        const i = next;
        next += 1;
        out[i] = await fn(items[i]!, i);
      }
    },
  );
  await Promise.all(workers);
  return out;
}

const OUTLINE_CONCURRENCY = 8;

async function resolveOutlineEntry(
  s: EngineSession,
  it: OutlineItem,
  depth: number,
): Promise<{ title: string; page: number; depth: number } | null> {
  let page: number | null = null;
  try {
    if (Array.isArray(it.dest)) {
      const ref = it.dest[0];
      if (ref && typeof ref === "object" && "num" in ref) {
        const idx = await s.pdf!.getPageIndex(ref);
        page = idx + 1;
      } else if (typeof ref === "number") {
        page = ref + 1;
      }
    } else if (typeof it.dest === "string") {
      const d = await s.pdf!.getDestination(it.dest);
      if (d && d[0]) {
        const ref = d[0];
        if (ref && typeof ref === "object" && "num" in ref) {
          const idx = await s.pdf!.getPageIndex(ref);
          page = idx + 1;
        }
      }
    }
  } catch (_) {
    page = null;
  }
  if (!page) return null;
  return { title: outlineTitle(it.title), page, depth };
}

async function flattenOutline(
  s: EngineSession,
  items: OutlineItem[] | null | undefined,
  depth: number,
): Promise<{ title: string; page: number; depth: number }[]> {
  const siblings = items || [];
  // Siblings resolve concurrently, then concatenate in document order.
  const perSibling = await mapWithLimit(siblings, OUTLINE_CONCURRENCY, async (it) => {
    const entry = await resolveOutlineEntry(s, it, depth);
    const descendants = await flattenOutline(s, it.items, depth + 1);
    return entry ? [entry, ...descendants] : descendants;
  });
  return perSibling.flat();
}

export async function resolveOutline(s: EngineSession): Promise<{
  ok: true;
  outline: { title: string; page: number; depth: number }[];
}> {
  if (!s.pdf) return { ok: true, outline: [] };
  try {
    // Race the timer against getOutline() AND flattenOutline().
    const outlinePromise = s.pdf.getOutline().then((items) =>
      flattenOutline(s, items, 0),
    );
    const outline = await withTimeout(outlinePromise, 4000, "outline timeout");
    return { ok: true, outline };
  } catch (_) {
    return { ok: true, outline: [] };
  }
}

// JPEG quality for shelf covers; 0.82 is the knee.
const COVER_QUALITY = 0.82;

// Encode a canvas as a JPEG data URL without blocking the frame.
function encodeJpeg(canvas: HTMLCanvasElement): Promise<string> {
  const sync = () => canvas.toDataURL("image/jpeg", COVER_QUALITY);
  if (typeof canvas.toBlob !== "function" || typeof FileReader === "undefined") {
    return Promise.resolve(sync());
  }
  return new Promise<string>((resolve) => {
    canvas.toBlob(
      (blob) => {
        if (!blob) return resolve(sync());
        const reader = new FileReader();
        reader.onloadend = () => {
          const url = reader.result;
          resolve(typeof url === "string" ? url : sync());
        };
        reader.onerror = () => resolve(sync());
        reader.readAsDataURL(blob);
      },
      "image/jpeg",
      COVER_QUALITY,
    );
  });
}

function checkCoverLive(signal?: AbortSignal): void {
  if (!signal?.aborted) return;
  const error = new Error("Cover bake cancelled");
  error.name = "AbortError";
  throw error;
}

async function renderCoverFromPdf(
  doc: PDFDocumentProxy,
  maxWidth: number,
  signal?: AbortSignal,
): Promise<{ dataUrl: string; width: number; height: number }> {
  checkCoverLive(signal);
  const page = await doc.getPage(1);
  checkCoverLive(signal);
  const vp1 = page.getViewport({ scale: 1 });
  const scale = Math.min((maxWidth || 240) / (vp1.width || 1), 2);
  const viewport = page.getViewport({ scale });
  const made = offscreenFor(viewport);
  if (!made) throw new Error("no_context");
  const { canvas: off, ctx } = made;
  let render: { promise: Promise<unknown>; cancel: () => void } | undefined;
  const cancel = () => {
    render?.cancel();
    releaseCanvas(off);
  };
  signal?.addEventListener("abort", cancel, { once: true });
  try {
    checkCoverLive(signal);
    render = page.render({ canvasContext: ctx, viewport });
    await render.promise;
    checkCoverLive(signal);
    const dataUrl = await encodeJpeg(off);
    checkCoverLive(signal);
    return { dataUrl, width: viewport.width, height: viewport.height };
  } finally {
    signal?.removeEventListener("abort", cancel);
    try { page.cleanup(); } catch (_) { /* best-effort page cleanup */ }
    releaseCanvas(off);
  }
}

export async function coverDataUrl(
  s: EngineSession,
  path: string,
  maxWidth = 240,
  signal?: AbortSignal,
): Promise<CoverResult> {
  try {
    checkCoverLive(signal);
    if (!path) return fail("no_path", "No path");
    let result: { dataUrl: string; width: number; height: number };
    if (s.pdf && s.currentPath === path) {
      result = await renderCoverFromPdf(s.pdf, maxWidth, signal);
    } else {
      const data = await fetchBytes(path);
      // Native reads can answer after cancellation; never spawn their worker.
      checkCoverLive(signal);
      const task = await getDocument({ ...BASE_PARAMS, data });
      noteWorkerCreated(s);
      let cancelled: Promise<void> | undefined;
      const cancel = () => { cancelled ??= destroyTask(s, task); };
      signal?.addEventListener("abort", cancel, { once: true });
      if (signal?.aborted) cancel();
      try {
        checkCoverLive(signal);
        const doc = await task.promise;
        checkCoverLive(signal);
        result = await renderCoverFromPdf(doc, maxWidth, signal);
      } finally {
        signal?.removeEventListener("abort", cancel);
        // The choke point is idempotent; await the original abort's shutdown.
        await (cancelled ?? destroyTask(s, task));
      }
    }
    return { ok: true, dataUrl: result.dataUrl, width: result.width, height: result.height };
  } catch (e) {
    return failFrom(e);
  }
}
