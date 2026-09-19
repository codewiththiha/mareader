// Reading continuity is page-based, not just a fraction of the viewport:
// at page 3, keep 1–5 ready even when a zoomed page spans several screens.
import type { ScrollPhase } from "./raster-scheduler";

// Mirrored by the PDF strip's READ_AHEAD_PAGES; CI checks the contract.
export const READ_AHEAD_PAGES = 2;
export interface PageIntent {
  visible: boolean;
  distance: number;
  predicted: number;
  ahead: boolean;
}

export function pageRasterPriority(
  phase: ScrollPhase, page: number, anchor: number | null, intent: PageIntent, direction: number,
): number | null {
  const delta = anchor === null ? null : page - anchor;
  const neighbor = delta !== null && Math.abs(delta) <= READ_AHEAD_PAGES;
  if (!neighbor && intent.distance > 0.75) return null;
  // Ordinary tracking MUST render ahead. Only a fast fling pauses admission;
  // leave queued intent alive so deceleration can resume without another event.
  if (phase === "Fling") return -Infinity;
  if (intent.visible) return 1000 - Math.min(intent.predicted, 5);
  const ahead = delta === null ? intent.ahead : Math.sign(delta) === (direction || 1);
  return (ahead ? 900 : 750) - Math.abs(delta ?? 0) * 30
    - Math.min(intent.distance, 5) * 4 - Math.min(intent.predicted, 5);
}

export function rasterConcurrency(phase: ScrollPhase): number {
  return phase === "Fling" ? 0 : phase === "Tracking" ? 1 : 2;
}
