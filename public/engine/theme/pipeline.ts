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
  pipelineCache.gen += 1;
}
export function readPipeline(): PipelineCache {
  const root = document.documentElement;
  const token = root.getAttribute("style") || "";
  if (pipelineCache.token === token) return pipelineCache;
  let filter = "none";
  let blend = "normal";
  try {
    const cs = getComputedStyle(root);
    filter = (cs.getPropertyValue("--canvas-filter") || "none").trim() || "none";
    blend = (cs.getPropertyValue("--canvas-blend") || "normal").trim() || "normal";
  } catch (_) {
    /* identity */
  }
  pipelineCache.token = token;
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
