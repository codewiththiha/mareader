// The paper pipeline's EYES. Every colour decision — detection, the
// per-page palette, the scroll interpolation — lives in the `pdf-paper`
// crate behind the Rust paper session; this module only moves pixels across
// the boundary:
//
// * `stashPaperFrame` — the renderer parks each live raster's raw frame at
//   the one pipeline moment the page's own paper is still unbaked; the Rust
//   session drains it after each successful render via `takePaperFrame`.
// * `samplePaperPage` — an offscreen render at a tiny scale, for the
//   look-ahead. Resolves only after a yield, so a burst of samples never
//   starves live renders.
// * `setPaper` — publish (or clear) `--pdf-paper`.
//
// Nothing is persisted: the palette is rebuilt from live frames every time
// a book opens. (Older builds kept a per-document colour cache in
// localStorage under `pdfreader.blend-paper.v2`; it is cleared on load.)
//
// Cost per frame: one ≤96×96 downscale and one pixel readback — and none
// at all while blend mode is off, which is the common case: the session
// gates the stash from the Rust side (setPaperActive).

import { session } from "./state";
import type { PaperFrame } from "./types";
import { releaseCanvas } from "./canvas";

/** Longest edge of a frame handed to Rust. Small enough that a page render
 * for colour purposes is near-free, large enough that a paper/plain region
 * survives the downscale. */
const SAMPLE_EDGE = 96;

/** At most this many undrained stashed frames: one per recently rendered
 * canvas. The Rust session drains after every render, so this is a safety
 * valve, not a working set. */
const STASH_MAX = 8;

/** localStorage keys older builds used for the per-document paper cache
 * (v1: per-scope colours; v2: one colour + detection area). The cache is
 * gone — the backdrop follows the reader page by page and needs no memory
 * of the book — so the stale entries are swept on load. */
const LEGACY_CACHE_KEYS = ["pdfreader.blend-paper.v1", "pdfreader.blend-paper.v2"];

const stash = new Map<string, PaperFrame>();

/** Whether the Rust paper session wants frames. Defaults to true so a pure
 * JS consumer sees the old behaviour; `paper::configure` flips it with the
 * blend switch, because stashing a ≤96px downscale + readback per render for
 * a session that will ignore every frame is pure waste. */
let active = true;

/** The Rust session's word for "blend mode is on" — gates stashPaperFrame. */
export function setPaperActive(on: boolean): void {
  active = !!on;
}

/** One scratch canvas for every downscale, reused across renders: a live
 * render stashes on EVERY completion, and a ≤96px bitmap is not worth an
 * allocation per page flip. */
let scratch: HTMLCanvasElement | null = null;

// --------------------------------------------------------------------------
// Legacy cache sweep
// --------------------------------------------------------------------------

/** Drop the per-document colour cache older builds left behind. Idempotent
 * and silent: a missing key or an unavailable storage is nothing to report. */
export function clearLegacyPaperCache(): void {
  try {
    for (const key of LEGACY_CACHE_KEYS) globalThis.localStorage?.removeItem(key);
  } catch {
    /* storage unavailable: nothing to sweep */
  }
}

// --------------------------------------------------------------------------
// Pixels
// --------------------------------------------------------------------------

/** Downscale `src` to ≤ SAMPLE_EDGE and read its pixels back. */
function downscale(src: HTMLCanvasElement): PaperFrame | null {
  const k = Math.min(SAMPLE_EDGE / src.width, SAMPLE_EDGE / src.height, 1);
  const w = Math.max(16, Math.floor(src.width * k));
  const h = Math.max(16, Math.floor(src.height * k));
  if (!scratch) scratch = document.createElement("canvas");
  const c = scratch;
  c.width = w;
  c.height = h;
  let out: PaperFrame | null = null;
  const ctx = c.getContext("2d", { willReadFrequently: true });
  if (ctx) {
    ctx.drawImage(src, 0, 0, w, h);
    try {
      out = { page: 0, width: w, height: h, data: ctx.getImageData(0, 0, w, h).data };
    } catch {
      out = null;
    }
  }
  return out;
}

/** Park a live raster's raw frame for the Rust session to drain. Called at
 * the renderer's raw-pixel moment, before the theme bake touches it. A no-op
 * while the session has blend mode off (see setPaperActive). */
export function stashPaperFrame(
  canvasId: string,
  page: number,
  src: HTMLCanvasElement | null,
): void {
  if (!active) return;
  if (!src || src.width < 8 || src.height < 8) return;
  const frame = downscale(src);
  if (!frame) return;
  frame.page = page;
  stash.delete(canvasId); // re-insert at the end = most recent
  stash.set(canvasId, frame);
  while (stash.size > STASH_MAX) {
    const oldest = stash.keys().next().value;
    if (oldest === undefined) break;
    stash.delete(oldest);
  }
}

/** Drain the frame stashed for `canvasId` (null when there is none). The
 * stash is consumed exactly once: a frame is one render's answer, not a
 * standing fact about the canvas. */
export function takePaperFrame(
  canvasId: string,
): { ok: true; page: number; width: number; height: number; data: Uint8ClampedArray } | null {
  const frame = stash.get(canvasId) ?? null;
  stash.delete(canvasId);
  return frame ? { ok: true, ...frame } : null;
}

// --------------------------------------------------------------------------
// Public API (pdfEngine facade)
// --------------------------------------------------------------------------

/** Publish `hex` as `--pdf-paper` (empty string clears it). */
export function setPaper(hex: string): void {
  session.setDetectedPaper(hex ? hex : null);
}

/** Render `page` offscreen at a tiny scale and hand its frame back. The
 * promise resolves only after a macrotask yield, so a burst of samples
 * leaves live renders their turn. `{ok:true}` with no frame = the page had
 * no answer (the caller skips it). */
export async function samplePaperPage(page: number): Promise<
  | { ok: true; page: number; width: number; height: number; data: Uint8ClampedArray }
  | { ok: true }
> {
  const doc = session.pdf;
  if (!doc || page < 1) return { ok: true };
  try {
    const p = await doc.getPage(page);
    const vp1 = p.getViewport({ scale: 1 });
    const k = Math.min(SAMPLE_EDGE / vp1.width, SAMPLE_EDGE / vp1.height, 1);
    const c = document.createElement("canvas");
    c.width = Math.max(8, Math.floor(vp1.width * k));
    c.height = Math.max(8, Math.floor(vp1.height * k));
    const ctx = c.getContext("2d", { willReadFrequently: true });
    if (!ctx) return { ok: true };
    const task = p.render({ canvasContext: ctx, viewport: p.getViewport({ scale: k }) });
    let frame: PaperFrame | null = null;
    try {
      await task.promise;
      // The render canvas is already ≤ SAMPLE_EDGE on its long side — read
      // it directly instead of paying a second drawImage through the
      // scratch downscaler.
      frame = {
        page,
        width: c.width,
        height: c.height,
        data: ctx.getImageData(0, 0, c.width, c.height).data,
      };
    } catch {
      /* cancelled: nothing to sample */
    } finally {
      releaseCanvas(c);
      try { p.cleanup(); } catch { /* already cleaned */ }
    }
    // Yield before answering so consecutive samples can never queue ahead
    // of a live render that slipped in between them.
    await new Promise((resolve) => setTimeout(resolve, 0));
    if (!frame) return { ok: true };
    return { ok: true, page, width: frame.width, height: frame.height, data: frame.data };
  } catch {
    /* page unavailable: the caller skips it */
    return { ok: true };
  }
}

/** A new document: drop the previous book's undrained frames. Also the
 * teardown path — nothing here outlives the book (the Rust session holds
 * the decisions). */
export function resetPaperForDocument(): void {
  stash.clear();
}
