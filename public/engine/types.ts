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
  // Content hashes: `[permanent, temporary]`; the search index caches
  // under the permanent one.
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
  /** Registered with its own elements: never re-resolved by id. */
  pinned: boolean;
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
  // Root/style fingerprint; values compared separately.
  token: string | null;
  /** Actual bake inputs for this document session's pane. */
  inputs: string | null;
  filter: string;
  blend: string;
  paperInfo: PaperInfo | null;
  gen: number;
};

export type PaperInfo = { color: string; rgb: [number, number, number] };
export type FilterMatrix = { m: number[]; o: number[] };

// A raw page frame: the raster downscaled to a ≤96px long edge.
export type PaperFrame = {
  page: number;
  width: number;
  height: number;
  data: Uint8ClampedArray;
};

export type ActiveMatch = { page: number; index: number } | null;

type Err = { ok: false; error: { name: string; message: string } };
type Ok<T extends Record<string, unknown>> = T & { ok: true };
// Success or failure; the `ok` boolean is the discriminant.
type Result<T extends Record<string, unknown>> = Ok<T> | Err;

export type OpenResult = Result<{
  numPages: number;
  title: string | null;
  author: string | null;
  // The document's permanent content fingerprint; null on old engines.
  fingerprint: string | null;
  // Deliberately empty: the outline resolves after the reader is up.
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
// One page's intrinsic (scale-1) box, from the document, not a raster.
export type PageSizeResult = Result<{ width: number; height: number }>;
export type ThumbResult = Result<{ width: number; height: number; scale: number }>;
export type CoverResult = Result<{ dataUrl: string; width: number; height: number }>;
// The engine's live resource picture: gauges and counters.
export type Stats = {
  // Recent time-to-visible in ms (request, queue, raster slot, raster);
  // max-aggregated across sessions.
  fillMs: number;
  /** One session's page-lane slot count, max-aggregated for the same reason. */
  pageLimit: number;
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
  // Monotonic counters. Pairing: sessions, workers, renders, prefetches.
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
  // The open document's page count, not the registered hosts.
  documentPages: number;
  // Thumbnail generation map size; teardown requires 0.
  thumbGenerationSize: number;
  // Raw-raster retention timers still armed; teardown requires 0.
  rawRetentionTimers: number;
  /** The document-scoped idle sweeper timer, 0/1. Destroy cancels it. */
  sweepTimerArmed: number;
  // Estimated bytes of page render surfaces (w × h × 4).
  pageCanvasBytesEst: number;
  /** Estimated bytes of the thumbnail cache's rasters (raw + display). */
  thumbnailRasterBytesEst: number;
  // Estimated bytes of retained unbaked raws; teardown requires 0.
  rawRetentionBytesEst: number;
  // Estimated bytes in the bake recycler; must not grow per cycle.
  pooledIntermediateBytesEst: number;
};
export type RenderTracePhase = "start" | "complete" | "cancel" | "fail";

export type RenderTraceEntry = {
  /** The session whose page the raster belongs to. */
  sid: number;
  /** The measurement generation the raster belongs to. */
  gen: number;
  /** The page number the engine started rasterizing. */
  page: number;
  phase: RenderTracePhase;
  t: number;
};

// A session id, minted once and never reused; unknown sids do nothing.
export type Sid = number;

// The realm aggregate: sessions live and retired.
export type AggregateStats = Stats & { sessionsLive: number; sessionsRetired: number };

export type PDFReaderApi = {
  version: () => string;
  // Begin a render-trace generation; rasters carry the returned id.
  beginRenderGeneration: () => number;
  /** A bounded copy of the engine's render trace, oldest first. */
  renderTrace: () => RenderTraceEntry[];
  // Turn lifecycle narration on/off; counters stay live.
  setLifecycleLog: (on: boolean) => void;

  // --- Session lifecycle ---------------------------------------------------
  /** Register session `sid` (false if the sid is not new). */
  createSession: (sid: Sid) => boolean;
  // Tear the session down and retire the sid; idempotent.
  destroySession: (sid: Sid) => Promise<void>;
  // Make `sid`'s paper the root backdrop's.
  presentSession: (sid: Sid) => void;
  /** The live sids, in creation order (diagnostics). */
  sessions: () => Sid[];

  // --- Document (all session-scoped) ---------------------------------------
  open: (sid: Sid, path: string) => Promise<OpenResult>;
  resolveOutline: (sid: Sid) => Promise<OutlineResult>;
  // `canvas`/`host` are the page's OWN elements: pinned, never re-resolved.
  registerPage: (
    sid: Sid,
    page: number,
    canvasId: string,
    hostId?: string,
    canvas?: HTMLCanvasElement | null,
    host?: HTMLElement | null
  ) => void;
  unregisterPage: (sid: Sid, canvasId: string) => void;
  cancelPage: (sid: Sid, canvasId: string) => void;
  // Re-rank a queued page raster; it reorders the lane without restarting.
  reprioritizePage: (sid: Sid, canvasId: string, rank: number) => void;
  // Cancel every in-flight page render of the session.
  cancelPageRenders: (sid: Sid) => void;
  // The work-stop half: every in-flight and queued job stops now.
  quiesce: (sid: Sid) => void;
  // `rank` orders this session's page lane, lower first.
  renderPage: (
    sid: Sid,
    canvasId: string,
    scale: number,
    renderText: boolean,
    rank?: number
  ) => Promise<RenderResult>;
  renderThumb: (
    sid: Sid,
    canvasId: string,
    page: number,
    scale: number
  ) => Promise<ThumbResult>;
  // One page's scale-1 box, one worker trip, no surface.
  probePageSize: (sid: Sid, page: number) => Promise<PageSizeResult>;
  cancelThumb: (sid: Sid, canvasId: string) => void;
  hasThumb: (sid: Sid, page: number, scale: number) => boolean;
  coverDataUrl: (sid: Sid, path: string, maxWidth?: number) => Promise<CoverResult>;
  /** Extract one page's text runs for the Rust search index. */
  extractPageText: (sid: Sid, page: number) => Promise<
    | (Ok<{ page: number; items: { str: string; x: number; y: number; w: number; h: number }[] }>)
    | Err
  >;
  /** Publish the active query so the session's text layers repaint. */
  setSearchContext: (sid: Sid, query: string) => void;
  setActiveMatch: (sid: Sid, page: number, index: number) => void;
  clearHighlights: (sid: Sid) => void;
  // Record the session's paper; the root follows the publisher.
  setPaper: (sid: Sid, hex: string) => void;
  // The session's blend switch; gates stashPaperFrame.
  setPaperActive: (sid: Sid, on: boolean) => void;
  takePaperFrame: (sid: Sid, canvasId: string) => (PaperFrame & { ok: true }) | null;
  samplePaperPage: (sid: Sid, page: number) => Promise<
    | (PaperFrame & { ok: true })
    | { ok: true }
  >;
  sweep: (sid: Sid) => void;
  /** Drop the `.page-snapshot` scrub covers the session's page hosts still
   *  carry, zeroing their backing stores. */
  sweepSnapshots: (sid: Sid) => void;
  prefetchThumb: (sid: Sid, page: number, scale: number) => Promise<void>;
  // The pane left the screen: drop queued prefetches, refuse new ones.
  suspendPrefetches: (sid: Sid) => void;
  resumePrefetches: (sid: Sid) => void;
  /** One session's gauges and counters (null for an unknown sid). */
  sessionStats: (sid: Sid) => Stats | null;

  // --- Realm (no document identity) ----------------------------------------
  // Realm aggregate: gauges over live, counters over all sessions.
  stats: () => AggregateStats;
  /** Appearance broadcast: every live session re-bakes its own rasters
   *  against the new global appearance. */
  refreshTheme: () => Promise<void>;
  // Enter/leave the scrub window on every live session.
  setScrubMode: (on: boolean) => Promise<void>;
  // Whether the appearance popover is open; raws are kept while it is.
  setAppearanceMenuOpen: (on: boolean) => void;
};
