// window.PDFReader facade. The implementation lives in public/engine/*
// (loader, renderer, thumbnails, search, theme); this file wires the public
// API and session teardown. Compiled to public/pdfEngine.js and loaded by
// the browser as an ES module.
//
// The facade holds NO document. Every document call names a session id
// (`sid`) minted by the Rust `PdfSession` that owns the session; the
// registry in engine/state.ts is the only way from a sid to its state, and
// an unknown or retired sid resolves to nothing. The realm-level calls that
// remain carry no document identity: the appearance broadcast (each live
// session re-derives its OWN raster theme), diagnostics, and the aggregate
// stats. docs/session-ownership.md is the ownership record.

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

/** The envelope every session-scoped async call resolves when its sid names
 *  no live session — a stale caller, never an engine fault. */
function noSession(): { ok: false; error: { name: string; message: string } } {
  return fail("no_session", "No live PDF session with this id");
}

/** Cancel and release every live page surface of `s`. Shared by
 *  `destroySession`, `quiesce` and the pagehide release. */
function cancelAndReleasePages(s: EngineSession): void {
  for (const st of s.stateByCanvasId.values()) {
    st.dead = true;
    try { st.renderTask && st.renderTask.cancel(); } catch (_) { /* ignore */ }
    try { st.textLayer && st.textLayer.cancel(); } catch (_) { /* ignore */ }
    s.releasePageSurfaces(st);
  }
}

/** The close intent's work-stop half for ONE session: every in-flight and
 *  queued job — page renders, thumbnail rasters, prefetch awaits — stops in
 *  the caller's task. The session survives; destroySession stays the one
 *  teardown and repeats this sweep idempotently. */
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

/** Tear ONE session down, in the order the ownership rules require:
 *  stop accepting (the sid stops resolving) → advance invalidation
 *  (`disposed`, the document-gone signal, the lane epoch) → cancel active
 *  work → destroy the document and its pdf.js worker → clear the page
 *  registry → clear cache references → forget the session. Other sessions
 *  are untouched; no timeout decides when this is done. */
async function destroySession(sid: Sid): Promise<void> {
  const s = sessionFor(sid);
  if (!s || !beginRetire(s)) return;
  // Disconnect the pane-root observer at the top of teardown, before any
  // asynchronous worker/document release can fail or await a late task.
  unobserveThemeRoot(s);
  // The sid no longer resolves, so nothing can queue again: take this
  // session out of the realm lane's registry FIRST, before any teardown
  // step below. The registry holds the session weakly (state.ts), so even
  // a skipped unregister could not pin it — but a live entry would still
  // receive pumps during the drain, and teardown leaves nothing behind.
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
    // Zero and remove the entry snapshots, not just the map: a canvas
    // backing store lingers until GC unless zeroed (WKWebView keeps the
    // IOSurface on DOM removal alone) — the same rule releaseSnapshots
    // applies to the zoom masks.
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
    // The realm's last document session is gone (none live, none draining):
    // the shared bake worker has nobody left to bake for. Realm-shared by
    // design while documents exist — never while none do.
    if (registryCounts().live === 0) releaseBakeWorker();
  }
}

// ---------------------------------------------------------------------------
// The appearance broadcast. Appearance is a global setting; the rasters it
// is baked into are session-owned, so a change enqueues on EVERY live
// session's own theme chain. Rust invokes these fire-and-forget, so each
// session's mutations ride one promise chain: a pause in a tint drag cannot
// interleave `scrub off -> bake` with a new `scrub on`. A failed mutation is
// reported but swallowed so it never poisons that session's queue.

/** The appearance state a session created mid-drag (or with the menu open)
 *  must start in. Global appearance, not document state. */
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

/** Re-bake only the sessions whose own bake inputs moved since they were
 *  last brought up to date. Split panes share this realm: a pane-local edit
 *  (independent themes) changes ONE pane root's tokens, and every other
 *  session reads the same pipeline it already baked with, so it is left
 *  alone rather than re-rendered. A scrubbing session always settles. */
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

/** The pipeline generation each session was last refreshed at. Weak: a
 *  disposed session drops out with its last reference. */
const refreshedGen = new WeakMap<EngineSession, number>();

/** The pane a scrub is scoped to: the reader marks the document element
 *  with the edited pane's id while a slider edits ONE pane's look
 *  (independent themes), and clears it for a window-wide edit. */
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
    // The class leaves once every session has settled out of the scrub —
    // and only if no new scrub began meanwhile.
    if (!on && !appearanceScrub) {
      root?.classList.remove("appearance-scrubbing");
      for (const el of scopedScrubRoots) el.classList.remove("appearance-scrubbing");
      scopedScrubRoots = [];
    }
  });
}

// Not enqueued: a retention flag, not a canvas mutation. The theme queue
// serializes raster swaps; a menu toggle must neither wait behind a bake
// nor delay one, and setting session state is synchronous anyway.
function setAppearanceMenuOpen(on: boolean): void {
  appearanceMenuOpen = on;
  for (const s of liveSessions()) s.setAppearanceMenuOpen(on);
}

// ---------------------------------------------------------------------------
// Session lifecycle.

function createEngineSession(sid: Sid): boolean {
  const s = createSession(sid);
  if (!s) return false;
  // The realm lane's registry: a freed raster slot re-offers the lane to
  // this session's queue head, so its pages pace with every other pane's.
  // The registry holds the session WEAKLY and destroySession drops the
  // entry, so it can never outlive the session.
  s.unregisterLanePump = registerLanePump(s);
  // A session opening mid-scrub joins a window-wide scrub; a scoped one
  // belongs to a pane that already exists.
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
  // A PDF session holds exactly one document: a new document is a new
  // session (a new sid), so nothing captured against the old one can reach
  // it.
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

/** The realm aggregate: gauges summed over live and draining sessions,
 *  counters over every session that ever lived (realm totals). */
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

// ---------------------------------------------------------------------------
// Realm-level memory listeners: the window is one, the sessions are many,
// so each walks every held session.

/** Release every GPU/canvas surface of every session. Registered on
 *  `pagehide` — the last reliable event before WKWebView tears the
 *  document down. */
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

// The selection tracker is NOT installed here: it is format-agnostic and
// lives in the reader bundle (public/readerEngine.ts), which index.html loads
// first. Nothing in this facade depends on it.

// The standing watch over the tokens the published backdrop paper is
// computed from (public/engine/theme/paper.ts): a drag repaints the root
// per frame and a texture click never reaches the scheduler at all, so the
// publish rides the mutations instead of waiting to be called. Installed
// with the other module-lifetime listeners; self-guarded where there is no
// MutationObserver (the node smoke harness). It republishes the PUBLISHING
// session's paper (engine/state.ts, setPaperPublisher).
watchPaperTokens();

// ---------------------------------------------------------------------------
// The surface. Session-scoped entries resolve their sid first; an unknown
// sid is a no-op (sync) or a `no_session` envelope (async).

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

// The engine contract is fixed by the Rust bridge: surface integrity beats
// extensibility, so freeze the object (has_pdf_reader only checks existence).
Object.freeze(globalThis.PDFReader);

// only the changed file was rewritten
