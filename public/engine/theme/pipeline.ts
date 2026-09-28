// Theme pipeline discovery: read the root CSS variables that describe the
// current appearance and cache them until the root style changes.

import type { PipelineCache } from "../types";
import { paperInfo } from "./paper";

export const pipelineCache: PipelineCache = {
  token: null,
  filter: "none",
  blend: "normal",
  paperInfo: null,
  gen: 0,
};
export function invalidatePipeline(): void {
  pipelineCache.token = null;
  // An explicit invalidation is a real change: the next read re-derives even
  // if the values look identical (the caller knows something they don't).
  lastInputs = null;
  pipelineCache.gen += 1;
}
// The last VALUES the pipeline was derived from. The root style attribute is
// only the cheap change detector: it also carries tokens the bake never reads
// — the engine's own `--pdf-paper` / `--pdf-paper-baked` publications, which
// change as pages render and the paper is re-detected, most visibly during a
// zoom. Bumping `gen` on every attribute change made each of those writes
// look like a new theme: renders already in flight were discarded and
// re-baked (renderer.ts compares the gen at bake time), the re-bake published
// the paper again, and the page flickered through a loop of re-renders. So
// `gen` moves only when an input the bake actually consumes moves.
let lastInputs: string | null = null;

export function readPipeline(): PipelineCache {
  const root = document.documentElement;
  const token = root.getAttribute("style") || "";
  if (pipelineCache.token === token) return pipelineCache;
  let filter = "none";
  let blend = "normal";
  let paper = "";
  try {
    const cs = getComputedStyle(root);
    filter = (cs.getPropertyValue("--canvas-filter") || "none").trim() || "none";
    blend = (cs.getPropertyValue("--canvas-blend") || "normal").trim() || "normal";
    paper = (cs.getPropertyValue("--color-paper") || "").trim();
  } catch (_) {
    /* identity */
  }
  pipelineCache.token = token;
  const inputs = `${filter}|${blend}|${paper}`;
  if (inputs === lastInputs) {
    // Same theme, different attribute string: nothing the rasters depend on
    // changed, so every bake in flight (and every cached thumbnail) stays
    // valid, and the resolved paper can be kept too.
    return pipelineCache;
  }
  lastInputs = inputs;
  pipelineCache.filter = filter;
  pipelineCache.blend = blend;
  pipelineCache.paperInfo = null;
  pipelineCache.gen += 1;
  return pipelineCache;
}
export function pipelineIsIdentity(pipeline: PipelineCache): boolean {
  if (pipeline.filter !== "none") return false;
  if (pipeline.blend === "normal") return true;
  if (pipeline.blend === "multiply") {
    const rgb = paperInfo(pipeline).rgb;
    return rgb[0] === 255 && rgb[1] === 255 && rgb[2] === 255;
  }
  return false;
}
