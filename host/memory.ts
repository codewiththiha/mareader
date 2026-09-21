// Memory instrumentation — per plan §9 / §11.
// `performance.measureUserAgentSpecificMemory()` estimates application memory
// including iframes and workers where the browser exposes it. Not universally
// supported; treat as diagnostic, not as absolute truth.
//
// Call at route/pane boundaries: library-ready, reader-ready, workspace-ready,
// pane-opened, pane-closed, workspace-closed, library-reopened.
// Compare samples after GC/idle rather than immediate RSS.

export async function logMemory(label: string): Promise<void> {
  const perf = performance as Performance & {
    measureUserAgentSpecificMemory?: () => Promise<{ bytes: number; breakdown: unknown[] }>;
  };
  if (typeof perf.measureUserAgentSpecificMemory !== "function") {
    console.log(`[memory:${label}] measureUserAgentSpecificMemory not supported in this browser`);
    return;
  }
  try {
    const sample = await perf.measureUserAgentSpecificMemory();
    console.log(`[memory:${label}]`, JSON.stringify(sample, null, 2));
    // Also log coarse totals for quick diff.
    const bytes = (sample as unknown as { bytes?: number }).bytes;
    if (typeof bytes === "number") {
      console.log(`[memory:${label}] total ${Math.round(bytes / 1024)} KiB`);
    }
  } catch (e) {
    console.log(`[memory:${label}] measurement failed`, String(e));
  }
}

// Bounded render-cache helpers — mirror the plan's §16 / §17 budgets.
// Used by the host to decide whether to keep a pane's raster/thumbnail.
//
// Visible ± 2 rows keep policy: evict far pages aggressively, keep current
// and nearby medium, thumbnail low-res. The WASM heap cannot shrink, so the
// only isolation is whole-instance destruction plus bounded retention.
export function shouldKeepThumbnail(page: number, current: number, retentionWindow = 64): boolean {
  const lo = Math.max(1, current - retentionWindow);
  const hi = current + retentionWindow;
  return page >= lo && page <= hi;
}

export function visibleRange(current: number, total: number, window = 4): [number, number] {
  const lo = Math.max(1, current - window);
  const hi = Math.min(total, current + window);
  return [lo, hi];
}
