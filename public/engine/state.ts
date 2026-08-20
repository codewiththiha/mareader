// Mutable engine session state. One document, many page/thumb surfaces.

import type {
  LoadingTask,
  PDFDocumentProxy,
  PageState,
  RenderTask,
  SearchRect,
  TextIndexEntry,
  ThumbEntry,
  ActiveMatch,
} from "./types";
import { disposeScratch, releaseCanvas } from "./canvas";

export const ENGINE_VERSION = "0.2.0";

export let loadingTask: LoadingTask | null = null;
export let pdf: PDFDocumentProxy | null = null;
export let numPages = 0;
export let currentPath: string | null = null;

export function setLoadingTask(t: LoadingTask | null): void {
  loadingTask = t;
}
export function setPdf(doc: PDFDocumentProxy | null): void {
  pdf = doc;
}
export function setNumPages(n: number): void {
  numPages = n;
}
export function setCurrentPath(p: string | null): void {
  currentPath = p;
}

export const stateByCanvasId = new Map<string, PageState>();
export const thumbCache = new Map<number, ThumbEntry>();
/** Cap kept tight: each thumb is a pair of rasters. 8 covers the 2-column
 *  window + one buffer row without pinning a textbook's worth of bitmaps. */
export const THUMB_CACHE_MAX = 8;
export const thumbTasks = new Map<string, RenderTask>();
export const thumbCancelled = new Set<string>();
export const thumbLive = new Map<string, { page: number }>();

export const textIndex = new Map<number, TextIndexEntry[]>();
export const highlightsByPage = new Map<number, SearchRect[]>();
export let searchQuery = "";
export let highlightMode: "live" | "stale" = "live";
export let activeMatch: ActiveMatch = null;

export function setSearchQuery(q: string): void {
  searchQuery = q;
}
export function setHighlightModeValue(m: "live" | "stale"): void {
  highlightMode = m;
}
export function setActiveMatchValue(m: ActiveMatch): void {
  activeMatch = m;
}

export let renderCount = 0;
export const CLEANUP_EVERY = 5;
export function bumpRenderCount(): number {
  renderCount += 1;
  return renderCount;
}

export let scrubbing = false;
export function setScrubbing(on: boolean): void {
  scrubbing = on;
}

export const PAGE_MAX_PIXELS = 4 * 1024 * 1024;
export const CANVAS_AREA_FACTOR = 1.0;

const RAW_IDLE_MS = 10_000;
const rawTimers = new WeakMap<PageState, ReturnType<typeof setTimeout>>();

let idleTimer: ReturnType<typeof setTimeout> | 0 = 0;

/** Reset the 30s idle sweeper (pdf.cleanup + scratch/pool drain). */
export function noteActivity(): void {
  if (idleTimer) clearTimeout(idleTimer);
  idleTimer = setTimeout(() => {
    sweepPdf();
    disposeScratch();
  }, 30_000);
}

export function releaseThumbEntry(entry: ThumbEntry | null | undefined): void {
  if (!entry) return;
  try {
    if (entry.display && typeof (entry.display as ImageBitmap).close === "function") {
      (entry.display as ImageBitmap).close();
    }
  } catch (_) {
    /* already closed */
  }
  const display = entry.display;
  const raw = entry.raw;
  entry.display = null;
  entry.raw = null;
  releaseCanvas(display);
  if (raw && raw !== display) releaseCanvas(raw);
}

export function releasePageSurfaces(st: PageState | null): void {
  if (!st) return;
  if (st.host) {
    try {
      st.host.querySelectorAll("canvas").forEach((c) => releaseCanvas(c as HTMLCanvasElement));
      const text = st.host.querySelector(".textLayer");
      if (text) text.replaceChildren();
      const links = st.host.querySelector(".linkLayer");
      if (links) links.remove();
      st.host.querySelectorAll(".highlight").forEach((n) => n.remove());
      st.host.querySelectorAll(".page-snapshot").forEach((n) => n.remove());
    } catch (_) {
      /* host already detached */
    }
  }
  const rawTimer = rawTimers.get(st);
  if (rawTimer) clearTimeout(rawTimer);
  if (st.rawCanvas && st.rawCanvas !== st.canvas) releaseCanvas(st.rawCanvas);
  st.rawCanvas = null;
  releaseCanvas(st.canvas);
  st.canvas = null;
  st.host = null;
  st.textLayerEl = null;
  st.viewport = null;
}

export function sweepPdf(): void {
  if (!pdf) return;
  try {
    Promise.resolve(pdf.cleanup()).catch(() => {
      /* advisory */
    });
  } catch (_) {
    /* ignore */
  }
}

/** Keep the unbaked raw briefly so a tint slider can restore it, then free it.
 *  The next theme change / scrub without a raw re-renders from pdf.js. */
export function dropRawIfIdle(st: PageState): void {
  const prev = rawTimers.get(st);
  if (prev) clearTimeout(prev);
  rawTimers.set(
    st,
    setTimeout(() => {
      if (st.dead || scrubbing) return;
      if (st.rawCanvas && st.rawCanvas !== st.canvas) releaseCanvas(st.rawCanvas);
      st.rawCanvas = null;
    }, RAW_IDLE_MS),
  );
}
