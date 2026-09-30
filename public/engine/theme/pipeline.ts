// Per-session theme pipeline discovery. Split panes share the engine realm,
// but their pane roots may carry distinct base/tint/filter tokens. A cache
// belongs to the EngineSession (not this module) so one pane's edit never
// re-bakes another pane against the same global pipeline.

import type { EngineSession } from "../state";
import type { PipelineCache } from "../types";
import { paperInfo } from "./paper";

export function invalidatePipeline(s: EngineSession): void {
  // The caller knows a theme boundary moved. Reset only the cheap token
  // detector; the per-session generation still advances only if actual bake
  // inputs changed on the next read.
  s.themePipeline.token = null;
}

function pipelineRoot(s: EngineSession): HTMLElement | null {
  return s.themeRoot ?? (typeof document === "undefined" ? null : document.documentElement);
}

export function readPipeline(s: EngineSession): PipelineCache {
  const cache = s.themePipeline;
  const root = pipelineRoot(s);
  if (!root) return cache;
  const documentRoot = root.ownerDocument?.documentElement ?? root;
  const token = [
    documentRoot.getAttribute("style") || "",
    documentRoot.getAttribute("class") || "",
    root === documentRoot ? "" : root.getAttribute("style") || "",
    root === documentRoot ? "" : root.getAttribute("class") || "",
  ].join("\u001f");
  if (cache.token === token) return cache;

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
  cache.token = token;
  const inputs = `${filter}|${blend}|${paper}`;
  if (inputs === cache.inputs) return cache;

  cache.inputs = inputs;
  cache.filter = filter;
  cache.blend = blend;
  cache.paperInfo = null;
  // Resolve the paper against THIS pane root. The canvas compositing paper
  // and the PDF's detected paper must be measured in the same CSS scope.
  paperInfo(cache, root);
  cache.gen += 1;
  return cache;
}

export function pipelineIsIdentity(pipeline: PipelineCache): boolean {
  if (pipeline.filter !== "none") return false;
  if (pipeline.blend === "normal") return true;
  if (pipeline.blend === "multiply") {
    const rgb = pipeline.paperInfo?.rgb ?? [255, 255, 255];
    return rgb[0] === 255 && rgb[1] === 255 && rgb[2] === 255;
  }
  return false;
}
