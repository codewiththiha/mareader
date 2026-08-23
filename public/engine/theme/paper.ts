// Paper colour discovery: resolve --color-paper to a concrete colour and
// its RGB pixels (used by the identity check and the bake blend step).

import type { PaperInfo, PipelineCache } from "../types";
import { acquireScratch, releaseScratch } from "../canvas";

export function paperInfo(pipeline: PipelineCache): PaperInfo {
  if (pipeline.paperInfo) return pipeline.paperInfo;
  const info: PaperInfo = { color: "#ffffff", rgb: [255, 255, 255] };
  try {
    const probe = document.createElement("div");
    probe.style.cssText = "display:none;background-color:var(--color-paper,#ffffff)";
    document.documentElement.appendChild(probe);
    const resolved = getComputedStyle(probe).backgroundColor;
    probe.remove();
    if (resolved && resolved !== "rgba(0, 0, 0, 0)") {
      info.color = resolved;
      const c = acquireScratch(1, 1);
      const ctx = c.getContext("2d");
      if (ctx) {
        ctx.fillStyle = resolved;
        ctx.fillRect(0, 0, 1, 1);
        const d = ctx.getImageData(0, 0, 1, 1).data;
        info.rgb = [d[0] ?? 255, d[1] ?? 255, d[2] ?? 255];
      }
      releaseScratch(c);
    }
  } catch (_) {
    /* white paper */
  }
  pipeline.paperInfo = info;
  return info;
}
