// Theme refresh + appearance-scrub mode: re-bake every live page and
// thumbnail after a theme change, and swap raw/unbaked pixels in/out while
// the appearance sliders are being dragged.

import { bakeInto } from "./bake";
import { showRaw } from "../canvas";
import {
  dropRawIfIdle,
  setThemeScrubActive,
  themeScrubActive,
  stateByCanvasId,
  thumbCache,
} from "../state";
import { readPipeline } from "./pipeline";
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
  if (themeScrubActive) return;
  const pipeline = readPipeline();
  const fingerprint = pipelineFingerprint();
  if (!force && fingerprint === lastBakedFingerprint) {
    // The output is already current even though invalidatePipeline assigned a
    // new generation. Align cache generations so lazy thumbnail paints do not
    // schedule the same bake later.
    for (const entry of thumbCache.values()) {
      if (entry.display) entry.gen = pipeline.gen;
    }
    return;
  }

  for (const st of stateByCanvasId.values()) {
    // Only re-bake from a DISTINCT raw raster. If raw === live canvas the
    // pixels may already be themed; baking again double-filters.
    if (st.dead || !st.canvas || !st.rawCanvas || st.rawCanvas === st.canvas) continue;
    bakeInto(st.canvas, st.rawCanvas, pipeline, "canvas-raw");
    dropRawIfIdle(st);
  }

  for (const entry of thumbCache.values()) {
    await ensureEntryCurrent(entry);
    if (themeScrubActive) return;
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
  if (themeScrubActive === on) return;

  if (on) {
    // The global class now controls only the texture stacking order. Canvas
    // theming is attached to each raw raster by showRaw/showBaked, so a baked
    // canvas remains unfiltered while another canvas changes asynchronously.
    document.documentElement.classList.add("appearance-scrubbing");
    setThemeScrubActive(true);
    for (const st of stateByCanvasId.values()) {
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
  const needsRerender = [...stateByCanvasId.values()].some(
    (st) => !st.dead && !!st.canvas && (!st.rawCanvas || st.rawCanvas === st.canvas),
  );
  setThemeScrubActive(false);
  await rebakeTheme(true);
  if (needsRerender) await rerenderLivePages();
  document.documentElement.classList.remove("appearance-scrubbing");
}
