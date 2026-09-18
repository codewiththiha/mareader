// Paper colour discovery: resolve --color-paper to a concrete colour and its
// RGB pixels (used by the identity check and the bake blend step), and
// publish the backdrop's pre-rendered (pre-themed) paper.

import type { PaperInfo, PipelineCache } from "../types";
import { acquireScratch, releaseScratch } from "../canvas";
import { applyFilterToData } from "./filterKernel";
import { readPipeline } from "./pipeline";
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

// Re-deriving the document paper in CSS (filter + blend over --pdf-paper) is
// only valid while the canvases are RAW: the compositor performs the same
// operation on the same inputs, so page and gutter composite identically.
// Once the baker has burned the pipeline into every raster, a page shows an
// already-themed paper and the CSS pass would apply the pipeline TWICE.
// multiply (light) and screen (dark) are identity on the paper, which is why
// the double pass hid there; dim's soft-light is not, and re-applying it
// moved the gutter away from the page.
// So the engine publishes the themed paper itself: the detected paper run
// through the SAME filter kernel + blend composite the baker uses, exposed as
// --pdf-paper-baked for the backdrop rule in styles/components/shell.css.
// Both stages reuse the baker's own implementations (filterKernel + a canvas
// globalCompositeOperation blend), so backdrop and baked rasters agree by
// construction, not by a second copy of the maths. A third stage folds in
// the page's texture overlay — grain the compositor lays OVER the baked
// raster but not over the flat backdrop — so a page edge meeting the gutter
// on a fractional device pixel mixes the same colour on both sides.

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

/** The page texture overlay's average effect on the paper under it, as a
 *  multiplier on the flat colour: 1 when no texture is on, else a small
 *  darkening (multiply, light family) or lightening (screen, dark family)
 *  scaled by the dial. `--tex-opacity` is the dial as the compositor sees it
 *  — the Rust paint (src/effects/app/theme.rs) publishes zero when no
 *  texture mode is on, so a persisted opacity cannot leak through. */
function textureShiftFactor(): number {
  let opacity = 0;
  try {
    const raw = getComputedStyle(document.documentElement).getPropertyValue("--tex-opacity");
    opacity = parseFloat(raw || "0") || 0;
  } catch (_) {
    return 1;
  }
  if (!(opacity > 0)) return 1;
  const dark = document.documentElement.classList.contains("dark");
  // The strokes are sparse — the overlay's mean effect on a flat colour is
  // a small fraction of the dial, not the dial itself.
  return dark ? 1 + opacity * 0.06 : 1 - opacity * 0.08;
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

  // Stage three — the texture overlay. The page's ::before grain rides OVER
  // the baked raster (styles/textures.css); the backdrop carries only the
  // flat colour published here. Approximate the overlay's mean effect so
  // the two meet at the page edge — worst case is Parchment, paper grain
  // at 85% over a warm tint, where the untextured gutter used to show as a
  // seam along fractional device pixels.
  const factor = textureShiftFactor();
  if (factor !== 1) {
    out = [out[0] * factor, out[1] * factor, out[2] * factor];
  }
  return toPaperHex(out);
}

/** Keep `--pdf-paper-baked` honest: the detected paper pre-rendered through
 *  the current filter + blend, with the texture overlay's mean shift folded
 *  in — the colour a baked raster's paper carries. Called whenever an input
 *  moves: the detected paper (`setPaper`), the theme (`rebakeTheme`, and
 *  through it the scrub exit's forced rebake), and the standing token watch
 *  below, which hears the moves no engine call carries — a drag's per-tick
 *  repaint and a texture click the Rust rebake signature never sees. */
export function publishBakedPaper(): void {
  let el: HTMLElement;
  try {
    el = document.documentElement;
  } catch (_) {
    return;
  }
  const hex = bakedPaperHex(readPipeline());
  if (hex) el.style.setProperty("--pdf-paper-baked", hex);
  else el.style.removeProperty("--pdf-paper-baked");
}

// THE STANDING WATCH. The published paper is computed from four root tokens
// (--canvas-filter, --canvas-blend, --color-paper, --tex-opacity) plus the
// detected paper, and those tokens move on no schedule the engine hears:
// a tint or texture drag repaints the root once per animation frame
// (src/effects/app/theme.rs), and a texture-mode click never crosses the
// Rust side's rebake signature at all — texture is a CSS overlay, not a
// pixel input, so Parchment→None moves no filter, blend or base and the
// theme effect stays silent. Without this watch the backdrop would settle
// on whatever value the scheduler last published: a flat colour missing the
// texture's mean shift against a page edge that already carries it — the
// seam stage three exists to kill, back again on every texture move.
//
// So the watch republishes on every fingerprint move, wherever it came
// from: per tick through a drag — one 1×1 composite, no raster work, which
// is what keeps the backdrop in lockstep with the live-blending canvases
// the SCRUB WINDOW cover in styles/components/shell.css describes — and in
// the same frame as a structural click's paint, because a MutationObserver
// callback is a microtask and the value lands before the browser paints.
// The fingerprint holds only the composite's INPUTS: the observer
// re-triggers on publishBakedPaper's own root-style write, and the
// published value not being part of the fingerprint is what dedupes that
// to one publish per real move.
let paperWatcher: MutationObserver | null = null;
let paperWatchLast = "";

function paperWatchFingerprint(): string {
  try {
    const cs = getComputedStyle(document.documentElement);
    return [
      cs.getPropertyValue("--canvas-filter"),
      cs.getPropertyValue("--canvas-blend"),
      cs.getPropertyValue("--color-paper"),
      cs.getPropertyValue("--tex-opacity"),
    ].join("|");
  } catch (_) {
    return "";
  }
}

/** Install the standing watch. Idempotent, and a no-op in environments
 *  without a compositor: the node smoke harness has no MutationObserver —
 *  and no backdrop to keep honest. */
export function watchPaperTokens(): void {
  if (typeof MutationObserver === "undefined") return;
  if (paperWatcher) return;
  try {
    paperWatchLast = paperWatchFingerprint();
    paperWatcher = new MutationObserver(() => {
      const fingerprint = paperWatchFingerprint();
      if (fingerprint === paperWatchLast) return;
      paperWatchLast = fingerprint;
      publishBakedPaper();
    });
    // "class" rides along because the texture's mean shift branches on the
    // dark-family class; every base flip writes the root style too, so the
    // style filter alone would catch it — the class filter just does not
    // have to trust that.
    paperWatcher.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["style", "class"],
    });
  } catch (_) {
    paperWatcher = null;
  }
}
