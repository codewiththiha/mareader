// Shared pdf.js / engine types. Runtime-free: tsc erases this module.

export type PdfjsLib = {
  getDocument: (params: Record<string, unknown>) => LoadingTask;
  GlobalWorkerOptions: { workerSrc: string };
  TextLayer: new (opts: {
    textContentSource: { items: unknown[] };
    container: HTMLElement;
    viewport: Viewport;
  }) => TextLayerHandle;
};

export type LoadingTask = {
  promise: Promise<PDFDocumentProxy>;
  destroy: () => Promise<void>;
};

export type PDFDocumentProxy = {
  numPages: number;
  /** Content hashes of the file: `[permanent, temporary]`. The permanent
   *  fingerprint is the document's identity across opens — the search index
   *  caches under it so a reopen adopts the retained index. */
  fingerprints: string[];
  getPage: (n: number) => Promise<PDFPageProxy>;
  getMetadata: () => Promise<{ info?: { Title?: string | null; Author?: string | null } }>;
  getOutline: () => Promise<OutlineItem[] | null>;
  getPageIndex: (ref: unknown) => Promise<number>;
  getDestination: (name: string) => Promise<unknown[] | null>;
  cleanup: () => Promise<void>;
};

export type PDFPageProxy = {
  getViewport: (opts: { scale: number }) => Viewport;
  render: (opts: {
    canvasContext: CanvasRenderingContext2D;
    viewport: Viewport;
    transform?: number[] | null;
  }) => RenderTask;
  getTextContent: () => Promise<{ items: TextItem[] }>;
  getAnnotations: (opts: { intent: string }) => Promise<Annotation[]>;
  cleanup: () => Promise<void>;
};

export type RenderTask = { promise: Promise<void>; cancel: () => void };
type TextLayerHandle = { render: () => Promise<void>; cancel: () => void };

export type Viewport = {
  width: number;
  height: number;
  convertToViewportPoint: (x: number, y: number) => [number, number];
};

export type TextItem = {
  str: string;
  transform?: number[];
  width?: number;
  height?: number;
};

export type OutlineItem = {
  title?: string | null;
  dest: string | unknown[];
  items?: OutlineItem[];
};

export type Annotation = {
  subtype?: string;
  url?: string;
  dest?: string | unknown[];
  rect?: [number, number, number, number];
};

export type MaybeCanvas = HTMLCanvasElement | ImageBitmap | null;
export type Raster = HTMLCanvasElement | ImageBitmap;

export type PageState = {
  page: number;
  canvas: HTMLCanvasElement | null;
  host: HTMLElement | null;
  textLayerEl: HTMLElement | null;
  renderTask: RenderTask | null;
  textLayer: TextLayerHandle | null;
  viewport: Viewport | null;
  scale: number;
  dead: boolean;
  rawCanvas: HTMLCanvasElement | null;
  queueGen: number;
  queueHandle: number;
};

export type ThumbEntry = {
  raw: MaybeCanvas;
  display: MaybeCanvas;
  cssW: number;
  cssH: number;
  scale: number;
  gen: number;
  pending: Promise<MaybeCanvas> | null;
};

export type PipelineCache = {
  token: string | null;
  filter: string;
  blend: string;
  paperInfo: PaperInfo | null;
  gen: number;
};

export type PaperInfo = { color: string; rgb: [number, number, number] };
export type FilterMatrix = { m: number[]; o: number[] };

/** A raw page frame for the paper pipeline: the raster downscaled to a
 * ≤96px long edge, with its RGBA pixels. */
export type PaperFrame = {
  page: number;
  width: number;
  height: number;
  data: Uint8ClampedArray;
};

export type ActiveMatch = { page: number; index: number } | null;

type Err = { ok: false; error: { name: string; message: string } };
type Ok<T extends Record<string, unknown>> = T & { ok: true };
/** A discriminated union of success or failure, the shape every async engine
 *  API resolves to. The `ok` boolean is the discriminant. */
type Result<T extends Record<string, unknown>> = Ok<T> | Err;

export type OpenResult = Result<{
  numPages: number;
  title: string | null;
  author: string | null;
  /** The document's permanent pdf.js fingerprint — the content identity the
   *  Rust search index caches under. Null only for engines that predate the
   *  field, where the index falls back to the path. */
  fingerprint: string | null;
  /** Deliberately empty: the chapter tree resolves via `resolveOutline`
   * after the reader is up — flattening it would hold `open` hostage to a
   * worker round trip per destination. */
  outline: { title: string; page: number; depth: number }[];
  page1Size: { width: number; height: number };
  pageHeights: number[];
  pageWidths: number[];
}>;
/** The result of resolving a document's outline: flattened chapter tree. */
type OutlineResult = Result<{
  outline: { title: string; page: number; depth: number }[];
}>;
export type RenderResult = Result<{ width: number; height: number; scale: number }>;
export type ThumbResult = Result<{ width: number; height: number; scale: number }>;
export type CoverResult = Result<{ dataUrl: string; width: number; height: number }>;
/** The engine's live resource picture, read by the app's diagnostics surface
 *  and asserted by the smoke teardown: the four session gauges plus the
 *  lifecycle counters whose pairing rules (every session/worker dies once,
 *  every started render resolves) are the teardown baseline. */
export type Stats = {
  /** Registered page hosts (live page surfaces). */
  pages: number;
  /** Cached thumbnail rasters. */
  thumbs: number;
  thumbLimit: number;
  /** In-flight thumbnail renders, prefetches included (prefetch tasks ride
   *  the same table under `prefetch-<page>` ids). */
  thumbTasks: number;
  /** Live page render tasks (started, not yet resolved). */
  activeRenders: number;
  /** Bounded page-render lane: queued jobs / running slots. */
  pageQueue: number;
  pageActive: number;
  /** Thumbnail lane: queued jobs / running slots. */
  thumbQueue: number;
  thumbActive: number;
  /** Thumbnail prefetches currently in flight (queued or rendering). */
  activePrefetches: number;
  /** A document proxy is open. */
  hasDocument: boolean;
  /** A worker LoadingTask is registered on the session. */
  hasLoadingTask: boolean;
  /** Monotonic lifecycle counters. Pairing rules: sessionsOpened ==
   *  sessionsDestroyed; workersCreated == workersTerminated; rendersStarted
   *  == rendersCompleted + rendersCancelled + rendersFailed;
   *  prefetchesStarted == prefetchesCompleted + prefetchesDropped. */
  sessionsOpened: number;
  sessionsDestroyed: number;
  workersCreated: number;
  workersTerminated: number;
  rendersStarted: number;
  rendersCompleted: number;
  rendersCancelled: number;
  rendersFailed: number;
  /** Jobs that entered the bounded render lane, and queue-level drops
   *  (superseded/unmounted before their turn). */
  rendersQueued: number;
  rendersDropped: number;
  /** Thumbnail prefetch lifecycle: the warmup/idle cache fills. */
  prefetchesStarted: number;
  prefetchesCompleted: number;
  prefetchesDropped: number;
  /** The OPEN DOCUMENT's page count — NOT `pages` (registered page hosts).
   *  The baseline gates its fixtures on this: a distant jump must cross
   *  many pages. */
  documentPages: number;
  /** Thumbnail generation map size: per-canvas bookkeeping kept until
   *  document teardown; teardown clears it, so the baseline requires 0. */
  thumbGenerationSize: number;
  /** Raw-raster retention timers still armed. Teardown releases every page
   *  surface, so the baseline requires 0 after a close. */
  rawRetentionTimers: number;
  /** The document-scoped idle sweeper timer, 0/1. Destroy cancels it. */
  sweepTimerArmed: number;
  /** Estimated bytes of the registered page render surfaces (the page
   *  states' bake targets). Width x height x 4 RGBA — an ESTIMATE for the
   *  baseline, not a physical allocation query, and a different ledger from
   *  the DOM-scanned liveCanvasBytes (same surfaces, engine registry vs DOM
   *  walk). Never part of a "total RAM" number. */
  pageCanvasBytesEst: number;
  /** Estimated bytes of the thumbnail cache's rasters (raw + display). */
  thumbnailRasterBytesEst: number;
  /** Estimated bytes of retained unbaked raws (the appearance/scrub
   *  retention window). Teardown releases every page surface, so the
   *  baseline requires 0 after a close. */
  rawRetentionBytesEst: number;
  /** Estimated bytes in the bake-intermediates recycler (pooled canvases +
   *  the shared scratch). Module-bounded, not document-owned: it may hold
   *  placeholders across closes but must not grow per cycle. */
  pooledIntermediateBytesEst: number;
};
export type RenderTracePhase = "start" | "complete" | "cancel" | "fail";

export type RenderTraceEntry = {
  /** The measurement generation the raster belongs to. */
  gen: number;
  /** The page number the engine started rasterizing. */
  page: number;
  phase: RenderTracePhase;
  t: number;
};

export type PDFReaderApi = {
  version: () => string;
  /** Begin a render-trace measurement generation (the Phase 0 fast-jump
   *  page-identity proof): every raster started from now carries the
   *  returned id. */
  beginRenderGeneration: () => number;
  /** A bounded copy of the engine's render trace, oldest first. */
  renderTrace: () => RenderTraceEntry[];
  open: (path: string) => Promise<OpenResult>;
  resolveOutline: () => Promise<OutlineResult>;
  destroy: () => Promise<void>;
  /** Turn the engine's lifecycle event narration on/off (dev diagnostics;
   *  the counters in stats() are always live). */
  setLifecycleLog: (on: boolean) => void;
  registerPage: (page: number, canvasId: string, hostId?: string) => void;
  unregisterPage: (canvasId: string) => void;
  cancelPage: (canvasId: string) => void;
  renderPage: (
    canvasId: string,
    scale: number,
    renderText: boolean
  ) => Promise<RenderResult>;
  renderThumb: (
    canvasId: string,
    page: number,
    scale: number
  ) => Promise<ThumbResult>;
  cancelThumb: (canvasId: string) => void;
  hasThumb: (page: number, scale: number) => boolean;
  blitThumb: (canvasId: string, page: number) => boolean;
  coverDataUrl: (path: string, maxWidth?: number) => Promise<CoverResult>;
  stats: () => Stats;
  /** Extract one page's text runs for the Rust search index. */
  extractPageText: (page: number) => Promise<
    | (Ok<{ page: number; items: { str: string; x: number; y: number; w: number; h: number }[] }>)
    | Err
  >;
  /** Publish the active query so mounted text layers repaint highlights. */
  setSearchContext: (query: string) => void;
  setActiveMatch: (page: number, index: number) => void;
  clearHighlights: () => void;
  refreshTheme: () => Promise<void>;
  /** Enter/leave the scrub window's real-time compositing: raw rasters under
   * the live CSS filter + blend, re-baked on exit. */
  setScrubMode: (on: boolean) => Promise<void>;
  /** Whether the appearance popover is open. Rendered pages retain their
   * unbaked raws while it is, so the session's first tint drag blits
   * instead of re-rendering; closing arms the idle tail that frees them. */
  setAppearanceMenuOpen: (on: boolean) => void;
  /** Publish (or, with "", clear) `--pdf-paper`. */
  setPaper: (hex: string) => void;
  /** The Rust paper session's blend switch — gates stashPaperFrame so idle
   * renders cost nothing on the paper pipeline. */
  setPaperActive: (on: boolean) => void;
  takePaperFrame: (canvasId: string) => (PaperFrame & { ok: true }) | null;
  samplePaperPage: (page: number) => Promise<
    | (PaperFrame & { ok: true })
    | { ok: true }
  >;
  sweep: () => void;
  /** Drop the `.page-snapshot` zoom masks the live page hosts still carry,
   *  zeroing their backing stores: a mask whose render was superseded or
   *  never landed would otherwise keep a full-page raster alive until the
   *  host unmounts. */
  sweepSnapshots: () => void;
  takePendingFile: () => Promise<string | null>;
  prefetchThumb: (page: number, scale: number) => Promise<void>;
};
