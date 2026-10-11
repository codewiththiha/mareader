// Mutable engine session state, one instance per document session.

import type {
  ActiveMatch,
  LoadingTask,
  PaperFrame,
  PDFDocumentProxy,
  PageState,
  PipelineCache,
  RenderTask,
  ThumbEntry,
} from "./types";
import { disposeScratch, releaseCanvas } from "./canvas";
import { PAGE_SNAPSHOT_SELECTOR, TEXT_LAYER_SELECTOR } from "./dom-contract";

// `PDFReader.version()`, the JS surface's version; uncompared.
export const ENGINE_VERSION = "0.7.0";

// Cap kept tight: each thumb is a pair of rasters; 16 keeps several
// scroll-windowfuls warm.
export const THUMB_CACHE_MAX = 16;

// Max pixels per canvas layer: 12M keeps full devicePixelRatio to ~245%.
const PAGE_MAX_PIXELS_BASE = 12 * 1024 * 1024;

function memoryScaledPixelCeiling(): number {
  // Guarded end to end: `navigator` and `deviceMemory` may be absent.
  const nav = typeof navigator !== "undefined" ? (navigator as { deviceMemory?: number }) : undefined;
  const memory = nav && nav.deviceMemory;
  if (typeof memory !== "number" || !(memory > 0)) return PAGE_MAX_PIXELS_BASE;
  if (memory >= 4) return PAGE_MAX_PIXELS_BASE;
  return PAGE_MAX_PIXELS_BASE / 2;
}

export const PAGE_MAX_PIXELS = memoryScaledPixelCeiling();

// A retained raw is worth its surface only while a tint scrub can restore
// it.
const RAW_IDLE_MS = 2_000;
// How long a bake retains its unbaked raw after a scrub.
const SCRUB_RAW_RETAIN_MS = 30_000;
const SWEEP_IDLE_MS = 30_000;

// Drop every `.page-snapshot`, zeroing the backing store first.
function releaseSnapshots(host: HTMLElement): void {
  host.querySelectorAll(PAGE_SNAPSHOT_SELECTOR).forEach((n) => {
    releaseCanvas(n as HTMLCanvasElement);
    n.remove();
  });
}

/** The lifecycle counters every session keeps (see EngineSession). */
const COUNTER_KEYS = [
  "sessionsOpened",
  "sessionsDestroyed",
  "workersCreated",
  "workersTerminated",
  "rendersStarted",
  "rendersCompleted",
  "rendersCancelled",
  "rendersFailed",
  "rendersQueued",
  "rendersDropped",
  "prefetchesStarted",
  "prefetchesCompleted",
  "prefetchesDropped",
] as const;
export type CounterKey = (typeof COUNTER_KEYS)[number];

function zeroCounters(): Record<CounterKey, number> {
  return Object.fromEntries(COUNTER_KEYS.map((k) => [k, 0])) as Record<CounterKey, number>;
}

/** Realm totals over every session that ever lived — diagnostics only. */
export const realmCounters: Record<CounterKey, number> = zeroCounters();

// One queued raster: the canvas it paints, its fill rank, request order,
// and job.
type QueuedRaster = {
  canvasId: string;
  rank: number;
  seq: number;
  run: () => void;
  // Settles the caller's promise for a job that never reaches `run`.
  cancel: () => void;
};

// The page render lane: PAGE_RENDER_LIMIT rasters, queued by rank.
class PageLane {
  active = 0;
  readonly queue: QueuedRaster[] = [];
  private seq = 0;
  permitSerial = 0;
  readonly waiters = new Map<string, {
    canvasId: string;
    cancel: () => void;
    wake: () => void;
  }>();

  // Insert ahead of looser-ranked jobs, stable inside a rank.
  push(canvasId: string, rank: number, run: () => void, cancel: () => void): void {
    const job: QueuedRaster = { canvasId, rank, seq: this.seq++, run, cancel };
    this.queue.push(job);
    this.sort();
  }

  // A queued job's rank is a guess made when it was offered: the reader may
  // have reversed since. Move what the session now says.
  reprioritize(canvasId: string, rank: number): void {
    let moved = false;
    for (const job of this.queue) {
      if (job.canvasId === canvasId && job.rank !== rank) {
        job.rank = rank;
        moved = true;
      }
    }
    if (moved) this.sort();
  }

  // A queued job holds the page state and its canvas.
  // Settle it: callers await these.
  drop(canvasId: string): number {
    let dropped = 0;
    let keep = 0;
    for (let i = 0; i < this.queue.length; i += 1) {
      const job = this.queue[i];
      if (job.canvasId === canvasId) {
        dropped += 1;
        job.cancel();
        continue;
      }
      this.queue[keep] = job;
      keep += 1;
    }
    this.queue.length = keep;
    return dropped;
  }

  /** Harshest rank first, request order inside a rank. */
  private sort(): void {
    this.queue.sort((a, b) => (a.rank - b.rank) || (a.seq - b.seq));
  }

  /** Harshest rank first, request order inside a rank. */
  take(): (() => void) | null {
    const next = this.queue.shift();
    return next ? next.run : null;
  }
}

// Full-page rasters are main-thread work, so the cap is realm-wide.
export const REALM_PAGE_LIMIT = 2;
export const realmLane = { active: 0 };

// Sessions a freed realm slot may re-offer the lane to; weak on purpose.
const lanePumpSessions = new Set<WeakRef<EngineSession>>();

export function registerLanePump(s: EngineSession): () => void {
  const ref = new WeakRef(s);
  lanePumpSessions.add(ref);
  return () => {
    lanePumpSessions.delete(ref);
  };
}

// Walk the lane-pump registrants: pump the live ones, prune the rest.
export function pumpLaneRegistrants(pump: (s: EngineSession) => void): void {
  for (const ref of [...lanePumpSessions]) {
    const s = ref.deref();
    if (!s || s.disposed) {
      lanePumpSessions.delete(ref);
      continue;
    }
    pump(s);
  }
}

// The thumbnail lane and its prefetch bookkeeping, per session.
class ThumbLane {
  active = 0;
  readonly queue: Array<() => void> = [];
  readonly prefetchInFlight = new Set<number>();
  readonly generation = new Map<string, number>();
  open = false;
  epoch = 0;
  readonly epochWaiters: Array<() => void> = [];
  era = 0;
  suspended = false;
  readonly eraWaiters: Array<() => void> = [];
}

/** The raster-theme state a scrub leaves on ONE session's canvases. */
class ScrubState {
  lastBakedFingerprint: string | null = null;
  entryPrepare: Promise<void> | null = null;
  readonly entrySnapshots = new Map<string, HTMLCanvasElement>();
}

/** A raceable "world ended" promise: `unsubscribe` on every normal settle. */
export function worldEndedSignal(
  waiters: Array<() => void>,
  ended: boolean,
): { promise: Promise<void>; unsubscribe: () => void } {
  if (ended) return { promise: Promise.resolve(), unsubscribe: () => {} };
  let resolve!: () => void;
  const promise = new Promise<void>((r) => {
    resolve = r;
  });
  const waiter = () => resolve();
  waiters.push(waiter);
  return {
    promise,
    unsubscribe: () => {
      const at = waiters.indexOf(waiter);
      if (at >= 0) waiters.splice(at, 1);
    },
  };
}

// The engine's per-document state: proxy, surfaces, thumbs, theme.
export class EngineSession {
  /** The session id the Rust owner minted. Immutable, never reused. */
  readonly sid: number;
  /** Set by `retireSession`; every lane checks it before committing. */
  disposed = false;
  // Recent time-to-visible in ms: request, queue, raster slot, completion.
  // `0` before one lands.
  fillMs = 0;
  // Each canvas's current fill rank, so the lane can re-read it at dequeue.
  readonly rankByCanvas = new Map<string, number>();

  readonly pageLane = new PageLane();
  readonly thumbLane = new ThumbLane();
  readonly scrub = new ScrubState();
  // Each session bakes against the tokens on its own pane root.
  readonly themePipeline: PipelineCache = {
    token: null,
    inputs: null,
    filter: "none",
    blend: "normal",
    paperInfo: null,
    gen: 0,
  };
  // The first registered page pins the session to its pane's root.
  themeRoot: HTMLElement | null = null;
  /** Serialized theme mutations of THIS session's rasters. */
  themeChain: Promise<void> = Promise.resolve();
  // The realm registry's handle for this session's queue pump; null
  // unregistered.
  unregisterLanePump: (() => void) | null = null;

  // Raw frames parked for the paper session, and whether it wants them.
  readonly paperStash = new Map<string, PaperFrame>();
  paperActive = true;

  constructor(sid: number) {
    this.sid = sid;
  }

  loadingTask: LoadingTask | null = null;
  pdf: PDFDocumentProxy | null = null;
  numPages = 0;
  currentPath: string | null = null;

  // The document's dominant raster colour, or null until resolved.
  detectedPaper: string | null = null;

  // Live page surfaces by canvas id, bounded by the live window.
  readonly stateByCanvasId = new Map<string, PageState>();

  // Intrinsic (scale-1) page boxes, probed once per open.
  readonly intrinsicByPage = new Map<number, { width: number; height: number }>();

  /** Forget the probed page boxes: a new document is being opened. */
  clearIntrinsicSizes(): void {
    this.intrinsicByPage.clear();
  }

  /** LRU-capped thumbnail rasters (≤ THUMB_CACHE_MAX). */
  readonly thumbCache = new Map<number, ThumbEntry>();
  readonly thumbTasks = new Map<string, RenderTask>();
  readonly thumbCancelled = new Set<string>();
  readonly thumbLive = new Map<string, { page: number }>();

  /** Active query + current match for the DOM text-layer highlight pass. */
  searchQuery = "";
  activeMatch: ActiveMatch = null;

  // Heuristic sweep counter: every CLEANUP_EVERY renders, release caches.
  renderCount = 0;

  // Lifecycle counters, per session; writes also land in realm totals.
  readonly counts: Record<CounterKey, number> = zeroCounters();
  // Thumbnail prefetch gauge: must read zero after teardown.
  prefetchesActive = 0;

  themeScrubActive = false;

  // The last scrub-mode transition (Date.now()).
  lastScrubAt = 0;

  // Whether the appearance menu is open; it keeps a bake's raw.
  appearanceMenuOpen = false;

  private idleTimer: ReturnType<typeof setTimeout> | 0 = 0;
  private rawTimers = new WeakMap<PageState, ReturnType<typeof setTimeout>>();
  // Live raw-retention timers; teardown returns this to zero.
  private rawTimerCount = 0;

  setLoadingTask(t: LoadingTask | null): void {
    this.loadingTask = t;
  }

  setPdf(doc: PDFDocumentProxy | null): void {
    this.pdf = doc;
    this.documentAlive = doc !== null;
    if (!doc) this.setDetectedPaper(null); // document gone → re-detect on next open
  }

  /// The document's liveness as a waited-on flag.
  private documentAlive = false;
  private documentGoneWaiters: Array<() => void> = [];

  noteDocumentGone(): void {
    this.documentAlive = false;
    const waiters = this.documentGoneWaiters.splice(0);
    for (const wake of waiters) wake();
  }

  /// Subscribe to "this document is dying" (see `worldEndedSignal`).
  documentGoneSignal(): { promise: Promise<void>; unsubscribe: () => void } {
    return worldEndedSignal(this.documentGoneWaiters, !this.documentAlive);
  }

  /// Cancel the idle sweeper; `noteActivity` re-arms it.
  clearIdleTimer(): void {
    if (this.idleTimer) {
      clearTimeout(this.idleTimer);
      this.idleTimer = 0;
    }
  }

  // Whether the idle sweeper is armed (0/1 for stats).
  sweepTimerArmed(): number {
    return this.idleTimer ? 1 : 0;
  }

  /** Live raw-retention timers, for the stats surface. */
  rawRetentionTimers(): number {
    return this.rawTimerCount;
  }

  // Record this session's paper.
  setDetectedPaper(hex: string | null): void {
    this.detectedPaper = hex;
    if (publisher === this) writeRootPaper(hex);
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

  // Remember a scrub transition, so a bake inside the window keeps its raw.
  noteScrub(): void {
    this.lastScrubAt = Date.now();
  }

  // Open/close the menu's half of the retention gate.
  setAppearanceMenuOpen(on: boolean): void {
    if (this.appearanceMenuOpen === on) return;
    this.appearanceMenuOpen = on;
    if (on) return;
    for (const st of this.stateByCanvasId.values()) {
      if (!st.dead && st.rawCanvas && st.rawCanvas !== st.canvas) this.dropRawIfIdle(st);
    }
  }

  // Whether a tint scrub is plausible: a recent one, or an open menu.
  scrubIsPlausible(): boolean {
    if (this.appearanceMenuOpen) return true;
    return this.lastScrubAt > 0 && Date.now() - this.lastScrubAt < SCRUB_RAW_RETAIN_MS;
  }

  bumpRenderCount(): number {
    this.renderCount += 1;
    return this.renderCount;
  }

  // Reset the idle sweeper (pdf.cleanup + pool drain).
  noteActivity(): void {
    if (!this.pdf || this.disposed) return;
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
        releaseSnapshots(st.host);
      } catch (_) {
        /* host already detached */
      }
    }
    const rawTimer = this.rawTimers.get(st);
    if (rawTimer) {
      clearTimeout(rawTimer);
      this.rawTimerCount -= 1;
    }
    // Drop the dead handle so a second release cannot double-decrement.
    this.rawTimers.delete(st);
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

  // Drop the zoom masks every live host carries.
  sweepSnapshots(): void {
    for (const st of this.stateByCanvasId.values()) {
      if (!st.host) continue;
      try {
        releaseSnapshots(st.host);
      } catch (_) {
        /* host already detached */
      }
    }
  }

  // Keep the unbaked raw until a tint slider's window passes.
  dropRawIfIdle(st: PageState): void {
    const prev = this.rawTimers.get(st);
    if (prev) {
      clearTimeout(prev);
      this.rawTimerCount -= 1; // the replaced timer will never fire
    }
    this.rawTimerCount += 1;
    this.rawTimers.set(
      st,
      setTimeout(() => {
        this.rawTimerCount -= 1; // this timer just fired
        // Drop the dead handle to keep the mirror counter honest.
        this.rawTimers.delete(st);
        if (st.dead || this.themeScrubActive || this.appearanceMenuOpen) return;
        if (st.rawCanvas && st.rawCanvas !== st.canvas) releaseCanvas(st.rawCanvas);
        st.rawCanvas = null;
      }, RAW_IDLE_MS),
    );
  }
}

// The counter accessors (interface merge, so `s.x += 1` type-checks).
export interface EngineSession extends Record<CounterKey, number> {}
for (const key of COUNTER_KEYS) {
  Object.defineProperty(EngineSession.prototype, key, {
    get(this: EngineSession): number {
      return this.counts[key];
    },
    set(this: EngineSession, value: number) {
      realmCounters[key] += value - this.counts[key];
      this.counts[key] = value;
    },
  });
}

// The session registry: sid to session, plus the root-paper publisher.

const sessions = new Map<number, EngineSession>();
// The highest sid accepted; sids are monotonic, so none is reused.
let highestSid = 0;

// Register a new session, refusing a sid at or below one already seen.
export function createSession(sid: number): EngineSession | null {
  if (!Number.isInteger(sid) || sid <= highestSid) return null;
  highestSid = sid;
  const s = new EngineSession(sid);
  sessions.set(sid, s);
  lifecycleEvent("engine_session:create");
  return s;
}

/** The live session for `sid`, or null (unknown, retired, or not a sid). */
export function sessionFor(sid: unknown): EngineSession | null {
  if (typeof sid !== "number") return null;
  return sessions.get(sid) ?? null;
}

/** Every live session, in creation order. */
export function liveSessions(): EngineSession[] {
  return [...sessions.values()];
}

let sessionsRetired = 0;

// Sessions whose teardown has begun but not finished.
const draining = new Set<EngineSession>();

// Step one of retirement: stop accepting, advance `disposed`, drop the
// paper.
export function beginRetire(s: EngineSession): boolean {
  if (sessions.get(s.sid) !== s) return false;
  s.disposed = true;
  sessions.delete(s.sid);
  draining.add(s);
  // The root paper goes with a presenting session — only that session.
  releasePresentation(s);
  lifecycleEvent("engine_session:dispose_begin");
  return true;
}

/** Step two: the session's resources are released; forget it. */
export function finishRetire(s: EngineSession): void {
  if (!draining.delete(s)) return;
  s.themeRoot = null;
  s.themePipeline.token = null;
  s.themePipeline.inputs = null;
  s.themePipeline.paperInfo = null;
  sessionsRetired += 1;
  lifecycleEvent("engine_session:dispose_complete");
}

/** Live and draining sessions — what the aggregate gauges and the
 *  realm-level memory listeners walk. */
export function heldSessions(): EngineSession[] {
  return [...sessions.values(), ...draining];
}

/** Sessions created and retired so far (the registry's own pairing rule). */
export function registryCounts(): { live: number; retired: number } {
  return { live: sessions.size + draining.size, retired: sessionsRetired };
}

/** The session whose paper the root backdrop shows. */
let publisher: EngineSession | null = null;

// Presentation recency, most recent first: the split workspace's
// focus-blind fallback.
const presented: EngineSession[] = [];

export function paperPublisher(): EngineSession | null {
  return publisher;
}

// Make `s` the root-paper publisher and restate its paper.
export function setPaperPublisher(s: EngineSession | null): void {
  if (s && s.disposed) return;
  publisher = s;
  if (s) {
    const at = presented.indexOf(s);
    if (at >= 0) presented.splice(at, 1);
    presented.unshift(s);
  }
  // A presenting session with nothing detected yet holds the previous
  // colour.
  if (!s) writeRootPaper(null);
  else if (s.detectedPaper) writeRootPaper(s.detectedPaper);
}

// `s` is going away: forget its presentation; returns the fallback.
function releasePresentation(s: EngineSession): void {
  const at = presented.indexOf(s);
  if (at >= 0) presented.splice(at, 1);
  if (publisher !== s) return;
  publisher = null;
  while (presented.length > 0) {
    const next = presented[0];
    if (next && !next.disposed) {
      setPaperPublisher(next);
      return;
    }
    presented.shift();
  }
  writeRootPaper(null);
}

function writeRootPaper(hex: string | null): void {
  try {
    const el = document.documentElement;
    if (hex) el.style.setProperty("--pdf-paper", hex);
    else el.style.removeProperty("--pdf-paper");
  } catch (_) {
    /* no document */
  }
}

// Lifecycle diagnostics (Phase 0 baseline): counters over this module's
// resources, plus silent narration.

let lifecycleLog = false;

/** Turn the lifecycle event narration on/off (dev-only surface; the
 *  counters are always live regardless). */
export function setLifecycleLog(on: boolean): void {
  lifecycleLog = on;
}

/** Narrate one lifecycle event when logging is on. */
export function lifecycleEvent(name: string): void {
  if (lifecycleLog && typeof console !== "undefined") {
    console.info(`[lifecycle] ${name}`);
  }
}

/** Count one pdf.js worker coming into existence (a fresh LoadingTask). */
export function noteWorkerCreated(s: EngineSession): void {
  s.workersCreated += 1;
  lifecycleEvent("pdf_worker:create");
}

export const CLEANUP_EVERY = 5;
