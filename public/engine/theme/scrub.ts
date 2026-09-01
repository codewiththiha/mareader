// Theme refresh + appearance-scrub mode. In live mode, raw page and thumbnail
// pixels stay under CSS filter + blend permanently, so theme changes never
// need a bake. In baked mode the rebake/swap path below runs instead, and the
// raw exposure lasts only for the duration of a slider scrub.

import { bakeInto } from "./bake";
import { showRaw } from "../canvas";
import { session } from "../state";
import { isLivePipeline, readPipeline, setLivePipeline } from "./pipeline";
import { paperInfo } from "./paper";
import { ensureEntryCurrent, paintAllVisibleThumbs } from "./thumbnails";
import { preparePagesForScrub, rerenderLivePages } from "../renderer";

// A Settings commit after a scrub has the same final pipeline the scrub exit
// just baked. Remember it by value rather than generation: invalidation bumps
// generations even when the actual filter/paper output is unchanged.
let lastBakedFingerprint: string | null = null;

function pipelineFingerprint(): string {
  const pipeline = readPipeline();
  return `${pipeline.filter}|${pipeline.blend}|${paperInfo(pipeline).color}`;
}

export async function rebakeTheme(force = false): Promise<void> {
  if (isLivePipeline()) return;
  if (session.themeScrubActive) return;
  const pipeline = readPipeline();
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
  lastBakedFingerprint = fingerprint;
}

/**
 * Enter and leave the live-CSS appearance pipeline as one atomic operation.
 * This function is called only through pdfEngine's serialized theme queue.
 */
export async function setScrubModeInternal(on: boolean): Promise<void> {
  // In live mode the raw exposure is permanent, so a scrub request is already
  // satisfied and a scrub exit must not bake. Baked mode uses the real
  // enter/leave transitions below.
  if (isLivePipeline()) on = true;
  if (session.themeScrubActive === on) return;

  if (on) {
    // The global class now controls only the texture stacking order. Canvas
    // theming is attached to each raw raster by showRaw/showBaked, so a baked
    // canvas remains unfiltered while another canvas changes asynchronously.
    document.documentElement.classList.add("appearance-scrubbing");
    session.setThemeScrubActive(true);
    for (const st of session.stateByCanvasId.values()) {
      if (st.dead || !st.canvas || !st.rawCanvas || st.rawCanvas === st.canvas) continue;
      showRaw(st.canvas, st.rawCanvas, "canvas-raw");
    }
    paintAllVisibleThumbs();
    // Pages without a retained raw are rendered into their live canvas by
    // preparePagesForScrub; renderer tags that raw result before yielding.
    await preparePagesForScrub();
    return;
  }

  // Keep the class up while async bakes replace raw rasters. A page whose
  // live canvas became its only raw backing during scrub cannot be baked in
  // place without double-filtering, so re-render it before releasing CSS.
  const needsRerender = [...session.stateByCanvasId.values()].some(
    (st) => !st.dead && !!st.canvas && (!st.rawCanvas || st.rawCanvas === st.canvas),
  );
  session.setThemeScrubActive(false);
  await rebakeTheme(true);
  if (needsRerender) await rerenderLivePages();
  document.documentElement.classList.remove("appearance-scrubbing");
}

/**
 * Switch the whole theming pipeline between live (compositor filter + blend on
 * the raw rasters) and baked (the filter burned into every raster). Called
 * only through pdfEngine's serialized theme queue, so the raster swap below
 * can never interleave with a refresh or a scrub.
 */
export async function setPipelineModeInternal(live: boolean): Promise<void> {
  if (isLivePipeline() === live) return;
  setLivePipeline(live);
  if (live) {
    // Expose the raws and hand the theming back to CSS. Identical to entering
    // a scrub, except nothing will leave it again.
    await setScrubModeInternal(true);
    return;
  }
  // Leaving live: bake the current pipeline into every raster and drop the
  // raw markers. A page whose only backing is its live canvas is re-rendered
  // rather than baked in place, or it would be filtered twice.
  await setScrubModeInternal(false);
}
