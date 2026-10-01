// Mutable engine session state, one instance PER DOCUMENT SESSION. The
// facade (public/pdfEngine.ts) holds no document: every document call names
// a session id (`sid`), minted by the Rust `PdfSession` that owns it, and
// this module's registry is the only way from a sid to its state. A sid is
// accepted once, never reused, and dropped by `retireSession` — so a call
// captured against a disposed session finds nothing and does nothing. The
// maps below are window-bound (the virtualizer keeps `budget` pages live) or
// LRU-bounded (thumbCache <= THUMB_CACHE_MAX), so a session never grows with
// document length. docs/session-ownership.md is the ownership record.

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
import { releaseWarm, type WarmPage } from "./warm";
import { PAGE_SNAPSHOT_SELECTOR, TEXT_LAYER_SELECTOR } from "./dom-contract";

// The engine's own API version, served as `PDFReader.version()`. It tracks the
// JS surface rather than the app release, and unlike the six sources
// `tools/check-versions.ts` compares, nothing here checks it against them.
export const ENGINE_VERSION = "0.7.0";

/** Cap kept tight: each thumb is a pair of rasters. 16 keeps several
 *  scroll-windowfuls warm: ~8MB total (thumb pairs at 0.25 scale are small). */
export const THUMB_CACHE_MAX = 16;

/** Max pixels per canvas layer. The 12M base (~48 MB RGBA) is the ceiling,
 *  not the target: US-Letter at 100% zoom on a 2x display is ~1.5M px, at
 *  200% ~7.8M, so 12M keeps the FULL native devicePixelRatio through ~245%
 *  zoom — past where anyone is inspecting rather than reading — while every
 *  transient surface a zoom commit stacks (scratch, bake output, snapshot
 *  mask) is a quarter smaller than the 16M this used to be. The footprint
 *  latches onto the session's dirty high-water mark and never hands it back,
 *  so the cheapest megabyte is the one a transient never allocates; total
 *  GPU memory is bounded by the 3-page mounted ceiling (RENDER_BUDGET), not
 *  by this. The ceiling used to DOUBLE on machines reporting >= 8 GB; that
 *  bought no visible sharpness and made every transient twice the cost,
 *  permanently. Low-memory devices still get half the base. */
const PAGE_MAX_PIXELS_BASE = 12 * 1024 * 1024;

function memoryScaledPixelCeiling(): number {
  // Guarded end to end: `navigator` is absent in the Node smoke harness and
  // in old webview sandboxes, and `deviceMemory` is Chromium-only — both
  // must fall back to the base ceiling without throwing at module load.
  const nav = typeof navigator !== "undefined" ? (navigator as { deviceMemory?: number }) : undefined;
  const memory = nav && nav.deviceMemory;
  if (typeof memory !== "number" || !(memory > 0)) return PAGE_MAX_PIXELS_BASE;
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

/** Drop every scrub cover (`.page-snapshot`) a host still carries, zeroing the
 *  backing stores before the nodes go: WKWebView does not release a canvas
 *  IOSurface on DOM removal alone, so a cover dropped without this keeps its
 *  full-page RGBA buffer alive. */
function releaseSnapshots(host: HTMLElement): void {
  host.querySelectorAll(PAGE_SNAPSHOT_SELECTOR).forEach((n) => {
    releaseCanvas(n as HTMLCanvasElement);
    n.remove();
  });
}

/** The lifecycle counters every session keeps (see EngineSession). */
export const COUNTER_KEYS = [
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

/** The page render lane: at most PAGE_RENDER_LIMIT rasters of THIS session
 *  in flight, the rest queued FIFO. Per session, so one pane's burst never
 *  queues behind another pane's pages and one pane's teardown drains only
 *  its own queue. */
export class PageLane {
  active = 0;
  readonly queue: Array<() => void> = [];
}

/** Full-page rasters are MAIN-THREAD work — pdf.js draws the page into the
 *  canvas synchronously; only parsing/decoding runs in pdf.js's worker — so
 *  their concurrency cap is realm-wide, not per session: four panes
 *  rasterising or re-theming together queue as one paced sweep instead of
 *  stacking up to eight concurrent stalls on the one thread. A single pane
 *  sees the same two slots it always had. The per-session QUEUE (and its
 *  drain on teardown) stays — the realm cap only decides when a queued job
 *  may start. */
export const REALM_PAGE_LIMIT = 2;
export const realmLane = { active: 0 };

/** Sessions with a page queue a freed realm slot should re-offer the lane
 *  to. Held by WEAK REFERENCE on purpose: this registry is module state
 *  that outlives any one session, and a strong handle here would pin the
 *  whole EngineSession — page surfaces, thumb cache, pdf proxy — past its
 *  teardown if an unregister were ever skipped. The renderer walks the
 *  refs and prunes any whose session is gone (collected) or retired
 *  (`disposed`), so a stale entry cannot even receive a pump. */
const lanePumpSessions = new Set<WeakRef<EngineSession>>();

export function registerLanePump(s: EngineSession): () => void {
  const ref = new WeakRef(s);
  lanePumpSessions.add(ref);
  return () => {
    lanePumpSessions.delete(ref);
  };
}

/** Walk the lane-pump registrants: `pump` every session still alive and
 *  not yet retired, prune the refs that are not. The renderer supplies the
 *  pump (its own `pumpPageQueue`); this module must not import it. */
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

/** The thumbnail lane and its prefetch bookkeeping, per session: the lane
 *  epoch (bumped by this session's teardown), the prefetch era (bumped by
 *  this pane's suspend), and the per-canvas generations. */
export class ThumbLane {
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
export class ScrubState {
  lastBakedFingerprint: string | null = null;
  entryPrepare: Promise<void> | null = null;
  readonly entrySnapshots = new Map<string, HTMLCanvasElement>();
}

/** The engine's per-document session state: the pdf.js document proxy, live
 *  page surfaces, thumbnail cache, search context, and theme pipeline state.
 *  One instance per document session, created by `createSession(sid)` and
 *  retired by the facade's `destroySession(sid)`. */
export class EngineSession {
  /** The session id the Rust owner minted. Immutable, never reused. */
  readonly sid: number;
  /** Set by `retireSession`; every lane checks it before committing. */
  disposed = false;

  readonly pageLane = new PageLane();
  readonly thumbLane = new ThumbLane();
  readonly scrub = new ScrubState();
  /** Each PDF session bakes against the appearance tokens on its own pane root. */
  readonly themePipeline: PipelineCache = {
    token: null,
    inputs: null,
    filter: "none",
    blend: "normal",
    paperInfo: null,
    gen: 0,
  };
  /** The first registered page pins this session to its pane's appearance root. */
  themeRoot: HTMLElement | null = null;
  /** Serialized theme mutations of THIS session's rasters. */
  themeChain: Promise<void> = Promise.resolve();
  /** The realm lane registry's handle for this session's page-queue pump;
   *  set when the session registers with the engine, cleared by its
   *  destroy. Null for a session nobody registered (the host's stubs). */
  unregisterLanePump: (() => void) | null = null;

  /** Raw frames parked for the Rust paper session (engine/paper.ts), and
   *  whether that session wants them (its blend switch). */
  readonly paperStash = new Map<string, PaperFrame>();
  paperActive = true;

  constructor(sid: number) {
    this.sid = sid;
  }

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
  /** Downscaled copies of settled pages that left the window (warm.ts owns
   *  the bound, WARM_PAGE_MAX, and the drains). */
  readonly warmPages = new Map<number, WarmPage>();
  readonly thumbTasks = new Map<string, RenderTask>();
  readonly thumbCancelled = new Set<string>();
  readonly thumbLive = new Map<string, { page: number }>();

  /** Active query + current match for the DOM text-layer highlight pass. */
  searchQuery = "";
  activeMatch: ActiveMatch = null;

  /** Heuristic sweep counter: every CLEANUP_EVERY renders, release worker
   *  caches so memory drops during long reading sessions. */
  renderCount = 0;

  // Lifecycle counters (Phase 0 diagnostics), per session. Monotonic; read
  // via stats(). They are accessors (installed below from COUNTER_KEYS):
  // every write lands in this session's record AND in the realm totals in
  // the same statement, so the aggregate never loses an increment — not
  // even one a late settle makes after the session retired. The pairing
  // rules the teardown baseline asserts:
  //   sessionsOpened   == sessionsDestroyed   (every open document dies once)
  //   workersCreated   == workersTerminated   (every LoadingTask destroyed once)
  //   rendersStarted   == rendersCompleted + rendersCancelled + rendersFailed
  //   prefetchesStarted == prefetchesCompleted + prefetchesDropped
  readonly counts: Record<CounterKey, number> = zeroCounters();
  // Thumbnail prefetch gauge: must read zero after teardown.
  prefetchesActive = 0;

  themeScrubActive = false;

  /** The last scrub-mode transition (Date.now()), recorded by the theme
   *  queue on the way in AND out. A bake retains its unbaked raw only while
   *  a scrub inside SCRUB_RAW_RETAIN_MS of this is plausible. */
  lastScrubAt = 0;

  /** Whether the appearance popover is open, told by the app over the
   *  bridge (`setAppearanceMenuOpen`). The menu is where a scrub is born:
   *  while it is open, a bake retains its unbaked raw even with no recent
   *  scrub, so the FIRST drag of a session blits retained pixels under the
   *  live CSS instead of re-rendering every page; closing arms the idle
   *  tail that frees them. */
  appearanceMenuOpen = false;

  private idleTimer: ReturnType<typeof setTimeout> | 0 = 0;
  private rawTimers = new WeakMap<PageState, ReturnType<typeof setTimeout>>();
  /** Live raw-retention timers (armed, unfired, uncleared). The WeakMap
   *  above is uncountable by design; this mirror counter is what the stats
   *  surface reads, and teardown must return it to zero. */
  private rawTimerCount = 0;

  setLoadingTask(t: LoadingTask | null): void {
    this.loadingTask = t;
  }

  setPdf(doc: PDFDocumentProxy | null): void {
    this.pdf = doc;
    this.documentAlive = doc !== null;
    if (!doc) this.setDetectedPaper(null); // document gone → re-detect on next open
  }

  /// The document's liveness as a waited-on flag, not just a field: an
  /// await inside the destroy window (a task born after the cancel sweep
  /// but before the document nulls) sits on a worker that never answers,
  /// so the awaits race this instead of trusting the promise. Set false by
  /// `noteDocumentGone` the moment a destroy BEGINS — by completion would
  /// be too late for those awaits — and true again by the next open.
  private documentAlive = false;
  private documentGoneWaiters: Array<() => void> = [];

  noteDocumentGone(): void {
    this.documentAlive = false;
    const waiters = this.documentGoneWaiters.splice(0);
    for (const wake of waiters) wake();
  }

  /// Subscribe to "this document is dying". The unsubscribe is the leak
  /// guard: a prefetch that settles NORMALLY (the usual case) must remove
  /// its waiter, or every successful prefetch leaves a resolver parked here
  /// for the document's whole lifetime — exactly the async bookkeeping
  /// growth a memory baseline exists to catch.
  documentGoneSignal(): { promise: Promise<void>; unsubscribe: () => void } {
    if (!this.documentAlive) {
      return { promise: Promise.resolve(), unsubscribe: () => {} };
    }
    let resolve!: () => void;
    const promise = new Promise<void>((r) => {
      resolve = r;
    });
    const waiter = () => resolve();
    this.documentGoneWaiters.push(waiter);
    return {
      promise,
      unsubscribe: () => {
        const at = this.documentGoneWaiters.indexOf(waiter);
        if (at >= 0) this.documentGoneWaiters.splice(at, 1);
      },
    };
  }

  /// Cancel the idle sweeper: a close must not leave a document-scoped
  /// timer that later fires `sweepPdf` over whatever document is open by
  /// then. `noteActivity` re-arms it for the living document.
  clearIdleTimer(): void {
    if (this.idleTimer) {
      clearTimeout(this.idleTimer);
      this.idleTimer = 0;
    }
  }

  /** Whether the document-scoped idle sweeper is armed (0/1 for stats).
   *  Destroy cancels the timer, so the baseline requires 0 after a close. */
  sweepTimerArmed(): number {
    return this.idleTimer ? 1 : 0;
  }

  /** Live raw-retention timers, for the stats surface. */
  rawRetentionTimers(): number {
    return this.rawTimerCount;
  }

  /** Record this session's paper. The realm publisher updates the shared
   *  fallback; pane-local publication is handled by theme/paper.ts. */
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

  /** Remember a scrub transition: bakes landing inside the retention window
   *  keep their unbaked raw so the next drag restores without a re-render. */
  noteScrub(): void {
    this.lastScrubAt = Date.now();
  }

  /** Open/close the appearance menu's half of the retention gate. Closing
   *  re-arms the idle timer on every raw the open menu was holding, so the
   *  surfaces leave on the same short tail a scrub's raws do — the flag
   *  alone would strand them until the next bake or teardown. */
  setAppearanceMenuOpen(on: boolean): void {
    if (this.appearanceMenuOpen === on) return;
    this.appearanceMenuOpen = on;
    if (on) return;
    for (const st of this.stateByCanvasId.values()) {
      if (!st.dead && st.rawCanvas && st.rawCanvas !== st.canvas) this.dropRawIfIdle(st);
    }
  }

  /** Whether a tint scrub is plausible right now — the retention gate for
   *  the unbaked raw a bake just produced. An open appearance menu counts
   *  on its own: the dials are on screen, so a drag can start with no
   *  scrub ever having happened this session. Zero means "never scrubbed
   *  this session", which alone is not plausible. */
  scrubIsPlausible(): boolean {
    if (this.appearanceMenuOpen) return true;
    return this.lastScrubAt > 0 && Date.now() - this.lastScrubAt < SCRUB_RAW_RETAIN_MS;
  }

  bumpRenderCount(): number {
    this.renderCount += 1;
    return this.renderCount;
  }

  /**
   * Reset the idle sweeper (pdf.cleanup + scratch/pool drain).
   *
   * Document-scoped by definition: with no document there is nothing left to
   * sweep, and a timer armed here would outlive the close. `destroySession` clears
   * the sweeper, but it cannot un-arm a render still resuming from an await —
   * and `sweepTimerArmed` is a field the teardown baseline reads, so a stray
   * re-arm is a close that never reads drained.
   */
  noteActivity(): void {
    if (!this.pdf || this.disposed) return;
    if (this.idleTimer) clearTimeout(this.idleTimer);
    this.idleTimer = setTimeout(() => {
      this.sweepPdf();
      releaseWarm(this);
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
    // Delete the (now dead) handle so a second release of the same state
    // cannot decrement the mirror counter twice.
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

  /** Drop the zoom masks every live host still carries — a mask whose render
   *  was superseded, or never landed, keeps a full-page RGBA surface alive
   *  until the host unmounts. The app-side `remove_snapshots` clears a host
   *  when ITS render completes; this is the engine-side net for the hosts
   *  whose completion never came. Fired where reading work ends, alongside
   *  `sweepPdf`. */
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

  /** Keep the unbaked raw briefly so a tint slider can restore it, then free
   *  it. The next theme change / scrub without a raw re-renders from pdf.js.
   *  The timer is a no-op while scrubbing is active or the appearance menu
   *  is open (both are the raw's reason to exist; the menu's close re-arms
   *  this), and teardown (releasePageSurfaces) clears it outright. */
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
        // Drop the dead handle so a later release of the same state cannot
        // decrement the mirror counter a second time.
        this.rawTimers.delete(st);
        if (st.dead || this.themeScrubActive || this.appearanceMenuOpen) return;
        if (st.rawCanvas && st.rawCanvas !== st.canvas) releaseCanvas(st.rawCanvas);
        st.rawCanvas = null;
      }, RAW_IDLE_MS),
    );
  }
}

// The counter accessors (declared through the interface merge so
// `s.rendersStarted += 1` type-checks like a plain field).
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

// ---------------------------------------------------------------------------
// The session registry. The only realm-level document structure left: a map
// from sid to its session, plus which session publishes the root paper.

const sessions = new Map<number, EngineSession>();
/** The highest sid ever accepted: sids are monotonic, so a retired sid can
 *  never be registered again and a stale caller can never alias a new
 *  session by reusing its number. */
let highestSid = 0;

/** Register a new session. Refuses a sid that is not a positive integer
 *  above every sid seen so far (reuse is how a stale call would alias a
 *  live session). */
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

/** Sessions whose teardown has begun but not finished: no longer reachable
 *  by sid (nothing new is accepted), still counted by the aggregate gauges
 *  until their worker and surfaces are gone. */
const draining = new Set<EngineSession>();

/** Step one of retirement, the first act of destroySession: stop accepting
 *  (the sid stops resolving) and advance the invalidation state every lane
 *  checks (`disposed`). The root paper goes with a publishing session. */
export function beginRetire(s: EngineSession): boolean {
  if (sessions.get(s.sid) !== s) return false;
  s.disposed = true;
  sessions.delete(s.sid);
  draining.add(s);
  // The root paper goes with a presenting session — but only that session:
  // another open document's colour takes over the backdrop (the split
  // workspace's focus-blind fallback), and only an empty realm clears it.
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

/** Presentation recency: the sessions that have presented their paper, most
 *  recent first. The focus-blind fallback the split workspace needs — when
 *  the publisher's session goes away while other documents are still open
 *  (the focus sits on a reflowable pane and cannot present), the backdrop
 *  falls back to the most recently presented live session's colour instead
 *  of dropping to the theme paper. Bounded by the live session count;
 *  entries leave in `beginRetire` and dead ones prune on every fallback. */
const presented: EngineSession[] = [];

export function paperPublisher(): EngineSession | null {
  return publisher;
}

/** Make `s` the root-paper publisher (null = nobody) and restate its paper.
 *  The latest document to open presents by default; the host can name a
 *  session explicitly (`presentSession`). A presentation also records the
 *  recency the destroy-time fallback walks. */
export function setPaperPublisher(s: EngineSession | null): void {
  if (s && s.disposed) return;
  publisher = s;
  if (s) {
    const at = presented.indexOf(s);
    if (at >= 0) presented.splice(at, 1);
    presented.unshift(s);
  }
  // A presenting session with nothing detected yet HOLDS the previous
  // colour: a fresh open beside a coloured workspace must not flash the
  // backdrop to the theme paper — the session's first detected colour
  // lands the swap. A null publisher is deliberate: clear.
  if (!s) writeRootPaper(null);
  else if (s.detectedPaper) writeRootPaper(s.detectedPaper);
}

/** `s` is going away: forget its presentation. When it was presenting, the
 *  most recently presented live session takes over the backdrop — or the
 *  paper clears, when no other document is open. Returns the fallback (or
 *  null) so the caller's `publishBakedPaper` lands on the new publisher. */
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

// ---------------------------------------------------------------------------
// Lifecycle diagnostics (Phase 0 baseline).
//
// Counters over the resources THIS module owns: the document session, the
// pdf.js worker behind its loading task, and the page render lane. They are
// the production signal — cheap, always on, read through `stats()` — and the
// open/teardown paths bump them where the resource is actually created or
// released, so a teardown that misses a resource is visible as an unbalanced
// pair rather than as a claim.
//
// `lifecycleEvent` is the narration half: silent unless a development
// surface opts in (`setLifecycleLog`), because create/dispose events are
// rare but render events are not, and per-render logs are noise in normal
// operation.

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
