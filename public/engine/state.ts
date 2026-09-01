// Mutable engine session state, one instance. A document open/teardown
// resets it; every module reaches it through the exported `session` object
// rather than drifting `export let` bindings that each import site could
// shadow. One object makes the lifetime explicit: `destroy()` in
// public/pdfEngine.ts is the single place everything is torn down.
//
// The maps below are window-bound (the virtualizer keeps `budget` pages
// live) or LRU-bounded (thumbCache ≤ THUMB_CACHE_MAX), so the session never
// grows with document length.

import type {
  ActiveMatch,
  LoadingTask,
  PDFDocumentProxy,
  PageState,
  RenderTask,
  ThumbEntry,
} from "./types";
import { disposeScratch, releaseCanvas } from "./canvas";

export const ENGINE_VERSION = "0.5.0"; // 0.5.0: search fully ported to Rust (extractPageText + setSearchContext); registerPage became typed args

/** Cap kept tight: each thumb is a pair of rasters. 16 keeps several
 *  scroll-windowfuls warm: ~8MB total (thumb pairs at 0.25 scale are small). */
export const THUMB_CACHE_MAX = 16;

/** Max pixels per canvas layer. The base ceiling is 16M ≈ 64 MB RGBA — the
 *  ceiling, not the target. A US-Letter page at 100% zoom on a 2x display is
 *  ~1.5M px; at 200% on 2x it's ~7.8M; on a 3x display at 100% it's ~4.4M.
 *  16M keeps the FULL native devicePixelRatio through ~200% zoom on any
 *  display, and only the 3-page mounted ceiling (RENDER_BUDGET max_items: 3)
 *  bounds total GPU memory (≤3 × 16M × 2 copies × 4B ≈ 384 MB worst case;
 *  typical usage is a fraction of that).
 *
 *  The ceiling scales with the device's reported memory so a 4 GB machine
 *  still gets the full 16M at 100% while a 16 GB one can push ~200% on a
 *  3x display without hitting the cap (navigator.deviceMemory is
 *  Chromium-only; elsewhere the base ceiling applies). */
const PAGE_MAX_PIXELS_BASE = 16 * 1024 * 1024;

function memoryScaledPixelCeiling(): number {
  // Guarded end to end: `navigator` is absent in the Node smoke harness and
  // in old webview sandboxes, and `deviceMemory` is Chromium-only — both
  // must fall back to the base ceiling without throwing at module load.
  const nav = typeof navigator !== "undefined" ? (navigator as { deviceMemory?: number }) : undefined;
  const memory = nav && nav.deviceMemory;
  if (typeof memory !== "number" || !(memory > 0)) return PAGE_MAX_PIXELS_BASE;
  if (memory >= 8) return PAGE_MAX_PIXELS_BASE * 2;
  if (memory >= 4) return PAGE_MAX_PIXELS_BASE;
  return PAGE_MAX_PIXELS_BASE / 2;
}

export const PAGE_MAX_PIXELS = memoryScaledPixelCeiling();

const RAW_IDLE_MS = 10_000;
const SWEEP_IDLE_MS = 30_000;

export class EngineSession {
  loadingTask: LoadingTask | null = null;
  pdf: PDFDocumentProxy | null = null;
  numPages = 0;
  currentPath: string | null = null;

  /** The dominant raster colour of the open document — the PDF's own paper —
   *  or null until the paper session (the Rust side of the pipeline)
   *  resolves one. The blend backdrop paints this through the same filter +
   *  blend the raw canvases use, so backdrop and page background are the
   *  same composite by construction. */
  detectedPaper: string | null = null;

  /** Live page surfaces, keyed by canvas id. Bounded by the virtualizer's
   *  live window; `unregisterPage` removes and releases on unmount. */
  readonly stateByCanvasId = new Map<string, PageState>();

  /** LRU-capped thumbnail rasters (≤ THUMB_CACHE_MAX). */
  readonly thumbCache = new Map<number, ThumbEntry>();
  readonly thumbTasks = new Map<string, RenderTask>();
  readonly thumbCancelled = new Set<string>();
  readonly thumbLive = new Map<string, { page: number }>();

  /** Active query + current match for the DOM text-layer highlight pass. */
  searchQuery = "";
  activeMatch: ActiveMatch = null;

  /** Heuristic sweep counter: every CLEANUP_EVERY renders, release worker
   *  caches so memory drops during long reading sessions. */
  renderCount = 0;

  themeScrubActive = false;

  private idleTimer: ReturnType<typeof setTimeout> | 0 = 0;
  private rawTimers = new WeakMap<PageState, ReturnType<typeof setTimeout>>();

  setLoadingTask(t: LoadingTask | null): void {
    this.loadingTask = t;
  }

  setPdf(doc: PDFDocumentProxy | null): void {
    this.pdf = doc;
    if (!doc) this.setDetectedPaper(null); // document gone → re-detect on next open
  }

  setDetectedPaper(hex: string | null): void {
    this.detectedPaper = hex;
    const el = document.documentElement;
    if (hex) el.style.setProperty("--pdf-paper", hex);
    else el.style.removeProperty("--pdf-paper");
  }

  setNumPages(n: number): void {
    this.numPages = n;
  }

  setCurrentPath(p: string | null): void {
    this.currentPath = p;
  }

  setSearchQuery(q: string): void {
    this.searchQuery = q;
  }

  setActiveMatchValue(m: ActiveMatch): void {
    this.activeMatch = m;
  }

  setThemeScrubActive(on: boolean): void {
    this.themeScrubActive = on;
  }

  bumpRenderCount(): number {
    this.renderCount += 1;
    return this.renderCount;
  }

  /** Reset the idle sweeper (pdf.cleanup + scratch/pool drain). */
  noteActivity(): void {
    if (this.idleTimer) clearTimeout(this.idleTimer);
    this.idleTimer = setTimeout(() => {
      this.sweepPdf();
      disposeScratch();
    }, SWEEP_IDLE_MS);
  }

  releaseThumbEntry(entry: ThumbEntry | null | undefined): void {
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

  releasePageSurfaces(st: PageState | null): void {
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
    const rawTimer = this.rawTimers.get(st);
    if (rawTimer) clearTimeout(rawTimer);
    if (st.rawCanvas && st.rawCanvas !== st.canvas) releaseCanvas(st.rawCanvas);
    st.rawCanvas = null;
    releaseCanvas(st.canvas);
    st.canvas = null;
    st.host = null;
    st.textLayerEl = null;
    st.viewport = null;
  }

  sweepPdf(): void {
    if (!this.pdf) return;
    try {
      Promise.resolve(this.pdf.cleanup()).catch(() => {
        /* advisory */
      });
    } catch (_) {
      /* ignore */
    }
  }

  /** Keep the unbaked raw briefly so a tint slider can restore it, then free
   *  it. The next theme change / scrub without a raw re-renders from pdf.js.
   *  The timer is a no-op while scrubbing is active, and teardown
   *  (releasePageSurfaces) clears it outright. */
  dropRawIfIdle(st: PageState): void {
    const prev = this.rawTimers.get(st);
    if (prev) clearTimeout(prev);
    this.rawTimers.set(
      st,
      setTimeout(() => {
        if (st.dead || this.themeScrubActive) return;
        if (st.rawCanvas && st.rawCanvas !== st.canvas) releaseCanvas(st.rawCanvas);
        st.rawCanvas = null;
      }, RAW_IDLE_MS),
    );
  }
}

/** The app's one engine session. */
export const session = new EngineSession();

export const CLEANUP_EVERY = 5;
