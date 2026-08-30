// Document loading. Local files are read via Tauri IPC (bytes). Web-served
// samples use fetch. Open returns as soon as page 1 is known so the reader
// is never stuck on "Opening…" — the chapter tree resolves separately
// (`resolveOutline`), because flattening it costs one worker round trip per
// destination and must not hold the first paint hostage.

import type {
  CoverResult,
  OpenResult,
  OutlineItem,
  PDFDocumentProxy,
} from "./types";
import { errorInfo, fail, releaseCanvas } from "./canvas";
import { resetPaperForDocument } from "./paper";
import {
  currentPath,
  numPages,
  pdf,
  setCurrentPath,
  setLoadingTask,
  setNumPages,
  setPdf,
} from "./state";

type PdfjsLib = {
  getDocument: (params: Record<string, unknown>) => {
    promise: Promise<PDFDocumentProxy>;
    destroy: () => Promise<void>;
  };
  GlobalWorkerOptions: { workerSrc: string };
  TextLayer: unknown;
};

export type TextLayerCtor = {
  new (opts: {
    textContentSource: { items: unknown[] };
    container: HTMLElement;
    viewport: { width: number; height: number };
  }): { render: () => Promise<void>; cancel: () => void };
};

function isWebServedPath(path: string): boolean {
  if (/^https?:\/\//i.test(path) || /^blob:/i.test(path)) return true;
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

/** Resolve pdf.js off globalThis at call time — never at module evaluate. */
export function getPdfjs(): PdfjsLib {
  const l = globalThis.pdfjsLib as PdfjsLib | undefined;
  if (!l || typeof l.getDocument !== "function") {
    throw new Error("pdf.js is not loaded");
  }
  // Absolute worker URL so Tauri's Worker constructor resolves against the
  // webview origin, not a broken custom-protocol base.
  if (l.GlobalWorkerOptions) {
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

export function getDocument(params: Record<string, unknown>) {
  return getPdfjs().getDocument(params);
}

export function TextLayer(opts: ConstructorParameters<TextLayerCtor>[0]) {
  const Ctor = getPdfjs().TextLayer as TextLayerCtor;
  return new Ctor(opts);
}

const CMAP = { cMapUrl: "/vendor/pdfjs/cmaps/", cMapPacked: true };

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
  if (Array.isArray(bytes)) return Uint8Array.from(bytes as number[]);
  throw Object.assign(new Error("read_file_bytes returned an unexpected type"), {
    name: "UnexpectedResponseException",
  });
}

export async function fetchBytes(path: string): Promise<Uint8Array> {
  if (isWebServedPath(path)) {
    const url = path.startsWith("samples/") ? "/" + path : path;
    return doFetch(url);
  }
  const tauri = globalThis.__TAURI__;
  if (tauri && tauri.core && typeof tauri.core.invoke === "function") {
    const bytes = await tauri.core.invoke("read_file_bytes", { path });
    return toUint8(bytes);
  }
  return doFetch(path);
}

export /** Tauri `convertFileSrc` URL only — never a raw `/Users/...` path (that
 *  was treated as an HTTP URL and hung getDocument). */
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

async function openFromUrl(url: string): Promise<PDFDocumentProxy> {
  const task = getDocument({
    url,
    ...CMAP,
    disableRange: false,
    disableStream: true,
    disableAutoFetch: true,
    rangeChunkSize: 65536,
    isEvalSupported: false,
  });
  setLoadingTask(task);
  return await withTimeout(
    task.promise,
    8000,
    "Timed out opening this PDF (pdf.js worker failed to initialize)",
    () => {
      try {
        void task.destroy();
      } catch (_) {
        /* ignore */
      }
      setLoadingTask(null);
    },
  );
}

async function openFromBytes(bytes: Uint8Array): Promise<PDFDocumentProxy> {
  const task = getDocument({
    data: bytes,
    ...CMAP,
    disableAutoFetch: true,
    disableStream: true,
    isEvalSupported: false,
  });
  setLoadingTask(task);
  return await withTimeout(
    task.promise,
    8000,
    "Timed out opening this PDF (pdf.js worker failed to initialize)",
    () => {
      try {
        void task.destroy();
      } catch (_) {
        /* ignore */
      }
      setLoadingTask(null);
    },
  );
}

async function openDocument(path: string): Promise<PDFDocumentProxy> {
  if (isWebServedPath(path)) {
    const url = path.startsWith("samples/") ? "/" + path : path;
    return await openFromUrl(url);
  }

  // Prefer the asset protocol so pdf.js can Range-request instead of holding
  // the whole file in V8. Windows `https://asset.localhost` intermittently
  // fails ("Failed to fetch") — probe first and fall back to binary IPC
  // (`read_file_bytes` returns an ArrayBuffer, not JSON/Base64).
  const asset = localAssetUrl(path);
  if (asset && (await probeAssetUrl(asset))) {
    try {
      return await openFromUrl(asset);
    } catch (_) {
      /* bytes fallback */
    }
  }

  return await openFromBytes(await fetchBytes(path));
}

function outlineTitle(raw: string | null | undefined): string {
  return String(raw == null ? "" : raw).trim() || "(untitled)";
}

async function flattenOutline(
  items: OutlineItem[] | null | undefined,
  depth: number,
  acc: { title: string; page: number; depth: number }[]
): Promise<typeof acc> {
  for (const it of items || []) {
    let page: number | null = null;
    try {
      if (Array.isArray(it.dest)) {
        const ref = it.dest[0];
        if (ref && typeof ref === "object" && "num" in ref) {
          const idx = await pdf!.getPageIndex(ref);
          page = idx + 1;
        } else if (typeof ref === "number") {
          page = ref + 1;
        }
      } else if (typeof it.dest === "string") {
        const d = await pdf!.getDestination(it.dest);
        if (d && d[0]) {
          const ref = d[0];
          if (ref && typeof ref === "object" && "num" in ref) {
            const idx = await pdf!.getPageIndex(ref);
            page = idx + 1;
          }
        }
      }
    } catch (_) {
      page = null;
    }
    if (page) acc.push({ title: outlineTitle(it.title), page, depth });
    await flattenOutline(it.items, depth + 1, acc);
  }
  return acc;
}

export async function open(path: string): Promise<OpenResult> {
  try {
    const destroy = (globalThis as unknown as { __pdfDestroy?: () => Promise<void> }).__pdfDestroy;
    if (destroy) await destroy();
    const doc = await openDocument(path);
    setPdf(doc);
    setNumPages(doc.numPages);
    setCurrentPath(path);
    // A new document means a fresh paper-detection budget and palette
    // (engine/paper.ts) — plus, when the cache remembers this book, its
    // colours published right away. Runs after setCurrentPath so the cache
    // can key on the path.
    resetPaperForDocument();

    // Metadata and page 1 are independent worker round trips — asking for
    // them together is one hop off every document open. Metadata failures are
    // swallowed (exotic docs); page 1 failing IS an open failure.
    const [meta, page1] = await Promise.all([
      doc.getMetadata().catch(() => null),
      doc.getPage(1),
    ]);
    const title: string | null = (meta && meta.info && meta.info.Title) || null;
    const author: string | null = (meta && meta.info && meta.info.Author) || null;

    const vp = page1.getViewport({ scale: 1 });
    try { page1.cleanup(); } catch (_) { /* ignore */ }

    // Seed every page with page-1's size so open returns immediately.
    // A serial getPage(n) over a long book looked like a permanent hang.
    const pageHeights: number[] = new Array(numPages);
    const pageWidths: number[] = new Array(numPages);
    for (let i = 0; i < numPages; i += 1) {
      pageHeights[i] = vp.height;
      pageWidths[i] = vp.width;
    }

    return {
      ok: true,
      numPages,
      title,
      author,
      // The outline is deliberately NOT resolved here — see resolveOutline.
      outline: [],
      page1Size: { width: vp.width, height: vp.height },
      pageHeights,
      pageWidths,
    };
  } catch (e) {
    const er = e as { name?: string };
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
    const info = errorInfo(e);
    return fail(info.name, info.message);
  }
}

/** The open document's flattened chapter tree. Each entry's destination is
 * resolved through the pdf.js worker (`getPageIndex`), so a textbook-sized
 * outline costs a round trip per chapter — call this AFTER the reader is
 * up, never in front of `open`. Races the 4s timeout; an empty tree on
 * failure keeps the caller's fallback path ("no chapters"). */
export async function resolveOutline(): Promise<{
  ok: true;
  outline: { title: string; page: number; depth: number }[];
}> {
  if (!pdf) return { ok: true, outline: [] };
  try {
    // Race the timer against getOutline() AND flattenOutline(). Awaiting
    // getOutline() first meant a hung outline never started the 4s timeout.
    const outlinePromise = pdf.getOutline().then((items) =>
      flattenOutline(items, 0, []),
    );
    const outline = await withTimeout(outlinePromise, 4000, "outline timeout");
    return { ok: true, outline };
  } catch (_) {
    return { ok: true, outline: [] };
  }
}

function renderCoverFromPdf(
  doc: PDFDocumentProxy,
  maxWidth: number
): Promise<{ dataUrl: string; width: number; height: number }> {
  return doc.getPage(1).then((page) => {
    const vp1 = page.getViewport({ scale: 1 });
    const scale = Math.min((maxWidth || 240) / (vp1.width || 1), 2);
    const viewport = page.getViewport({ scale });

    const off = document.createElement("canvas");
    off.width = Math.max(1, Math.floor(viewport.width));
    off.height = Math.max(1, Math.floor(viewport.height));
    const ctx = off.getContext("2d", { alpha: false });
    if (!ctx) throw new Error("no_context");
    return page
      .render({ canvasContext: ctx, viewport })
      .promise.then(() => {
        try { page.cleanup(); } catch (_) { /* ignore */ }
        const dataUrl = off.toDataURL("image/jpeg", 0.82);
        releaseCanvas(off);
        return { dataUrl, width: viewport.width, height: viewport.height };
      });
  });
}

export async function coverDataUrl(path: string, maxWidth = 240): Promise<CoverResult> {
  try {
    if (!path) return fail("no_path", "No path");
    let result: { dataUrl: string; width: number; height: number };
    if (pdf && currentPath === path) {
      result = await renderCoverFromPdf(pdf, maxWidth);
    } else {
      const task = getDocument({
        data: await fetchBytes(path),
        ...CMAP,
        disableAutoFetch: true,
        disableStream: true,
      });
      try {
        const doc = await task.promise;
        result = await renderCoverFromPdf(doc, maxWidth);
      } finally {
        try { await task.destroy(); } catch (_) { /* ignore */ }
      }
    }
    return { ok: true, dataUrl: result.dataUrl, width: result.width, height: result.height };
  } catch (e) {
    const info = errorInfo(e);
    return fail(info.name, info.message);
  }
}

export async function takePendingFile(): Promise<string | null> {
  const tauri = globalThis.__TAURI__;
  if (!tauri || !tauri.core || typeof tauri.core.invoke !== "function") {
    return null;
  }
  try {
    const path = await tauri.core.invoke("take_pending_file");
    return typeof path === "string" && path ? path : null;
  } catch (_) {
    return null;
  }
}

