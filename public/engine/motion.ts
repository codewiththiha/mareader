// The scheduler's motion input: what the reader is doing, published by the
// strip that owns the scroller once per coalesced scroll frame.
//
// The engine cannot derive any of this itself. It sees canvases appear and
// disappear and render requests arrive; it has no idea whether the reader is
// parked on a page or throwing the document at twelve thousand pixels a
// second. The virtualizer does know — it owns the scroll container, the
// geometry and the smoothed velocity — so it publishes the answer, and the
// scheduler spends its lanes on it.
//
// Everything here is state, not policy: the numbers arrive over the bridge
// (`setScrollMotion`, `configureMotion` in pdfEngine.ts) and the scheduler and
// the memory ledger read them. The one piece of interpretation this module
// owns is the phase name, because the wire spelling is a contract with the
// Rust side and belongs in exactly one place per language — the host test in
// crates/pdf-engine/tests/engine_contract.rs pins the two together.

/** The wire spellings, in ascending order of movement. `MotionPhase::wire`
 *  in crates/pdf-engine/src/api/motion.rs declares the same five. */
export const PHASES = ["idle", "slow", "normal", "fast", "fling"] as const;

/** How fast the scroller is moving, as the virtualizer classified it. */
export type Phase = (typeof PHASES)[number];

/** A 1-based inclusive page range. */
export type PageWindow = { first: number; last: number };

/** One scroll frame's motion. */
export type MotionState = {
  phase: Phase;
  /** Sign of the smoothed velocity: 1 forward through the document, -1 back,
   *  0 at rest. The magnitude itself does not cross the bridge — the phase is
   *  the speed, classified, and nothing on this side would spend the number on
   *  anything else. */
  direction: number;
  /** The page the reader is projected to reach. 0 when unpublished. */
  predictedPage: number;
  /** Pages owed a full raster. Null means "no window published". */
  full: PageWindow | null;
  /** Pages owed at least a preview raster. Null means "no window published". */
  preview: PageWindow | null;
  /** How long a newly requested render waits before it may run. */
  delayMs: number;
  /** How many raster lanes to run. 0 means "the engine's own default". */
  workers: number;
};

/** The raster budget the memory ledger enforces (public/engine/memory.ts). */
export type MotionBudget = {
  /** Every surface the engine holds, bytes. */
  maxBytes: number;
  /** The preview tier's own ceiling, bytes. */
  previewBytes: number;
  /** Hard ceiling on preview surfaces, whatever they cost. */
  maxPreviewPages: number;
  /** Output-scale multiplier a preview raster is rendered at. */
  previewScale: number;
};

/** The state a document starts in, and the one a mode that publishes no
 *  motion (single page, spread) leaves in place: nothing is being paced, no
 *  window is closed, so every request is owed a full raster at the engine's
 *  own default lane count. */
export const IDLE_MOTION: MotionState = {
  phase: "idle",
  direction: 0,
  predictedPage: 0,
  full: null,
  preview: null,
  delayMs: 0,
  workers: 0,
};

export const DEFAULT_BUDGET: MotionBudget = {
  maxBytes: 96 * 1024 * 1024,
  previewBytes: 24 * 1024 * 1024,
  maxPreviewPages: 8,
  previewScale: 0.45,
};

/** A motion publication older than this is not a description of the present.
 *  The scheduler only cancels work on the strength of a FRESH moving frame:
 *  a zoom holds scroll feedback off for the length of its tween, and a
 *  window published before it would otherwise be used to cancel the renders
 *  the zoom's own commit just asked for. */
const MOTION_TTL_MS = 400;

let motion: MotionState = IDLE_MOTION;
let budget: MotionBudget = { ...DEFAULT_BUDGET };
let generation = 0;
let publishedAt = 0;
let live = false;

/** Prediction telemetry: the last page a moving frame predicted, and how the
 *  reader's next rest compared to it. The hit rate is the number the
 *  projection horizon should be tuned against — a predictor nobody believes
 *  is just latency. */
let lastPredicted = 0;
let predictions = 0;
let predictionHits = 0;

function parsePhase(value: string): Phase {
  const found = PHASES.find((phase) => phase === value);
  return found ?? "idle";
}

/** A page range off the wire, where `0,0` spells "nothing published". */
function parseWindow(first: number, last: number): PageWindow | null {
  if (!(first > 0) || !(last >= first)) return null;
  return { first, last };
}

/** Adopt the movement half of one frame. Called from the facade's
 *  `setScrollMotion`; the tier windows arrive right behind it over
 *  `setRenderTiers`, which is what re-scores the queue. */
export function setMotion(
  phase: string,
  direction: number,
  predictedPage: number,
  delayMs: number,
  workers: number,
): void {
  const next: MotionState = {
    ...motion,
    phase: parsePhase(phase),
    direction: direction > 0 ? 1 : direction < 0 ? -1 : 0,
    predictedPage: predictedPage > 0 ? Math.floor(predictedPage) : 0,
    delayMs: delayMs > 0 ? Math.floor(delayMs) : 0,
    workers: workers > 0 ? Math.floor(workers) : 0,
  };
  notePrediction(next);
  motion = next;
  publishedAt = Date.now();
  live = true;
}

/** Adopt the geometry half of one frame: the two tier windows, 1-based and
 *  inclusive, where `0,0` means "unpublished" and leaves the tier open. */
export function setTiers(
  firstFull: number,
  lastFull: number,
  firstPreview: number,
  lastPreview: number,
): void {
  motion = {
    ...motion,
    full: parseWindow(firstFull, lastFull),
    preview: parseWindow(firstPreview, lastPreview),
  };
  generation += 1;
  publishedAt = Date.now();
  live = true;
}

/** Score one prediction against the rest that followed it. A frame that
 *  stops moving is the reader arriving, so the page they arrived at is the
 *  answer the previous frame's projection can be graded against. */
function notePrediction(next: MotionState): void {
  if (motion.phase !== "idle" && motion.predictedPage > 0) {
    lastPredicted = motion.predictedPage;
  }
  if (next.phase !== "idle" || lastPredicted === 0 || next.predictedPage === 0) return;
  predictions += 1;
  if (Math.abs(lastPredicted - next.predictedPage) <= 1) predictionHits += 1;
  lastPredicted = 0;
}

/** The current motion. Never null: an unpublished document reads as idle. */
export function currentMotion(): MotionState {
  return motion;
}

/** Bumped once per publication. A queued render records the generation it was
 *  asked for at, and the scheduler uses the gap to tell a request the reader
 *  has already outrun from one that is still current. */
export function motionGeneration(): number {
  return generation;
}

/** Whether the motion in hand may be acted on: a strip published it, and it
 *  is recent enough to still describe the reader. */
export function motionIsCurrent(): boolean {
  return live && Date.now() - publishedAt <= MOTION_TTL_MS;
}

export function currentBudget(): MotionBudget {
  return budget;
}

/** Adopt the raster budget. Called once per document from the facade's
 *  `configureMotion`; the defaults stand until then. */
export function configureBudget(
  maxBytes: number,
  previewBytes: number,
  maxPreviewPages: number,
  previewScale: number,
): void {
  budget = {
    maxBytes: maxBytes > 0 ? maxBytes : DEFAULT_BUDGET.maxBytes,
    previewBytes: previewBytes > 0 ? Math.min(previewBytes, maxBytes) : DEFAULT_BUDGET.previewBytes,
    maxPreviewPages: maxPreviewPages > 0 ? Math.floor(maxPreviewPages) : DEFAULT_BUDGET.maxPreviewPages,
    previewScale: previewScale > 0 && previewScale <= 1 ? previewScale : DEFAULT_BUDGET.previewScale,
  };
}

/** Prediction telemetry, for `stats()`. */
export function predictionStats(): { predictions: number; hits: number } {
  return { predictions, hits: predictionHits };
}

/** Forget the document's motion: called on open and on teardown, so a book
 *  that was being flung cannot pace the first renders of the next one. */
export function resetMotion(): void {
  motion = IDLE_MOTION;
  generation += 1;
  publishedAt = 0;
  live = false;
  lastPredicted = 0;
  predictions = 0;
  predictionHits = 0;
}

/** Whether `page` falls inside `window`. An unpublished window contains
 *  nothing, which is the honest answer: a caller that never said what it owes
 *  has not asked the scheduler to hold anything back — every gate that
 *  excludes work is also gated on [`motionIsCurrent`]. */
export function inWindow(page: number, window: PageWindow | null): boolean {
  return !!window && page >= window.first && page <= window.last;
}

/** Distance from `page` to the reader's heading, weighted against travel.
 *
 * A page behind the reader costs three times what the same distance ahead
 * does: reading is directional, and the page just read is only coming back if
 * the reader changes their mind. `anchor` of 0 means "unknown", and then the
 * term is dropped rather than measured from page 1. */
export function directionalDistance(page: number, anchor: number, direction: number): number {
  if (!(anchor > 0)) return 0;
  const delta = page - anchor;
  if (direction > 0) return delta >= 0 ? delta : -delta * 3;
  if (direction < 0) return delta <= 0 ? -delta : delta * 3;
  return Math.abs(delta);
}
