// Theme refresh and the scrub window's real-time compositing.

import { bakeInto } from "./bake";
import { releaseCanvas, showRaw } from "../canvas";
import { PAGE_SNAPSHOT_CLASS, PAGE_SNAPSHOT_SELECTOR } from "../dom-contract";
import type { EngineSession } from "../state";
import { readPipeline } from "./pipeline";
import { paperInfo, publishBakedPaper } from "./paper";
import { ensureEntryCurrent, paintAllVisibleThumbs, visibleThumbPages } from "./thumbnails";
import { preparePagesForScrub, renderPage, rerenderLivePages } from "../renderer";

// A Settings commit after a scrub equals the exit's pipeline.

// The scrub entry's re-render cover, released when the fresh raw lands.

function releaseEntrySnapshot(s: EngineSession, canvasId: string): void {
  const snap = s.scrub.entrySnapshots.get(canvasId);
  if (!snap) return;
  s.scrub.entrySnapshots.delete(canvasId);
  // Zero the backing store before the node goes.
  releaseCanvas(snap);
  snap.remove();
}

export function releaseAllEntrySnapshots(s: EngineSession): void {
  for (const canvasId of [...s.scrub.entrySnapshots.keys()]) releaseEntrySnapshot(s, canvasId);
}

// Copy visible pages with no raw into a `.page-snapshot` mask.
function snapshotStragglerPages(s: EngineSession): void {
  const vh = typeof window === "undefined" ? 0 : window.innerHeight;
  for (const [canvasId, st] of s.stateByCanvasId) {
    if (st.dead || !st.canvas || !st.host || st.rawCanvas) continue;
    if (s.scrub.entrySnapshots.has(canvasId)) continue;
    if (st.canvas.width === 0) continue;
    const rect = st.canvas.getBoundingClientRect();
    if (vh > 0 && (rect.bottom < 0 || rect.top > vh)) continue;
    // A zoom mask already covers this host.
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
    // Between canvas and text layer, the zoom masks' own slot.
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

// The paper is published: the per-tick republish is the token watch's.

export async function rebakeTheme(s: EngineSession, force = false): Promise<void> {
  if (s.themeScrubActive) return;
  const pipeline = readPipeline(s);
  // The backdrop's paper rides the same filter; refreshes alongside.
  publishBakedPaper(s);
  const fingerprint = pipelineFingerprint(s);
  if (!force && fingerprint === s.scrub.lastBakedFingerprint) {
    // Inputs already baked; align the thumbnail entries.
    for (const entry of s.thumbCache.values()) {
      if (entry.display) entry.gen = pipeline.gen;
    }
    return;
  }

  for (const st of s.stateByCanvasId.values()) {
    // Re-bake only from a DISTINCT raw raster.
    if (st.dead || !st.canvas || !st.rawCanvas || st.rawCanvas === st.canvas) continue;
    await bakeInto(st.canvas, st.rawCanvas, pipeline, "canvas-raw");
    s.dropRawIfIdle(st);
  }

  // Re-bake only the thumbs the reader can see.
  for (const page of visibleThumbPages(s)) {
    const entry = s.thumbCache.get(page);
    if (!entry) continue;
    await ensureEntryCurrent(s, entry);
    if (s.themeScrubActive) return;
  }

  // `paintCached` selects baked displays while scrub is off.
  paintAllVisibleThumbs(s);
  // Converge a render that landed mid-await before declaring current.
  await settleCanvasTheme(s);
  s.scrub.lastBakedFingerprint = fingerprint;
}

// Convergence sweep after a theme transition settles.
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

// Enter/leave the scrub window's compositing, atomically.
export async function setScrubModeInternal(s: EngineSession, on: boolean): Promise<void> {
  if (s.themeScrubActive === on) return;
  // Both edges of the window are remembered.
  s.noteScrub();

  if (on) {
    // The global class delimits the scrub window for the CSS.
    s.setThemeScrubActive(true);

    // The synchronous half: retained raws blit under the live CSS.
    for (const st of s.stateByCanvasId.values()) {
      if (st.dead || !st.canvas) continue;
      if (st.rawCanvas && st.rawCanvas !== st.canvas) {
        showRaw(st.canvas, st.rawCanvas, "canvas-raw");
      } else if (st.rawCanvas) {
        st.canvas.classList.add("canvas-raw");
      }
    }
    paintAllVisibleThumbs(s);

    // The asynchronous half: pages with no raw re-render in background.
    snapshotStragglerPages(s);
    s.scrub.entryPrepare = (async () => {
      try {
        await preparePagesForScrub(s, (canvasId) => releaseEntrySnapshot(s, canvasId));
      } catch (err) {
        console.warn("[pdfEngine] scrub prepare failed:", err);
      } finally {
        // A failed render keeps its cover.
        releaseAllEntrySnapshots(s);
      }
      await settleCanvasTheme(s).catch((err: unknown) => {
        console.warn("[pdfEngine] scrub settle failed:", err);
      });
    })();
    return;
  }

  // Join the entry's background prepare before unwinding.
  if (s.scrub.entryPrepare) {
    const preparing = s.scrub.entryPrepare;
    s.scrub.entryPrepare = null;
    await preparing;
  }

  // Keep the class up while async bakes replace raw rasters.
  const needsRerender = [...s.stateByCanvasId.values()].some(
    (st) => !st.dead && !!st.canvas && (!st.rawCanvas || st.rawCanvas === st.canvas),
  );
  s.setThemeScrubActive(false);
  await rebakeTheme(s, true);
  if (needsRerender) await rerenderLivePages(s);
  // `needsRerender` was snapshotted before the flag cleared.
  await settleCanvasTheme(s);
  // A straggler cover must not outlive the gesture.
  releaseAllEntrySnapshots(s);
}
