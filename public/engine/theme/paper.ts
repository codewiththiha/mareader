// Paper colour discovery and the backdrop's pre-rendered paper.

import type { PaperInfo, PipelineCache } from "../types";
import { acquireScratch, releaseScratch } from "../canvas";
import { applyFilterToData } from "./filterKernel";
import { heldSessions, paperPublisher } from "../state";
import type { EngineSession } from "../state";
import { readPipeline } from "./pipeline";

export function paperInfo(pipeline: PipelineCache, scope?: HTMLElement | null): PaperInfo {
  if (pipeline.paperInfo) return pipeline.paperInfo;
  const info: PaperInfo = { color: "#ffffff", rgb: [255, 255, 255] };
  try {
    const probe = document.createElement("div");
    probe.style.cssText = "display:none;background-color:var(--color-paper,#ffffff)";
    (scope ?? document.documentElement).appendChild(probe);
    const resolved = getComputedStyle(probe).backgroundColor;
    probe.remove();
    if (resolved && resolved !== "rgba(0, 0, 0, 0)") {
      info.color = resolved;
      const c = acquireScratch(1, 1);
      try {
        const ctx = c.getContext("2d");
        if (ctx) {
          ctx.fillStyle = resolved;
          ctx.fillRect(0, 0, 1, 1);
          const d = ctx.getImageData(0, 0, 1, 1).data;
          info.rgb = [d[0] ?? 255, d[1] ?? 255, d[2] ?? 255];
        }
      } finally {
        releaseScratch(c);
      }
    }
  } catch (_) {
    /* white paper */
  }
  pipeline.paperInfo = info;
  return info;
}

// Baked rasters would be themed twice by CSS, so the engine publishes
// the paper itself.

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

// The detected paper through one pass of the current filter + blend.
function themedPaperHex(pipeline: PipelineCache, raw: string | null): string | null {
  if (!raw) return null;
  const rgb = parsePaperHex(raw);
  if (!rgb) return raw; // not a shape we can re-theme: keep what we have

  // Stage one: the filter, via the shared LUT kernel.
  const px = new Uint8ClampedArray([rgb[0], rgb[1], rgb[2], 255]);
  if (pipeline.filter !== "none") {
    applyFilterToData(px, 1, 1, pipeline.filter);
  }
  if (pipeline.blend === "normal") return toPaperHex([px[0] ?? 0, px[1] ?? 0, px[2] ?? 0]);

  // Stage two: the blend, composited exactly as bakeRaster does.
  const paper = pipeline.paperInfo?.color ?? paperInfo(pipeline).color;
  let out: [number, number, number] = [px[0] ?? 0, px[1] ?? 0, px[2] ?? 0];
  let scratch: HTMLCanvasElement | null = null;
  try {
    scratch = acquireScratch(1, 1);
    const ctx = scratch.getContext("2d", { alpha: false });
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
  } catch (_) {
    /* the filtered colour alone is closer than none */
  } finally {
    if (scratch) releaseScratch(scratch);
  }
  return toPaperHex(out);
}

// Publish both paper scopes: the realm backdrop and the pane root.
export function publishBakedPaper(session?: EngineSession): void {
  const target = session ?? paperPublisher() ?? undefined;
  if (target && !target.disposed) {
    const pipeline = readPipeline(target);
    const hex = themedPaperHex(pipeline, target.detectedPaper);
    if (target.themeRoot) {
      if (hex) target.themeRoot.style.setProperty("--pane-pdf-paper-baked", hex);
      else target.themeRoot.style.removeProperty("--pane-pdf-paper-baked");
    }
    if (paperPublisher() === target) writeBakedPaper(document.documentElement, hex, "--pdf-paper-baked");
  } else {
    writeBakedPaper(document.documentElement, null, "--pdf-paper-baked");
  }
}

function writeBakedPaper(root: HTMLElement, hex: string | null, name: string): void {
  if (hex) root.style.setProperty(name, hex);
  else root.style.removeProperty(name);
}

// The standing watch: root writes realm-wide, pane roots per session.
let rootPaperWatcher: MutationObserver | null = null;
const panePaperWatchers = new WeakMap<EngineSession, MutationObserver>();
const paperWatchInputs = new WeakMap<EngineSession, string>();

function syncPaperFor(s: EngineSession): void {
  if (s.disposed) return;
  const pipeline = readPipeline(s);
  const input = `${pipeline.filter}|${pipeline.blend}|${pipeline.paperInfo?.color ?? ""}`;
  if (paperWatchInputs.get(s) === input) return;
  paperWatchInputs.set(s, input);
  publishBakedPaper(s);
}

// Observe this session's pinned pane root.
export function observeThemeRoot(s: EngineSession): void {
  if (typeof MutationObserver === "undefined" || !s.themeRoot || panePaperWatchers.has(s)) return;
  try {
    const observer = new MutationObserver(() => syncPaperFor(s));
    observer.observe(s.themeRoot, { attributes: true, attributeFilter: ["style", "class"] });
    panePaperWatchers.set(s, observer);
    syncPaperFor(s);
  } catch (_) {
    /* the engine still publishes on paper/theme calls */
  }
}

/** Release the per-session appearance observer before the root/session dies. */
export function unobserveThemeRoot(s: EngineSession): void {
  panePaperWatchers.get(s)?.disconnect();
  panePaperWatchers.delete(s);
  paperWatchInputs.delete(s);
}

// Install the one realm-root observer.
export function watchPaperTokens(): void {
  if (typeof MutationObserver === "undefined" || rootPaperWatcher) return;
  try {
    rootPaperWatcher = new MutationObserver(() => {
      for (const session of heldSessions()) syncPaperFor(session);
    });
    rootPaperWatcher.observe(document.documentElement, {
      attributes: true,
      attributeFilter: ["style", "class"],
    });
  } catch (_) {
    rootPaperWatcher = null;
  }
}
