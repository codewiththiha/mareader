// A session owns every pending permit callback. The host sees only weak wakes,
// so its lifetime can never pin a removed pane's realm or document session.
import type { EngineSession } from "./state";
import type { RasterLane, RasterPermit } from "../reader/raster-protocol";

function parentLane(): { lane: RasterLane; owner: string } | null {
  // Standalone engine smoke/cover realms have no pane parent. Their existing
  // realm-wide two-slot gate remains the only page lane they need.
  if (typeof window === "undefined" || !window.parent || window.parent === window) return null;
  const nonce = new URLSearchParams(window.location.search).get("pane");
  if (!nonce) return null;
  const scope = window.parent.__mareaderRasterScope;
  const owner = scope ? `${scope}/${nonce}` : nonce;
  const lane = window.parent.__mareaderRasterLane;
  if (!lane) throw new Error("The workspace raster coordinator did not load");
  return { lane, owner };
}

export function acquireRasterSlot(s: EngineSession, canvasId: string): Promise<RasterPermit | null> {
  const host = parentLane();
  if (!host) return Promise.resolve({ release() {} });
  const { lane, owner } = host;
  const id = `${s.sid}:${++s.pageLane.permitSerial}`;
  return new Promise<RasterPermit | null>((resolve) => {
    let settled = false;
    const wake = () => {
      if (settled) {
        lane.release(owner, id);
        return;
      }
      settled = true;
      s.pageLane.waiters.delete(id);
      let released = false;
      resolve({
        release() {
          if (released) return;
          released = true;
          lane.release(owner, id);
        },
      });
    };
    const cancel = () => {
      if (settled) return;
      settled = true;
      lane.cancel(owner, id);
      s.pageLane.waiters.delete(id);
      resolve(null);
    };
    // Keep the wake strong HERE, never in the window-wide coordinator.
    s.pageLane.waiters.set(id, { canvasId, cancel, wake });
    if (!lane.request(owner, id, wake)) cancel();
  });
}

export function cancelRasterWaiters(s: EngineSession, canvasId?: string): void {
  for (const waiting of [...s.pageLane.waiters.values()]) {
    if (canvasId === undefined || waiting.canvasId === canvasId) waiting.cancel();
  }
}
