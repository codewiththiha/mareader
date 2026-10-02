// One full-page raster budget for the persistent window. Engines retain their
// own jobs; the host retains only plain lease keys and WEAK wake callbacks.
// At most two outstanding requests per frame, drained on cancel or retirement.
import { WINDOW_RASTER_LIMIT, type RasterLane, type RasterLaneSnapshot } from "./reader/raster-protocol";

interface Waiting {
  owner: string;
  id: string;
  wake: WeakRef<() => void>;
}

const FRAME_REQUEST_LIMIT = 2;

class WindowRasterLane implements RasterLane {
  private readonly active = new Map<string, { owner: string; id: string }>();
  private waiting: Waiting[] = [];
  private peakActive = 0;

  private key(owner: string, id: string): string {
    return `${owner}/${id}`;
  }

  request(owner: string, id: string, wake: () => void): boolean {
    const key = this.key(owner, id);
    const pending = this.waiting.filter((entry) => entry.owner === owner).length;
    const running = [...this.active.values()].filter((entry) => entry.owner === owner).length;
    if (!owner || !id || pending + running >= FRAME_REQUEST_LIMIT || this.active.has(key) ||
        this.waiting.some((entry) => entry.owner === owner && entry.id === id)) return false;
    this.waiting.push({ owner, id, wake: new WeakRef(wake) });
    this.pump();
    return true;
  }

  cancel(owner: string, id: string): void {
    this.waiting = this.waiting.filter((entry) => entry.owner !== owner || entry.id !== id);
    this.pump();
  }

  release(owner: string, id: string): void {
    this.active.delete(this.key(owner, id));
    this.pump();
  }

  retire(owner: string): void {
    this.waiting = this.waiting.filter((entry) => entry.owner !== owner);
    for (const [key, entry] of this.active) {
      if (entry.owner === owner) this.active.delete(key);
    }
    this.pump();
  }

  private pump(): void {
    while (this.active.size < WINDOW_RASTER_LIMIT && this.waiting.length > 0) {
      const next = this.waiting.shift();
      if (!next) return;
      const wake = next.wake.deref();
      if (!wake) continue;
      const key = this.key(next.owner, next.id);
      this.active.set(key, { owner: next.owner, id: next.id });
      this.peakActive = Math.max(this.peakActive, this.active.size);
      try {
        wake();
      } catch (_) {
        // A realm that died before its wake cannot monopolize a permit.
        this.active.delete(key);
      }
    }
  }

  snapshot(): RasterLaneSnapshot {
    this.waiting = this.waiting.filter((entry) => entry.wake.deref() !== undefined);
    const owners = new Set([
      ...[...this.active.values()].map((entry) => entry.owner),
      ...this.waiting.map((entry) => entry.owner),
    ]);
    return {
      limit: WINDOW_RASTER_LIMIT,
      active: this.active.size,
      queued: this.waiting.length,
      owners: owners.size,
      peakActive: this.peakActive,
    };
  }
}

window.__mareaderRasterLane ??= new WindowRasterLane();
