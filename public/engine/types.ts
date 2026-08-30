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
export type TextLayerHandle = { render: () => Promise<void>; cancel: () => void };

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

/** Which pixels of a page the paper detector trusts — the engine side of
 * the reader's `layout.blend_area` setting, persisted alongside each cached
 * colour so a cache found under one area is not reused under the other. */
export type PaperArea = "whole" | "edges";

export type SearchRect = { x: number; y: number; w: number; h: number };
export type TextIndexEntry = { str: string; x: number; y: number; w: number; h: number };
export type SearchMatch = SearchRect & { page: number; index: number; text: string };
export type ActiveMatch = { page: number; index: number } | null;

export type Err = { ok: false; error: { name: string; message: string } };
export type Ok<T extends Record<string, unknown>> = T & { ok: true };
export type Result<T extends Record<string, unknown>> = Ok<T> | Err;

export type OpenResult = Result<{
  numPages: number;
  title: string | null;
  author: string | null;
  /** Deliberately empty: the chapter tree resolves via `resolveOutline`
   * after the reader is up (flattening it blocks on one worker round trip
   * per destination, and must not hold `open` hostage). */
  outline: { title: string; page: number; depth: number }[];
  page1Size: { width: number; height: number };
  pageHeights: number[];
  pageWidths: number[];
}>;
export type OutlineResult = Result<{
  outline: { title: string; page: number; depth: number }[];
}>;
export type RenderResult = Result<{ width: number; height: number; scale: number }>;
export type ThumbResult = Result<{ width: number; height: number; scale: number; cached: boolean }>;
export type CoverResult = Result<{ dataUrl: string; width: number; height: number }>;
export type Stats = {
  pages: number;
  thumbs: number;
  thumbLimit: number;
  thumbTasks: number;
};
export type SearchResult = Result<{
  query: string;
  total: number;
  matches: SearchMatch[];
}>;

export type PDFReaderApi = {
  version: () => string;
  open: (path: string) => Promise<OpenResult>;
  resolveOutline: () => Promise<OutlineResult>;
  destroy: () => Promise<void>;
  registerPage: (payload: {
    canvasId: string;
    hostId: string;
    page: number;
  }) => void;
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
  buildSearchIndex: () => Promise<Result<{ count: number }>>;
  search: (query: string) => Promise<SearchResult>;
  setActiveMatch: (page: number, index: number) => void;
  clearHighlights: () => void;
  refreshTheme: () => Promise<void>;
  setScrubMode: (on: boolean) => Promise<void>;
  setPaper: (hex: string, persist: boolean, area: PaperArea) => void;
  /** The Rust paper session's blend switch — gates stashPaperFrame so idle
   * renders cost nothing on the paper pipeline. */
  setPaperActive: (on: boolean) => void;
  /** Bank a fixed colour for the current document WITHOUT publishing it —
   * the Rust paper session's close path (an interrupted scan's answer). */
  persistPaper: (hex: string, area: PaperArea) => void;
  takePaperFrame: (canvasId: string) => (PaperFrame & { ok: true }) | null;
  samplePaperPage: (page: number) => Promise<
    | (PaperFrame & { ok: true })
    | { ok: true }
  >;
  getCachedPaper: (path: string) => {
    ok: true;
    hex: string | null;
    area: PaperArea | null;
  };
  sweep: () => void;
  takePendingFile: () => Promise<string | null>;
  prefetchThumb: (page: number, scale: number) => Promise<void>;
};
