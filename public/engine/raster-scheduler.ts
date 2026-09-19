// Resource admission, deliberately independent of the DOM/pdf.js so the
// cancellation and reservation invariants can be tested with deferred jobs.
export const SOFT_RASTER_BYTES = 192 * 1024 * 1024;
export const HARD_RASTER_BYTES = 320 * 1024 * 1024;

export interface RasterTicket {
  readonly bytes: number;
  current(): boolean;
}
export interface RasterJob<T> {
  key: string;
  // null discards an obsolete job; -Infinity pauses it without losing intent.
  priority(): number | null;
  bytes: number;
  minimumBytes?: number;
  run(ticket: RasterTicket): Promise<T>;
  cancel(): void;
  cancelled(): T;
  failed(error: unknown): T;
}
interface Pending {
  job: RasterJob<unknown>;
  resolve(value: unknown): void;
  stale: boolean;
  reserved: number;
}

export class RasterScheduler {
  private queued = new Map<string, Pending>();
  private active = new Map<string, Pending>();
  private scheduled = false;
  private reserved = 0;
  private dropped = 0;
  private canceled = 0;
  private peak = 0;
  private downgraded = 0;

  constructor(private readonly policy: {
    resident(): number;
    evict(bytes: number): void;
    concurrency(): number;
    defer(callback: () => void): void;
    hard?: number;
    soft?: number;
  }) {}

  request<T>(job: RasterJob<T>): Promise<T> {
    this.cancel(job.key);
    return new Promise<T>((resolve) => {
      this.queued.set(job.key, {
        job, resolve: resolve as (value: unknown) => void, stale: false, reserved: 0,
      });
      this.wake();
    });
  }

  cancel(key: string): void {
    const queued = this.queued.get(key);
    if (queued) {
      this.queued.delete(key);
      this.dropped++;
      queued.resolve(queued.job.cancelled());
    }
    this.cancelActive(key);
  }

  private cancelActive(key: string): void {
    const active = this.active.get(key);
    if (active && !active.stale) {
      active.stale = true;
      this.canceled++;
      active.job.cancel();
      // Do NOT release its reservation until the asynchronous renderer exits.
    }
  }

  reset(): void {
    for (const key of new Set([...this.queued.keys(), ...this.active.keys()])) this.cancel(key);
  }

  replan(): void {
    for (const pending of this.queued.values()) {
      if (pending.job.priority() === null) this.cancel(pending.job.key);
    }
    for (const pending of this.active.values()) {
      if (!pending.stale && pending.job.priority() === null) this.cancelActive(pending.job.key);
    }
    this.wake();
  }

  wake(): void {
    if (this.scheduled) return;
    this.scheduled = true;
    this.policy.defer(() => {
      this.scheduled = false;
      this.pump();
    });
  }

  has(key: string): boolean { return this.queued.has(key) || this.active.has(key); }

  stats() {
    return {
      residentBytes: this.policy.resident(), reservedBytes: this.reserved,
      softBytes: this.policy.soft ?? SOFT_RASTER_BYTES,
      hardBytes: this.policy.hard ?? HARD_RASTER_BYTES,
      queued: this.queued.size, running: this.active.size,
      queuedJobsDropped: this.dropped, activeJobsCanceled: this.canceled,
      downgraded: this.downgraded, peakAccountedBytes: this.peak,
    };
  }

  private pump(): void {
    const hard = this.policy.hard ?? HARD_RASTER_BYTES;
    const soft = this.policy.soft ?? SOFT_RASTER_BYTES;
    while (this.active.size < this.policy.concurrency()) {
      let best: Pending | undefined;
      let priority = -Infinity;
      for (const pending of this.queued.values()) {
        const rank = pending.job.priority();
        if (rank === null) { this.cancel(pending.job.key); continue; }
        // A canceled job still owns its canvas until its finally block exits.
        if (this.active.has(pending.job.key)) continue;
        if (rank > priority) { best = pending; priority = rank; }
      }
      if (!best) return;
      const job = best.job;
      this.policy.evict(Math.max(0, this.policy.resident() + this.reserved + job.bytes - soft));
      const available = Math.max(0, hard - this.policy.resident() - this.reserved);
      const bytes = Math.min(job.bytes, available);
      if (bytes < (job.minimumBytes ?? job.bytes)) {
        // Running work may free room. Otherwise fail promptly rather than
        // leaving a permanently pending promise behind a full budget.
        if (this.active.size) return;
        this.cancel(job.key);
        continue;
      }
      this.queued.delete(job.key);
      best.reserved = bytes;
      this.reserved += bytes;
      if (bytes < job.bytes) this.downgraded++;
      this.peak = Math.max(this.peak, this.policy.resident() + this.reserved);
      this.active.set(job.key, best);
      const running = best;
      const ticket: RasterTicket = { bytes, current: () => !running.stale };
      // Invoke through a promise so even a synchronous throw releases bytes.
      void Promise.resolve().then(() => job.run(ticket))
        .then((value) => running.resolve(running.stale ? job.cancelled() : value),
          (error: unknown) => running.resolve(running.stale ? job.cancelled() : job.failed(error)))
        .finally(() => {
          this.reserved -= running.reserved;
          this.active.delete(job.key);
          this.wake();
        });
    }
  }
}

/** No quality floor may defeat the pixel ceiling, even for banner-shaped PDFs. */
export function rasterSize(width: number, height: number, dpr: number, limit: number) {
  if (![width, height, dpr, limit].every(Number.isFinite) || width <= 0 || height <= 0 || dpr <= 0 || limit < 1) {
    throw new Error("Invalid raster dimensions");
  }
  const outputScale = Math.min(dpr, Math.sqrt(limit / width / height));
  let w = Math.max(1, Math.floor(width * outputScale));
  let h = Math.max(1, Math.floor(height * outputScale));
  // Clamping a very thin dimension to 1 must not break the area bound.
  w = Math.min(w, Math.floor(limit));
  h = Math.min(h, Math.max(1, Math.floor(limit / w)));
  return { width: w, height: h, outputScale };
}

export type ScrollPhase = "Idle" | "Tracking" | "Fling" | "Settling";
export class ScrollMotion {
  phase: ScrollPhase = "Idle";
  velocity = 0; // CSS px / ms (thresholds below are screens / SECOND).
  direction: -1 | 0 | 1 = 0;
  offset = 0;
  predicted = 0;
  generation = 0;
  private lastTs = 0;

  update(offset: number, now: number, viewport: number, total: number): void {
    const delta = offset - this.offset;
    const jump = Math.abs(delta) > viewport * 3;
    const raw = delta / Math.max(1, now - this.lastTs);
    const direction = Math.sign(delta) as -1 | 0 | 1;
    const reversal = direction !== 0 && this.direction !== 0 && direction !== this.direction;
    if (jump || reversal) { this.generation++; this.velocity = 0; }
    this.velocity = jump ? 0 : this.velocity * 0.8 + raw * 0.2;
    this.direction = direction || this.direction;
    this.phase = jump || Math.abs(this.velocity) * 1000 / Math.max(1, viewport) > 2.5 ? "Fling" : "Tracking";
    this.offset = offset;
    this.lastTs = now;
    this.predicted = Math.max(0, Math.min(Math.max(0, total - viewport), offset + this.velocity * 180));
  }

  settle(): void { this.phase = "Settling"; this.velocity = 0; this.predicted = this.offset; }
  idle(): void { this.phase = "Idle"; }
}
