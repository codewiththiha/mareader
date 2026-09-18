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
export type Stats = {
  pages: number;
  thumbs: number;
  thumbLimit: number;
  thumbTasks: number;
};
export type PDFReaderApi = {
  version: () => string;
  open: (path: string) => Promise<OpenResult>;
  resolveOutline: () => Promise<OutlineResult>;
  destroy: () => Promise<void>;
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
