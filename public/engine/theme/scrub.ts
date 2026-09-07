// Theme refresh + the scrub window's real-time compositing. The theme is
// PRE-RENDERED: every raster carries it baked in, and a theme change runs the
// rebake/swap path below. The one exception is a slider scrub, which flips
// the mounted rasters back to raw pixels under the live CSS filter + blend —
// real-time compositing — for the duration of the drag.

import { bakeInto } from "./bake";
import { showRaw } from "../canvas";
import { session } from "../state";
import { readPipeline } from "./pipeline";
import { paperInfo, publishBakedPaper } from "./paper";
import { ensureEntryCurrent, paintAllVisibleThumbs } from "./thumbnails";
import { preparePagesForScrub, renderPageInternal, rerenderLivePages } from "../renderer";

// A Settings commit after a scrub has the same final pipeline the scrub exit
// just baked. Remember it by value rather than generation: invalidation bumps
// generations even when the actual filter/paper output is unchanged.
let lastBakedFingerprint: string | null = null;

function pipelineFingerprint(): string {
  const pipeline = readPipeline();
  return `${pipeline.filter}|${pipeline.blend}|${paperInfo(pipeline).color}`;
}

// The scrub window repaints the root tokens per tick and the rasters
// composite live, but --pdf-paper-baked is a PUBLISHED value, and the
// flat-paper contract (styles/components/shell.css, styles/page_host.css)
// carries it on the backdrop base AND the page hosts: frozen for the length
// of a drag, the fractional page edge shows the stale base against the live
// gutter as a seam. Republish it per tick while the drag owns the tokens —
// one 1px filter + blend composite, no raster work. The observer re-triggers
// on its own write; the fingerprint (the composite's inputs only) dedupes
// that.
let rootStyleWatcher: MutationObserver | null = null;

function watchRootStyleForPaper(): void {
  // Environments without a compositor (the node smoke harness) have no
  // MutationObserver and no seam to prevent: nothing to watch.
  if (typeof MutationObserver === "undefined") return;
  let last = pipelineFingerprint();
  rootStyleWatcher = new MutationObserver(() => {
    if (!session.themeScrubActive) return;
    const fingerprint = pipelineFingerprint();
    if (fingerprint === last) return;
    last = fingerprint;
    publishBakedPaper();
  });
  rootStyleWatcher.observe(document.documentElement, {
    attributes: true,
    attributeFilter: ["style"],
  });
}

export async function rebakeTheme(force = false): Promise<void> {
  if (session.themeScrubActive) return;
  const pipeline = readPipeline();
  // The backdrop's pre-themed paper rides on the same filter + paper this
  // rebake burns into the rasters, so it refreshes alongside them. An
  // unchanged fingerprint rewrites the identical value; the detected paper
  // itself publishes from setPaper the moment it moves.
  publishBakedPaper();
  const fingerprint = pipelineFingerprint();
  if (!force && fingerprint === lastBakedFingerprint) {
    // The output is already current even though invalidatePipeline assigned a
    // new generation. Align cache generations so lazy thumbnail paints do not
    // schedule the same bake later.
    for (const entry of session.thumbCache.values()) {
      if (entry.display) entry.gen = pipeline.gen;
    }
    return;
  }

  for (const st of session.stateByCanvasId.values()) {
    // Only re-bake from a DISTINCT raw raster. If raw === live canvas the
    // pixels may already be themed; baking again double-filters.
    if (st.dead || !st.canvas || !st.rawCanvas || st.rawCanvas === st.canvas) continue;
    await bakeInto(st.canvas, st.rawCanvas, pipeline, "canvas-raw");
    session.dropRawIfIdle(st);
  }

  for (const entry of session.thumbCache.values()) {
    await ensureEntryCurrent(entry);
    if (session.themeScrubActive) return;
  }

  // `paintCached` selects baked displays while scrub is off, retaining the
  // stale baked canvas until each async replacement is ready.
  paintAllVisibleThumbs();
  // A page render that landed while this loop awaited could have been baked
  // against the superseded generation or left with the live tag; converge
  // before declaring the theme current.
  await settleCanvasTheme();
  lastBakedFingerprint = fingerprint;
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
async function settleCanvasTheme(): Promise<void> {
  const wantRaw = session.themeScrubActive;
  const rerender: Array<() => Promise<unknown>> = [];
  for (const [id, st] of session.stateByCanvasId) {
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
        await bakeInto(st.canvas, st.rawCanvas, readPipeline(), "canvas-raw");
        session.dropRawIfIdle(st);
      } else {
        rerender.push(() => renderPageInternal(id, st.scale || 1, !!st.textLayerEl));
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
export async function setScrubModeInternal(on: boolean): Promise<void> {
  if (session.themeScrubActive === on) return;

  if (on) {
    // The global class delimits the scrub window for the CSS that keys off
    // it: the texture stacking order, and the page hosts' / backdrop's return
    // to the live blend base (styles/page_host.css, styles/components/
    // shell.css). Canvas theming itself rides each raw raster via
    // showRaw/showBaked, so a baked canvas remains unfiltered while another
    // changes asynchronously.
    document.documentElement.classList.add("appearance-scrubbing");
    session.setThemeScrubActive(true);
    watchRootStyleForPaper();
    for (const st of session.stateByCanvasId.values()) {
      if (st.dead || !st.canvas || !st.rawCanvas || st.rawCanvas === st.canvas) continue;
      showRaw(st.canvas, st.rawCanvas, "canvas-raw");
    }
    paintAllVisibleThumbs();
    // Pages without a retained raw are rendered into their live canvas by
    // preparePagesForScrub; renderer tags that raw result before yielding.
    // The sweep then repairs any page whose render landed past the loop —
    // one half of a spread cannot be left un-themet.
    await preparePagesForScrub();
    await settleCanvasTheme();
    return;
  }

  // Keep the class up while async bakes replace raw rasters. A page whose
  // live canvas became its only raw backing during scrub cannot be baked in
  // place without double-filtering, so re-render it before releasing CSS.
  const needsRerender = [...session.stateByCanvasId.values()].some(
    (st) => !st.dead && !!st.canvas && (!st.rawCanvas || st.rawCanvas === st.canvas),
  );
  session.setThemeScrubActive(false);
  rootStyleWatcher?.disconnect();
  rootStyleWatcher = null;
  await rebakeTheme(true);
  if (needsRerender) await rerenderLivePages();
  // `needsRerender` was snapshotted before the flag cleared; a render landing
  // since then is covered here — as is any canvases the bake loop skipped
  // because their raw had become the live canvas mid-flight.
  await settleCanvasTheme();
  document.documentElement.classList.remove("appearance-scrubbing");
}
