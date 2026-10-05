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
  /** Root/style fingerprint; values are separately compared to ignore engine publications. */
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
/** One page's intrinsic (scale-1) box, read from the document rather than
 *  from a raster. The reader's fit maths asks for it BEFORE the page's first
 *  raster, so a mixed-size book's plate is fitted on its own terms instead of
 *  being rasterised at the previous page's fit and corrected afterwards. */
export type PageSizeResult = Result<{ width: number; height: number }>;
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
  /** The session whose page the raster belongs to. */
  sid: number;
  /** The measurement generation the raster belongs to. */
  gen: number;
  /** The page number the engine started rasterizing. */
  page: number;
  phase: RenderTracePhase;
  t: number;
};

/** A session id: minted by the Rust `PdfSession` that owns the session,
 *  registered once with `createSession`, never reused. Every document call
 *  names one; a call naming an unknown (or retired) sid does nothing — the
 *  async ones resolve `{ok:false, error:{name:"no_session"}}`. */
export type Sid = number;

/** The realm aggregate `stats()` adds to the per-session shape: how many
 *  sessions the registry holds and has retired. */
export type AggregateStats = Stats & { sessionsLive: number; sessionsRetired: number };

export type PDFReaderApi = {
  version: () => string;
  /** Begin a render-trace measurement generation (the Phase 0 fast-jump
   *  page-identity proof): every raster started from now carries the
   *  returned id. Diagnostics only — entries carry their `sid`. */
  beginRenderGeneration: () => number;
  /** A bounded copy of the engine's render trace, oldest first. */
  renderTrace: () => RenderTraceEntry[];
  /** Turn the engine's lifecycle event narration on/off (dev diagnostics;
   *  the counters in stats() are always live). */
  setLifecycleLog: (on: boolean) => void;

  // --- Session lifecycle ---------------------------------------------------
  /** Register session `sid` (false if the sid is not new). */
  createSession: (sid: Sid) => boolean;
  /** Tear the session down — cancel its work, destroy its document and its
   *  pdf.js worker, release its surfaces — and retire the sid. Idempotent;
   *  other sessions are untouched. */
  destroySession: (sid: Sid) => Promise<void>;
  /** Make `sid`'s paper the root backdrop's (the host's choice; by default
   *  the latest document to open presents). */
  presentSession: (sid: Sid) => void;
  /** The live sids, in creation order (diagnostics). */
  sessions: () => Sid[];

  // --- Document (all session-scoped) ---------------------------------------
  open: (sid: Sid, path: string) => Promise<OpenResult>;
  resolveOutline: (sid: Sid) => Promise<OutlineResult>;
  /** `canvas`/`host` are the page's OWN elements when the caller holds
   *  them: the page is then pinned to them and never re-resolved by id — two
   *  panes in one realm carry the same page ids, and a document-wide lookup
   *  would answer with whichever pane comes first. */
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
  /** Cancel every in-flight page render of the session — the close path's
   *  first act, so leaving interrupts raster work instead of racing the
   *  frame channel. */
  cancelPageRenders: (sid: Sid) => void;
  /** The work-stop half of a close intent: every in-flight and queued job
   *  of the session — page renders, thumbnail rasters, prefetch awaits —
   *  stops in the click's own task. The session survives; the one teardown
   *  stays destroySession's and shares the same idempotent sweep. */
  quiesce: (sid: Sid) => void;
  renderPage: (
    sid: Sid,
    canvasId: string,
    scale: number,
    renderText: boolean
  ) => Promise<RenderResult>;
  renderThumb: (
    sid: Sid,
    canvasId: string,
    page: number,
    scale: number
  ) => Promise<ThumbResult>;
  /** One page's scale-1 box, from the document instead of a raster. A worker
   *  round trip and no surface: the reader asks before a page's first raster
   *  (see `probePageSize` in engine/renderer.ts). */
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
  /** Record the session's paper (or, with "", clear it); the root
   *  `--pdf-paper` follows only for the publishing session. */
  setPaper: (sid: Sid, hex: string) => void;
  /** The session's paper blend switch — gates stashPaperFrame so idle
   * renders cost nothing on the paper pipeline. */
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
  /** The session's pane left the screen with its document still loaded:
   *  abandon its queued/in-flight idle prefetches (they settle as drops)
   *  and refuse new ones until `resumePrefetches`. */
  suspendPrefetches: (sid: Sid) => void;
  resumePrefetches: (sid: Sid) => void;
  /** One session's gauges and counters (null for an unknown sid). */
  sessionStats: (sid: Sid) => Stats | null;

  // --- Realm (no document identity) ----------------------------------------
  /** Realm aggregate: gauges summed over live sessions, counters over live
   *  and retired ones — the teardown baseline's view. */
  stats: () => AggregateStats;
  /** Appearance broadcast: every live session re-bakes its own rasters
   *  against the new global appearance. */
  refreshTheme: () => Promise<void>;
  /** Enter/leave the scrub window's real-time compositing on every live
   * session: raw rasters under the live CSS filter + blend, re-baked on
   * exit. */
  setScrubMode: (on: boolean) => Promise<void>;
  /** Whether the appearance popover is open. Rendered pages retain their
   * unbaked raws while it is, so the first tint drag blits instead of
   * re-rendering; closing arms the idle tail that frees them. */
  setAppearanceMenuOpen: (on: boolean) => void;
};

// only the changed file was rewritten
