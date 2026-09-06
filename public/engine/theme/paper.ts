// Paper colour discovery: resolve --color-paper to a concrete colour and
// its RGB pixels (used by the identity check and the bake blend step), and
// publish the backdrop's pre-themed paper when the baked pipeline owns the
// canvases.

import type { PaperInfo, PipelineCache } from "../types";
import { acquireScratch, releaseScratch } from "../canvas";
import { applyFilterToData } from "./filterKernel";
import { isLivePipeline, readPipeline } from "./pipeline";
import { session } from "../state";

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

// --- Baked backdrop paper ---------------------------------------------------
//
// The blend backdrop re-derives the document paper by running the canvas
// filter + blend over --pdf-paper. That re-derivation is only valid while
// the canvases are RAW: under the live pipeline (and during a scrub) the
// compositor performs the exact same operation on the exact same inputs, so
// page and gutter composite identically. Once the baker has burned the
// pipeline into every raster, though, a page shows an already-themed opaque
// paper — and re-running the filter + blend in CSS applies the pipeline
// TWICE. multiply (light) and screen (dark) are identity on the paper, so
// the double pass hid there; dim's soft-light is not identity, and
// re-applying it moved the gutter away from the page.
//
// The fix publishes the themed paper itself: the detected document paper run
// through the SAME filter kernel + blend composite the baker uses, exposed
// as --pdf-paper-baked for the gated backdrop rule in
// styles/components/shell.css. Both stages reuse the baker's own
// implementations (filterKernel + a canvas globalCompositeOperation blend),
// so the backdrop and the baked rasters agree by construction, not by a
// second copy of the maths.

/** Parse the `#rrggbb` the paper session publishes; null on anything else. */
function parsePaperHex(hex: string): [number, number, number] | null {
  const m = /^#([0-9a-f]{6})$/i.exec(hex.trim());
  if (!m || !m[1]) return null;
  const v = m[1];
  return [
    parseInt(v.slice(0, 2), 16),
    parseInt(v.slice(2, 4), 16),
    parseInt(v.slice(4, 6), 16),
  ];
}

function toPaperHex(rgb: [number, number, number]): string {
  return (
    "#" +
    rgb.map((c) => Math.max(0, Math.min(255, Math.round(c))).toString(16).padStart(2, "0")).join("")
  );
}

/** The detected paper run through one pass of the current filter + blend —
 *  the colour a baked raster's paper region carries. */
function bakedPaperHex(pipeline: PipelineCache): string | null {
  const raw = session.detectedPaper;
  if (!raw) return null;
  const rgb = parsePaperHex(raw);
  if (!rgb) return raw; // not a shape we can re-theme: keep what we have

  // Stage one — the filter. The same LUT kernel the inline fallback and the
  // bake worker share; identity filters leave the pixel untouched.
  const px = new Uint8ClampedArray([rgb[0], rgb[1], rgb[2], 255]);
  if (pipeline.filter !== "none") {
    applyFilterToData(px, 1, 1, pipeline.filter);
  }
  if (pipeline.blend === "normal") return toPaperHex([px[0] ?? 0, px[1] ?? 0, px[2] ?? 0]);

  // Stage two — the blend, composited exactly the way bakeRaster does: the
  // themed UI paper as the backdrop, one draw over it in the pipeline's
  // blend mode. A 1×1 canvas is all a single colour needs.
  const paper = paperInfo(pipeline).color;
  let out: [number, number, number] = [px[0] ?? 0, px[1] ?? 0, px[2] ?? 0];
  try {
    const c = acquireScratch(1, 1);
    const ctx = c.getContext("2d", { alpha: false });
    if (ctx) {
      ctx.globalCompositeOperation = "source-over";
      ctx.fillStyle = paper;
      ctx.fillRect(0, 0, 1, 1);
      ctx.globalCompositeOperation = pipeline.blend as GlobalCompositeOperation;
      ctx.fillStyle = `rgb(${out[0]}, ${out[1]}, ${out[2]})`;
      ctx.fillRect(0, 0, 1, 1);
      ctx.globalCompositeOperation = "source-over";
      const d = ctx.getImageData(0, 0, 1, 1).data;
      out = [d[0] ?? out[0], d[1] ?? out[1], d[2] ?? out[2]];
    }
    releaseScratch(c);
  } catch (_) {
    /* the filtered colour alone is closer than none */
  }
  return toPaperHex(out);
}

/** Keep `--pdf-paper-baked` honest for the pipeline the reader is in NOW:
 *  the pre-themed paper when the baked pipeline owns the canvases, nothing
 *  when the live pipeline re-derives the paper in the compositor. Called at
 *  every moment one of the three inputs moves — the detected paper itself
 *  (`setPaper`), the theme (`rebakeTheme`), and the pipeline switch
 *  (`setPipelineModeInternal`) — so the backdrop can never lag the pages.
 *  Live mode removes the property: the CSS only reads it under
 *  `html[data-pipeline="baked"]`, but a stale themed value must not outlive
 *  the mode that justified it. */
export function publishBakedPaper(): void {
  let el: HTMLElement;
  try {
    el = document.documentElement;
  } catch (_) {
    return;
  }
  if (isLivePipeline()) {
    el.style.removeProperty("--pdf-paper-baked");
    return;
  }
  const hex = bakedPaperHex(readPipeline());
  if (hex) el.style.setProperty("--pdf-paper-baked", hex);
  else el.style.removeProperty("--pdf-paper-baked");
}
