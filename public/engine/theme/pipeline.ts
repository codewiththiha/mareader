// Theme pipeline discovery: read the root CSS variables that describe the
// current appearance and cache them until the root style changes.

import type { PipelineCache } from "../types";
import { paperInfo } from "./paper";

/** Live mode keeps the compositor's filter + blend on every canvas, so a page
 * and the document backdrop share one floating-point pass. Baked mode burns
 * the same pipeline into each raster instead (worker or inline kernel), which
 * costs a re-bake per appearance change but leaves the compositor with plain
 * opaque textures. Baking re-quantizes in integer stages, so a baked page can
 * never be bit-identical to the live composite — which is why this is a
 * reader-facing choice rather than a build-time constant.
 *
 * Live is the default; `setLivePipeline` flips it at runtime. */
let livePipeline = true;

export function isLivePipeline(): boolean {
  return livePipeline;
}

/** Record the new mode. Callers must drive the raster swap themselves (the
 * engine facade does it inside its serialized theme queue). */
export function setLivePipeline(on: boolean): void {
  livePipeline = on;
}

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
