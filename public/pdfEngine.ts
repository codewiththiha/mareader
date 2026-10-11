// window.PDFReader facade over public/engine/*, compiled to pdfEngine.js.

export {};

import type { AggregateStats, PDFReaderApi, Sid, Stats } from "./engine/types";
import {
  disposeScratch,
  pooledIntermediateBytesEstimate,
  releaseCanvas,
} from "./engine/canvas";
import { fail } from "./engine/errors";
import { coverDataUrl, destroyTask, open, resolveOutline } from "./engine/loader";
import {
  beginRenderGeneration,
  cancelPage,
  cancelPageRenders,
  drainPageLane,
  pageLaneGauge,
  probePageSize,
  readRenderTrace,
  registerPage,
  renderPage,
  reprioritizePage,
  rerenderLivePages,
  unregisterPage,
} from "./engine/renderer";
import {
  cancelThumb,
  hasThumb,
  thumbGenerationSize,
  thumbLaneGauge,
  prefetchThumb,
  renderThumb,
  resetThumbLane,
  resumePrefetches,
  suspendPrefetches,
} from "./engine/thumbnails";
import {
  clearHighlights,
  extractPageText,
  setActiveMatch,
  setSearchContext,
} from "./engine/search";
import { rebakeTheme, releaseAllEntrySnapshots, setScrubModeInternal } from "./engine/theme/scrub";
import { releaseBakeWorker } from "./engine/theme/bake";
import { publishBakedPaper, unobserveThemeRoot, watchPaperTokens } from "./engine/theme/paper";
import { invalidatePipeline, readPipeline } from "./engine/theme/pipeline";
import { paintAllVisibleThumbs } from "./engine/theme/thumbnails";
import {
  resetPaperForDocument,
  samplePaperPage,
  setPaper,
  setPaperActive,
  takePaperFrame,
} from "./engine/paper";
import {
  beginRetire,
  createSession,
  ENGINE_VERSION,
  finishRetire,
  heldSessions,
  lifecycleEvent,
  liveSessions,
  realmCounters,
  registerLanePump,
  registryCounts,
  sessionFor,
  setLifecycleLog,
  setPaperPublisher,
  THUMB_CACHE_MAX,
} from "./engine/state";
import type { CounterKey, EngineSession } from "./engine/state";

declare global {
  interface Window {
    pdfjsLib: unknown;
  }
  var pdfjsLib: unknown;
  var __TAURI__:
    | {
        core: {
          invoke: (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;
          convertFileSrc: (p: string) => string;
        };
      }
    | undefined;
  var PDFReader: PDFReaderApi;
}

// The envelope a call resolves for a sid with no live session.
function noSession(): { ok: false; error: { name: string; message: string } } {
  return fail("no_session", "No live PDF session with this id");
}

// Cancel and release every live page surface of `s`.
function cancelAndReleasePages(s: EngineSession): void {
  for (const st of s.stateByCanvasId.values()) {
    st.dead = true;
    try { st.renderTask && st.renderTask.cancel(); } catch (_) { /* ignore */ }
    try { st.textLayer && st.textLayer.cancel(); } catch (_) { /* ignore */ }
    s.releasePageSurfaces(st);
  }
}

// The work-stop half for one session; destroySession stays the teardown.
function quiesce(s: EngineSession): void {
  if (s.pdf === null) return;
  lifecycleEvent("pdf_session:quiesce");
  s.noteDocumentGone();
  s.clearIdleTimer();
  cancelAndReleasePages(s);
  drainPageLane(s);
  for (const task of s.thumbTasks.values()) {
    try { task.cancel(); } catch (_) { /* ignore */ }
  }
  s.thumbTasks.clear();
  resetThumbLane(s);
}

// Tear ONE session down: stop accepting, cancel work, release, forget.
async function destroySession(sid: Sid): Promise<void> {
  const s = sessionFor(sid);
  if (!s || !beginRetire(s)) return;
  // Disconnect the pane-root observer before any async release.
  unobserveThemeRoot(s);
  // The sid no longer resolves: unregister from the realm lane first.
  if (s.unregisterLanePump) {
    s.unregisterLanePump();
    s.unregisterLanePump = null;
  }
  const hadDocument = s.pdf !== null;
  if (hadDocument) {
    s.sessionsDestroyed += 1;
    lifecycleEvent("pdf_session:dispose_begin");
  }
  s.noteDocumentGone();
  s.clearIdleTimer();
  try {
    s.sweepPdf();
    cancelAndReleasePages(s);
    drainPageLane(s);
    s.stateByCanvasId.clear();
    // The ranks outlive the states they name unless they are told otherwise.
    s.rankByCanvas.clear();
    for (const task of s.thumbTasks.values()) {
      try { task.cancel(); } catch (_) { /* ignore */ }
    }
    s.thumbTasks.clear();
    s.thumbCancelled.clear();
    s.thumbLive.clear();
    resetThumbLane(s);
    for (const entry of s.thumbCache.values()) s.releaseThumbEntry(entry);
    s.thumbCache.clear();
    s.setSearchQuery("");
    s.setActiveMatchValue(null);
    // Zero and remove the entry snapshots; a canvas store lingers till GC.
    releaseAllEntrySnapshots(s);
    s.scrub.entryPrepare = null;
    if (s.loadingTask) await destroyTask(s, s.loadingTask);
  } finally {
    s.clearIdleTimer();
    s.setPdf(null);
    s.setNumPages(0);
    s.setCurrentPath(null);
    resetPaperForDocument(s);
    publishBakedPaper();
    disposeScratch();
    if (hadDocument) lifecycleEvent("pdf_session:dispose_complete");
    finishRetire(s);
    // The realm's last session is gone: the bake worker has nobody.
    if (registryCounts().live === 0) releaseBakeWorker();
  }
}

// The appearance broadcast: a change enqueues on every live session.

// The appearance state a session opening mid-drag must start in.
let appearanceScrub = false;
/** Whether the scrub in flight edits one pane only (see `scrubScope`). */
let appearanceScrubScoped = false;
let appearanceMenuOpen = false;

function enqueueTheme(s: EngineSession, work: () => Promise<void>): Promise<void> {
  const guarded = () => (s.disposed ? Promise.resolve() : work());
  s.themeChain = s.themeChain
    .then(guarded, guarded)
    .catch((e: unknown) => {
      const msg = (e as { message?: string })?.message ?? e;
      console.warn("[pdfEngine] theme mutation failed:", msg);
    });
  return s.themeChain;
}

async function refreshSessionTheme(s: EngineSession): Promise<void> {
  if (s.themeScrubActive) {
    publishBakedPaper(s);
    return;
  }
  await rebakeTheme(s);
  let needsRerender = false;
  for (const st of s.stateByCanvasId.values()) {
    if (!st.dead && st.canvas && (!st.rawCanvas || st.rawCanvas === st.canvas)) {
      needsRerender = true;
      break;
    }
  }
  if (needsRerender) await rerenderLivePages(s);
  const thumbJobs: Promise<unknown>[] = [];
  for (const [canvasId, { page }] of s.thumbLive) {
    const entry = s.thumbCache.get(page);
    if (!entry || !entry.display || (entry.display as ImageBitmap).width <= 0) {
      thumbJobs.push(renderThumb(s, canvasId, page, entry?.scale || 0.25));
    }
  }
  if (thumbJobs.length) await Promise.all(thumbJobs);
  paintAllVisibleThumbs(s);
}

// Re-bake only the sessions whose bake inputs moved since last refresh.
function refreshTheme(): Promise<void> {
  const held = liveSessions();
  if (held.length === 0) {
    publishBakedPaper();
    return Promise.resolve();
  }
  const moved = held.filter((session) => {
    const before = refreshedGen.get(session) ?? session.themePipeline.gen;
    invalidatePipeline(session);
    const gen = readPipeline(session).gen;
    refreshedGen.set(session, gen);
    return gen !== before || session.themeScrubActive;
  });
  return Promise.all(moved.map((s) => enqueueTheme(s, () => refreshSessionTheme(s)))).then(
    () => undefined,
  );
}

// The pipeline generation each session was last refreshed at; weak.
const refreshedGen = new WeakMap<EngineSession, number>();

// The pane a scrub is scoped to.
function scrubScope(): string | null {
  try {
    return document.documentElement.getAttribute("data-appearance-scope");
  } catch (_) {
    return null;
  }
}

function inScrubScope(s: EngineSession, scope: string | null): boolean {
  if (!scope) return true;
  const entry = s.themeRoot?.closest("[data-pane-id]");
  return entry?.getAttribute("data-pane-id") === scope;
}

/** Pane roots carrying the scoped scrub class, cleared on the way out. */
let scopedScrubRoots: HTMLElement[] = [];

function setScrubMode(on: boolean): Promise<void> {
  appearanceScrub = on;
  const root = (() => {
    try {
      return document.documentElement;
    } catch (_) {
      return null;
    }
  })();
  let held = liveSessions();
  if (on) {
    const scope = scrubScope();
    appearanceScrubScoped = scope !== null;
    held = held.filter((s) => inScrubScope(s, scope));
    if (scope === null) {
      root?.classList.add("appearance-scrubbing");
    } else {
      scopedScrubRoots = held.flatMap((s) => (s.themeRoot ? [s.themeRoot] : []));
      for (const el of scopedScrubRoots) el.classList.add("appearance-scrubbing");
    }
  }
  // Leaving visits every session: one that never entered returns at once.
  const jobs = held.map((s) => enqueueTheme(s, () => setScrubModeInternal(s, on)));
  return Promise.all(jobs).then(() => {
    // The class leaves once every session has settled out of the scrub.
    if (!on && !appearanceScrub) {
      root?.classList.remove("appearance-scrubbing");
      for (const el of scopedScrubRoots) el.classList.remove("appearance-scrubbing");
      scopedScrubRoots = [];
    }
  });
}

// Not enqueued: a retention flag, not a canvas mutation.
function setAppearanceMenuOpen(on: boolean): void {
  appearanceMenuOpen = on;
  for (const s of liveSessions()) s.setAppearanceMenuOpen(on);
}

// ---------------------------------------------------------------------------
// Session lifecycle.

function createEngineSession(sid: Sid): boolean {
  const s = createSession(sid);
  if (!s) return false;
  // The realm lane registry; weak, so it cannot outlive the session.
  s.unregisterLanePump = registerLanePump(s);
  // A session opening mid-scrub joins a window-wide scrub.
  if (appearanceScrub && !appearanceScrubScoped) {
    s.setThemeScrubActive(true);
    s.noteScrub();
  }
  s.appearanceMenuOpen = appearanceMenuOpen;
  return true;
}

async function openInSession(sid: Sid, path: string) {
  const s = sessionFor(sid);
  if (!s) return noSession();
  // A PDF session holds exactly one document.
  if (s.pdf || s.loadingTask) {
    return fail("session_in_use", "This PDF session already holds a document");
  }
  const result = await open(s, path);
  if (result.ok && !s.disposed) {
    // The latest document to open presents its paper on the root backdrop.
    setPaperPublisher(s);
    publishBakedPaper();
  }
  return result;
}

function presentSession(sid: Sid): void {
  const s = sessionFor(sid);
  if (!s) return;
  setPaperPublisher(s);
  publishBakedPaper();
}

// ---------------------------------------------------------------------------
// Stats.

/** One session's gauges. The counters come from the caller. */
function gauges(s: EngineSession): Omit<Stats, CounterKey> {
  let activeRenders = 0;
  let pageCanvasBytes = 0;
  let rawRetentionBytes = 0;
  for (const st of s.stateByCanvasId.values()) {
    if (st.renderTask) activeRenders += 1;
    if (st.canvas) pageCanvasBytes += st.canvas.width * st.canvas.height * 4;
    if (st.rawCanvas && st.rawCanvas !== st.canvas) {
      rawRetentionBytes += st.rawCanvas.width * st.rawCanvas.height * 4;
    }
  }
  let thumbnailRasterBytes = 0;
  for (const t of s.thumbCache.values()) {
    for (const c of [t.raw, t.display]) {
      if (c) thumbnailRasterBytes += c.width * c.height * 4;
    }
  }
  const pageLane = pageLaneGauge(s);
  const thumbLane = thumbLaneGauge(s);
  return {
    fillMs: s.fillMs,
    pageLimit: pageLane.pageLimit,
    pageQueue: pageLane.pageQueue,
    pageActive: pageLane.pageActive,
    thumbQueue: thumbLane.thumbQueue,
    thumbActive: thumbLane.thumbActive,
    pages: s.stateByCanvasId.size,
    thumbs: s.thumbCache.size,
    thumbLimit: THUMB_CACHE_MAX,
    thumbTasks: s.thumbTasks.size,
    activeRenders,
    activePrefetches: s.prefetchesActive,
    hasDocument: s.pdf !== null,
    hasLoadingTask: s.loadingTask !== null,
    documentPages: s.numPages,
    thumbGenerationSize: thumbGenerationSize(s),
    rawRetentionTimers: s.rawRetentionTimers(),
    sweepTimerArmed: s.sweepTimerArmed(),
    pageCanvasBytesEst: pageCanvasBytes,
    thumbnailRasterBytesEst: thumbnailRasterBytes,
    rawRetentionBytesEst: rawRetentionBytes,
    pooledIntermediateBytesEst: pooledIntermediateBytesEstimate(),
  };
}

function sessionStats(sid: Sid): Stats | null {
  const s = sessionFor(sid);
  return s ? { ...gauges(s), ...s.counts } : null;
}

// The realm aggregate over every session that ever lived.
function stats(): AggregateStats {
  const out: AggregateStats = {
    fillMs: 0,
    pageLimit: 0,
    pageQueue: 0,
    pageActive: 0,
    thumbQueue: 0,
    thumbActive: 0,
    pages: 0,
    thumbs: 0,
    thumbLimit: THUMB_CACHE_MAX,
    thumbTasks: 0,
    activeRenders: 0,
    activePrefetches: 0,
    hasDocument: false,
    hasLoadingTask: false,
    documentPages: 0,
    thumbGenerationSize: 0,
    rawRetentionTimers: 0,
    sweepTimerArmed: 0,
    pageCanvasBytesEst: 0,
    thumbnailRasterBytesEst: 0,
    rawRetentionBytesEst: 0,
    pooledIntermediateBytesEst: pooledIntermediateBytesEstimate(),
    ...realmCounters,
    sessionsLive: 0,
    sessionsRetired: 0,
  };
  for (const s of heldSessions()) {
    const g = gauges(s);
    // Per-session figures: max across panes, never a sum.
    out.fillMs = Math.max(out.fillMs, g.fillMs);
    out.pageLimit = Math.max(out.pageLimit, g.pageLimit);
    out.pageQueue += g.pageQueue;
    out.pageActive += g.pageActive;
    out.thumbQueue += g.thumbQueue;
    out.thumbActive += g.thumbActive;
    out.pages += g.pages;
    out.thumbs += g.thumbs;
    out.thumbTasks += g.thumbTasks;
    out.activeRenders += g.activeRenders;
    out.activePrefetches += g.activePrefetches;
    out.hasDocument = out.hasDocument || g.hasDocument;
    out.hasLoadingTask = out.hasLoadingTask || g.hasLoadingTask;
    out.documentPages += g.documentPages;
    out.thumbGenerationSize += g.thumbGenerationSize;
    out.rawRetentionTimers += g.rawRetentionTimers;
    out.sweepTimerArmed += g.sweepTimerArmed;
    out.pageCanvasBytesEst += g.pageCanvasBytesEst;
    out.thumbnailRasterBytesEst += g.thumbnailRasterBytesEst;
    out.rawRetentionBytesEst += g.rawRetentionBytesEst;
  }
  const counts = registryCounts();
  out.sessionsLive = counts.live;
  out.sessionsRetired = counts.retired;
  return out;
}

// Realm memory listeners: each walks every held session.

// Release every surface of every session, on `pagehide`.
function releaseAllSurfaces(): void {
  for (const s of heldSessions()) {
    cancelAndReleasePages(s);
    for (const entry of s.thumbCache.values()) s.releaseThumbEntry(entry);
  }
  try {
    document.querySelectorAll("canvas").forEach((c) => releaseCanvas(c as HTMLCanvasElement));
  } catch (_) { /* document already torn down */ }
  disposeScratch();
}

globalThis.addEventListener("pagehide", releaseAllSurfaces);
try {
  globalThis.addEventListener("visibilitychange", () => {
    if (document.visibilityState !== "hidden") return;
    for (const s of heldSessions()) {
      for (const st of s.stateByCanvasId.values()) {
        if (st.rawCanvas && st.rawCanvas !== st.canvas) {
          releaseCanvas(st.rawCanvas);
          st.rawCanvas = null;
        }
      }
    }
    disposeScratch();
  });
} catch (_) {
  /* no document */
}

// The selection tracker lives in the reader bundle, not here.

// The standing token watch over the backdrop paper's inputs.
watchPaperTokens();

// The surface: session entries resolve their sid first.

function withSession<T>(sid: Sid, missing: T, run: (s: EngineSession) => T): T {
  const s = sessionFor(sid);
  return s ? run(s) : missing;
}

globalThis.PDFReader = {
  version: () => ENGINE_VERSION,
  beginRenderGeneration,
  renderTrace: readRenderTrace,
  setLifecycleLog,

  createSession: createEngineSession,
  destroySession,
  presentSession,
  sessions: () => liveSessions().map((s) => s.sid),

  open: openInSession,
  resolveOutline: (sid) =>
    withSession(sid, Promise.resolve({ ok: true as const, outline: [] }), (s) => resolveOutline(s)),
  registerPage: (sid, page, canvasId, hostId, canvas, host) =>
    withSession(sid, undefined, (s) => registerPage(s, page, canvasId, hostId, canvas, host)),
  unregisterPage: (sid, canvasId) => withSession(sid, undefined, (s) => unregisterPage(s, canvasId)),
  cancelPage: (sid, canvasId) => withSession(sid, undefined, (s) => cancelPage(s, canvasId)),
  reprioritizePage: (sid, canvasId, rank) =>
    withSession(sid, undefined, (s) => reprioritizePage(s, canvasId, rank)),
  cancelPageRenders: (sid) => withSession(sid, undefined, (s) => cancelPageRenders(s)),
  quiesce: (sid) => withSession(sid, undefined, (s) => quiesce(s)),
  renderPage: (sid, canvasId, scale, renderText, rank = 0) =>
    withSession(sid, Promise.resolve(noSession()), (s) =>
      renderPage(s, canvasId, scale, renderText, rank)),
  renderThumb: (sid, canvasId, page, scale) =>
    withSession(sid, Promise.resolve(noSession()), (s) => renderThumb(s, canvasId, page, scale)),
  probePageSize: (sid, page) =>
    withSession(sid, Promise.resolve(noSession()), (s) => probePageSize(s, page)),
  cancelThumb: (sid, canvasId) => withSession(sid, undefined, (s) => cancelThumb(s, canvasId)),
  hasThumb: (sid, page, scale) => withSession(sid, false, (s) => hasThumb(s, page, scale)),
  coverDataUrl: (sid, path, maxWidth) =>
    withSession(sid, Promise.resolve(noSession()), (s) => coverDataUrl(s, path, maxWidth)),
  extractPageText: (sid, page) =>
    withSession(sid, Promise.resolve(noSession()), (s) => extractPageText(s, page)),
  setSearchContext: (sid, query) => withSession(sid, undefined, (s) => setSearchContext(s, query)),
  setActiveMatch: (sid, page, index) =>
    withSession(sid, undefined, (s) => setActiveMatch(s, page, index)),
  clearHighlights: (sid) => withSession(sid, undefined, (s) => clearHighlights(s)),
  setPaper: (sid, hex) => withSession(sid, undefined, (s) => setPaper(s, hex)),
  setPaperActive: (sid, on) => withSession(sid, undefined, (s) => setPaperActive(s, on)),
  takePaperFrame: (sid, canvasId) => withSession(sid, null, (s) => takePaperFrame(s, canvasId)),
  samplePaperPage: (sid, page) =>
    withSession(sid, Promise.resolve({ ok: true as const }), (s) => samplePaperPage(s, page)),
  sweep: (sid) => withSession(sid, undefined, (s) => s.sweepPdf()),
  sweepSnapshots: (sid) => withSession(sid, undefined, (s) => s.sweepSnapshots()),
  prefetchThumb: (sid, page, scale) =>
    withSession(sid, Promise.resolve(), (s) => prefetchThumb(s, page, scale)),
  suspendPrefetches: (sid) => withSession(sid, undefined, (s) => suspendPrefetches(s)),
  resumePrefetches: (sid) => withSession(sid, undefined, (s) => resumePrefetches(s)),
  sessionStats,

  stats,
  refreshTheme,
  setScrubMode,
  setAppearanceMenuOpen,
} satisfies PDFReaderApi;

// Freeze the object: the Rust bridge checks existence only.
Object.freeze(globalThis.PDFReader);
