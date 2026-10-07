// Theme refresh + the scrub window's real-time compositing. The theme is
// PRE-RENDERED: every raster carries it baked in, and a theme change runs the
// rebake/swap path below. The one exception is a slider scrub, which flips
// the mounted rasters back to raw pixels under the live CSS filter + blend —
// real-time compositing — for the duration of the drag.

import { bakeInto } from "./bake";
import { releaseCanvas, showRaw } from "../canvas";
import { PAGE_SNAPSHOT_CLASS, PAGE_SNAPSHOT_SELECTOR } from "../dom-contract";
import type { EngineSession } from "../state";
import { readPipeline } from "./pipeline";
import { paperInfo, publishBakedPaper } from "./paper";
import { ensureEntryCurrent, paintAllVisibleThumbs, visibleThumbPages } from "./thumbnails";
import { preparePagesForScrub, renderPage, rerenderLivePages } from "../renderer";

// A Settings commit after a scrub has the same final pipeline the scrub exit
// just baked. Remember it by value rather than generation: invalidation bumps
// generations even when the actual filter/paper output is unchanged.

// The scrub entry's re-render cover. A page that retained no unbaked raw is
// re-rendered in the background when a drag starts, and pdf.js wipes the
// backing store before it draws — the transparent frames in between read as
// a white flash under Dark's screen blend and a brightness jump under Dim's
// soft-light (Light's multiply hides them against white). Each such page
// carries a snapshot of its settled pixels for exactly the window of its own
// re-render, released the moment the fresh raw lands. Tracked so the exit
// path can release stragglers; zoom masks share the class but are never
// touched here.

function releaseEntrySnapshot(s: EngineSession, canvasId: string): void {
  const snap = s.scrub.entrySnapshots.get(canvasId);
  if (!snap) return;
  s.scrub.entrySnapshots.delete(canvasId);
  // Zero the backing store before the node goes: WKWebView does not release
  // a canvas IOSurface on DOM removal alone (state.ts, releaseSnapshots).
  releaseCanvas(snap);
  snap.remove();
}

export function releaseAllEntrySnapshots(s: EngineSession): void {
  for (const canvasId of [...s.scrub.entrySnapshots.keys()]) releaseEntrySnapshot(s, canvasId);
}

/** Copy the current pixels of every visible page that has no raw to swap in
 *  — exactly the set preparePagesForScrub will re-render — into a
 *  `.page-snapshot` mask. The mask hides the live canvas through CSS
 *  (styles/page_host.css) for as long as it is in the DOM. */
function snapshotStragglerPages(s: EngineSession): void {
  const vh = typeof window === "undefined" ? 0 : window.innerHeight;
  for (const [canvasId, st] of s.stateByCanvasId) {
    if (st.dead || !st.canvas || !st.host || st.rawCanvas) continue;
    if (s.scrub.entrySnapshots.has(canvasId)) continue;
    if (st.canvas.width === 0) continue;
    const rect = st.canvas.getBoundingClientRect();
    if (vh > 0 && (rect.bottom < 0 || rect.top > vh)) continue;
    // A zoom mask in flight already covers this host — a second copy stacked
    // over the first would show the same pixels at twice the memory.
    if (st.host.querySelector(PAGE_SNAPSHOT_SELECTOR)) continue;
    const snap = document.createElement("canvas");
    snap.className = PAGE_SNAPSHOT_CLASS;
    snap.width = st.canvas.width;
    snap.height = st.canvas.height;
    const ctx = snap.getContext("2d");
    if (!ctx) {
      releaseCanvas(snap);
      continue;
    }
    ctx.drawImage(st.canvas, 0, 0);
    // Between the canvas and the text layer — the same slot the zoom masks
    // use (src/components/formats/pdf/canvas_host.rs), so the shared CSS
    // stacking applies to both.
    const next = st.canvas.nextElementSibling;
    if (next) st.host.insertBefore(snap, next);
    else st.host.appendChild(snap);
    s.scrub.entrySnapshots.set(canvasId, snap);
  }
}

function pipelineFingerprint(s: EngineSession): string {
  const pipeline = readPipeline(s);
  return `${pipeline.filter}|${pipeline.blend}|${paperInfo(pipeline, s.themeRoot).color}`;
}

// The scrub window repaints the root tokens per tick and the rasters
// composite live, but the paper is a PUBLISHED value, and the flat-paper
// contract (styles/components/shell.css, styles/page_host.css) carries it on
// the backdrop base AND the page hosts: frozen for the length
// of a drag, the fractional page edge shows the stale base against the live
// gutter as a seam. The per-tick republish is the standing token watch's job
// (paper.ts, watchPaperTokens): it fires on the paint's own root-style
// writes — one 1×1 composite per tick, no raster work — and it also covers
// the texture moves and structural clicks no scrub window is open for. The
// exit's forced rebakeTheme republishes once more before the class drops,
// so nothing here manages an observer of its own.

export async function rebakeTheme(s: EngineSession, force = false): Promise<void> {
  if (s.themeScrubActive) return;
  const pipeline = readPipeline(s);
  // The backdrop's pre-themed paper rides on the same filter + paper this
  // rebake burns into the rasters, so it refreshes alongside them. An
  // unchanged fingerprint rewrites the identical value; the detected paper
  // itself publishes from setPaper the moment it moves.
  publishBakedPaper(s);
  const fingerprint = pipelineFingerprint(s);
  if (!force && fingerprint === s.scrub.lastBakedFingerprint) {
    // Inputs were already baked for this session. Align the thumbnail entries
    // so lazy paints do not schedule the same bake later.
    for (const entry of s.thumbCache.values()) {
      if (entry.display) entry.gen = pipeline.gen;
    }
    return;
  }

  for (const st of s.stateByCanvasId.values()) {
    // Only re-bake from a DISTINCT raw raster. If raw === live canvas the
    // pixels may already be themed; baking again double-filters.
    if (st.dead || !st.canvas || !st.rawCanvas || st.rawCanvas === st.canvas) continue;
    await bakeInto(st.canvas, st.rawCanvas, pipeline, "canvas-raw");
    s.dropRawIfIdle(st);
  }

  // Only the thumbs the reader can actually SEE are re-baked here: exactly the
  // set `paintAllVisibleThumbs` is about to paint. Every other cached page is
  // left one generation behind, and the generation check makes the cell that
  // next asks for it re-bake from its raw — a theme change must not raster the
  // whole LRU for cards nobody is looking at.
  for (const page of visibleThumbPages(s)) {
    const entry = s.thumbCache.get(page);
    if (!entry) continue;
    await ensureEntryCurrent(s, entry);
    if (s.themeScrubActive) return;
  }

  // `paintCached` selects baked displays while scrub is off, retaining the
  // stale baked canvas until each async replacement is ready.
  paintAllVisibleThumbs(s);
  // A page render that landed while this loop awaited could have been baked
  // against the superseded generation or left with the live tag; converge
  // before declaring the theme current.
  await settleCanvasTheme(s);
  s.scrub.lastBakedFingerprint = fingerprint;
}

/**
 * Convergence sweep after a theme transition settles: every live canvas must
 * carry exactly the theme state of the moment — `canvas-raw` (raw pixels
 * under the live CSS filter + blend) while a scrub is in force, pre-rendered
 * (baked) pixels without the tag otherwise.
 * Page renders are NOT serialized with the theme queue, so a render landing
 * mid-transition can settle one page on the other side of the tag from its
 * sibling (a spread half-themed) or bake against an invalidated palette
 * generation. This sweep is the generation guard's second half: idempotent,
 * and cheap when nothing drifted. Canvases that lost their unbaked raw are
 * re-rendered rather than baked in place, which would double-filter.
 */
async function settleCanvasTheme(s: EngineSession): Promise<void> {
  const wantRaw = s.themeScrubActive;
  const rerender: Array<() => Promise<unknown>> = [];
  for (const [id, st] of s.stateByCanvasId) {
    if (st.dead || !st.canvas) continue;
    const hasTag = st.canvas.classList.contains("canvas-raw");
    if (wantRaw) {
      if (hasTag) continue;
      if (st.rawCanvas && st.rawCanvas !== st.canvas) {
        showRaw(st.canvas, st.rawCanvas, "canvas-raw");
      } else {
        // The live canvas already holds raw pixels; only the tag is missing.
        st.canvas.classList.add("canvas-raw");
      }
    } else if (hasTag) {
      if (st.rawCanvas && st.rawCanvas !== st.canvas) {
        await bakeInto(st.canvas, st.rawCanvas, readPipeline(s), "canvas-raw");
        s.dropRawIfIdle(st);
      } else {
        rerender.push(() => renderPage(s, id, st.scale || 1, !!st.textLayerEl));
      }
    }
  }
  if (rerender.length) await Promise.all(rerender.map((job) => job()));
}

/**
 * Enter and leave the scrub window's real-time compositing — raw rasters
 * under the live CSS filter + blend — as one atomic operation. Called only
 * through pdfEngine's serialized theme queue.
 */
export async function setScrubModeInternal(s: EngineSession, on: boolean): Promise<void> {
  if (s.themeScrubActive === on) return;
  // Both edges of the window are worth remembering: a render baking just
  // after either one is a render a follow-up drag may want raw, so it keeps
  // its unbaked raster (renderer.ts). Outside the window bakes drop theirs.
  s.noteScrub();

  if (on) {
    // The global class delimits the scrub window for the CSS that keys off
    // it: the texture stacking order, and the page hosts' / backdrop's return
    // to the live blend base (styles/page_host.css, styles/components/
    // shell.css). Canvas theming itself rides each raw raster via
    // showRaw/showBaked, so a baked canvas remains unfiltered while another
    // changes asynchronously.
    s.setThemeScrubActive(true);

    // The synchronous half of the swap, and everything the gesture starts
    // with: pages that retained a raw raster blit it under the live CSS
    // filter, and a page whose live canvas IS the raw only needs the tag.
    // No await runs before this loop completes — the pixels and their tags
    // land in the same frame the drag first paints.
    for (const st of s.stateByCanvasId.values()) {
      if (st.dead || !st.canvas) continue;
      if (st.rawCanvas && st.rawCanvas !== st.canvas) {
        showRaw(st.canvas, st.rawCanvas, "canvas-raw");
      } else if (st.rawCanvas) {
        st.canvas.classList.add("canvas-raw");
      }
    }
    paintAllVisibleThumbs(s);

    // The asynchronous half: pages with no raw at all re-render in the
    // background, under a snapshot of their settled pixels. Awaiting that
    // here blocked the very frame the drag was trying to paint — a full
    // pdf.js re-render of every straggler between the pointer moving and
    // the page re-colouring. The exit path joins this promise, so a quick
    // drag-out still serializes behind the work started here, and the
    // settle sweep repairs any page whose render landed past the loop —
    // one half of a spread cannot be left un-themet.
    snapshotStragglerPages(s);
    s.scrub.entryPrepare = (async () => {
      try {
        await preparePagesForScrub(s, (canvasId) => releaseEntrySnapshot(s, canvasId));
      } catch (err) {
        console.warn("[pdfEngine] scrub prepare failed:", err);
      } finally {
        // A page whose render failed keeps its cover until here — settled
        // pixels beat a wiped canvas.
        releaseAllEntrySnapshots(s);
      }
      await settleCanvasTheme(s).catch((err: unknown) => {
        console.warn("[pdfEngine] scrub settle failed:", err);
      });
    })();
    return;
  }

  // Join the entry's background prepare before unwinding: the exit's rebake
  // has to see every straggler render the entry started, or it bakes a page
  // whose raw pixels are about to land on the live canvas — and the
  // needsRerender census below has to count the pages whose live canvas
  // became the only raw backing while that prepare ran.
  if (s.scrub.entryPrepare) {
    const preparing = s.scrub.entryPrepare;
    s.scrub.entryPrepare = null;
    await preparing;
  }

  // Keep the class up while async bakes replace raw rasters. A page whose
  // live canvas became its only raw backing during scrub cannot be baked in
  // place without double-filtering, so re-render it before releasing CSS.
  const needsRerender = [...s.stateByCanvasId.values()].some(
    (st) => !st.dead && !!st.canvas && (!st.rawCanvas || st.rawCanvas === st.canvas),
  );
  s.setThemeScrubActive(false);
  await rebakeTheme(s, true);
  if (needsRerender) await rerenderLivePages(s);
  // `needsRerender` was snapshotted before the flag cleared; a render landing
  // since then is covered here — as is any canvases the bake loop skipped
  // because their raw had become the live canvas mid-flight.
  await settleCanvasTheme(s);
  // Any straggler cover whose render never landed (a failed job, a host
  // unmounted mid-drag) must not outlive the gesture: the CSS hides the
  // live canvas under it.
  releaseAllEntrySnapshots(s);
}
