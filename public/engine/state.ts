// Mutable engine session state, one instance. A document open/teardown
// resets it; every module reaches it through the exported `session` object
// rather than `export let` bindings each import site could shadow, and
// destroy() in public/pdfEngine.ts is the single place everything is torn
// down. The maps below are window-bound (the virtualizer keeps `budget`
// pages live) or LRU-bounded (thumbCache <= THUMB_CACHE_MAX), so the session
// never grows with document length.

import type {
  ActiveMatch,
  LoadingTask,
  PDFDocumentProxy,
  PageState,
  RenderTask,
  ThumbEntry,
} from "./types";
import { disposeScratch, releaseCanvas } from "./canvas";
import { PAGE_SNAPSHOT_SELECTOR, TEXT_LAYER_SELECTOR } from "./dom-contract";

// The engine's own API version, served as `PDFReader.version()`. It tracks the
// JS surface rather than the app release, and unlike the six sources
// `tools/check-versions.ts` compares, nothing here checks it against them.
export const ENGINE_VERSION = "0.5.0";

/** Cap kept tight: each thumb is a pair of rasters. 16 keeps several
 *  scroll-windowfuls warm: ~8MB total (thumb pairs at 0.25 scale are small). */
export const THUMB_CACHE_MAX = 16;

/** Max pixels per canvas layer. The 16M base (~64 MB RGBA) is the ceiling,
 *  not the target: US-Letter at 100% zoom on a 2x display is ~1.5M px, at
 *  200% ~7.8M. 16M keeps the FULL native devicePixelRatio through ~200% zoom
 *  on any display; total GPU memory is bounded by the 3-page mounted ceiling
 *  (RENDER_BUDGET), not by this. The ceiling scales with the device's
 *  reported memory (navigator.deviceMemory, Chromium-only; elsewhere the
 *  base applies) so a 16 GB machine can push ~200% on a 3x display without
 *  hitting the cap. */
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

// A retained raw is only worth its full-page surface while a tint scrub can
// still plausibly restore it; the idle timer is the short tail of that
// window, not a standing keep-alive.
const RAW_IDLE_MS = 2_000;
/** How long after a scrub transition a bake still retains its unbaked raw,
 *  so back-to-back drags restore without a re-render. Outside the window the
 *  raw is dropped at the bake and the scrub path re-renders on demand
 *  (preparePagesForScrub) — a raw nobody will ask for is pure peak
 *  inflation, and the footprint latches onto the peak. */
const SCRUB_RAW_RETAIN_MS = 30_000;
const SWEEP_IDLE_MS = 30_000;

/** The engine's per-document session state: the pdf.js document proxy, live
 *  page surfaces, thumbnail cache, search context, and theme pipeline state.
 *  One instance per open document; destroyed and recreated on every `open()`. */
class EngineSession {
  loadingTask: LoadingTask | null = null;
  pdf: PDFDocumentProxy | null = null;
  numPages = 0;
  currentPath: string | null = null;

  /** The dominant raster colour of the open document — the PDF's own paper —
   *  or null until the paper session (the Rust side of the pipeline) resolves
   *  one. During a scrub the blend backdrop re-derives from this through the
   *  same live CSS filter + blend the raw canvases use; settled, it paints
   *  the pre-rendered twin the engine derives from it (--pdf-paper-baked,
   *  theme/paper.ts), so backdrop and page are the same composite by
   *  construction either way. */
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

  /** The last scrub-mode transition (Date.now()), recorded by the theme
   *  queue on the way in AND out. A bake retains its unbaked raw only while
   *  a scrub inside SCRUB_RAW_RETAIN_MS of this is plausible. */
  lastScrubAt = 0;

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

  /** Remember a scrub transition: bakes landing inside the retention window
   *  keep their unbaked raw so the next drag restores without a re-render. */
  noteScrub(): void {
    this.lastScrubAt = Date.now();
  }

  /** Whether a tint scrub is plausible right now — the retention gate for
   *  the unbaked raw a bake just produced. Zero means "never scrubbed this
   *  session", which is not plausible. */
  scrubIsPlausible(): boolean {
    return this.lastScrubAt > 0 && Date.now() - this.lastScrubAt < SCRUB_RAW_RETAIN_MS;
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
        const text = st.host.querySelector(TEXT_LAYER_SELECTOR);
        if (text) text.replaceChildren();
        const links = st.host.querySelector(".linkLayer");
        if (links) links.remove();
        st.host.querySelectorAll(".highlight").forEach((n) => n.remove());
        st.host.querySelectorAll(PAGE_SNAPSHOT_SELECTOR).forEach((n) => n.remove());
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
