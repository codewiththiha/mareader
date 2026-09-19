// Pure admission tests: no PDF, DOM or timers. CI bundles the production
// module in memory; nothing here is a second implementation of the policy.
import assert from "node:assert/strict";
import { build } from "esbuild";

export async function run(): Promise<void> {
  const output = await build({ entryPoints: ["public/engine/raster-scheduler.ts"], bundle: true, write: false, format: "esm", platform: "node" });
  const { RasterScheduler, ScrollMotion, rasterSize } = await import(`data:text/javascript;base64,${Buffer.from(output.outputFiles![0]!.text).toString("base64")}`);
  const frames: Array<() => void> = [];
  let concurrency = 2;
  let resident = 0;
  const lane = new RasterScheduler({ resident: () => resident, evict: () => {}, concurrency: () => concurrency,
    defer: (callback: () => void) => frames.push(callback), hard: 100, soft: 80 });
  const flush = async () => {
    for (let turn = 0; turn < 12; turn++) {
      for (const callback of frames.splice(0)) callback();
      await Promise.resolve();
    }
  };
  const started: string[] = [];
  const job = (key: string, priority = 1, bytes = 20) => ({
    key, bytes, priority: (): number | null => priority,
    cancel: () => {}, cancelled: () => "cancelled", failed: () => "failed",
    run: async () => { started.push(key); return key; },
  });

  // Latest intent wins, and EVERY superseded caller settles.
  const old = lane.request(job("page", 1));
  const latest = lane.request(job("page", 1000));
  const neighbor = lane.request(job("neighbor", 500));
  await flush();
  assert.deepEqual(await Promise.all([old, latest, neighbor]), ["cancelled", "page", "neighbor"]);
  assert.deepEqual(started.splice(0), ["page", "neighbor"]);
  assert.equal(lane.stats().reservedBytes, 0);

  // Huge jump: physically remove the skipped path before any job starts.
  let generation = 1;
  const skipped = Array.from({ length: 100 }, (_, index) => lane.request({ ...job(`old-${index}`), priority: () => generation === 1 ? 100 : null }));
  generation++;
  lane.replan();
  assert.equal(lane.stats().queued, 0);
  const landing = lane.request(job("landing", 1000));
  await flush();
  assert.ok((await Promise.all(skipped)).every((value) => value === "cancelled"));
  assert.equal(await landing, "landing");
  assert.deepEqual(started.splice(0), ["landing"]);

  // A canceled active task keeps its bytes AND exclusive canvas ownership
  // until finally. Replanning cannot cancel its newer queued replacement.
  let finish!: () => void;
  let canceled = 0;
  let stale = false;
  const active = lane.request({ ...job("same", 10, 70),
    priority: () => stale ? null : 10,
    cancel: () => { canceled++; },
    run: async (ticket: { current(): boolean }) => {
      await new Promise<void>((resolve) => { finish = resolve; });
      assert.equal(ticket.current(), false);
      return "old";
    },
  });
  await flush();
  stale = true;
  const replacement = lane.request(job("same", 1000, 70));
  lane.replan();
  await flush();
  assert.equal(canceled, 1);
  assert.equal(lane.stats().running, 1);
  assert.equal(lane.stats().queued, 1);
  assert.equal(lane.stats().reservedBytes, 70);
  finish();
  await flush();
  assert.equal(await active, "cancelled");
  assert.equal(await replacement, "same");
  assert.equal(lane.stats().reservedBytes, 0);

  // Reserve BEFORE work, include existing residents, and degrade rather
  // than exceed hard. An impossible request terminates rather than hanging.
  resident = 40;
  let reservation = 0;
  const degraded = lane.request({ ...job("large", 1, 80), minimumBytes: 10,
    run: async (ticket: { bytes: number }) => { reservation = ticket.bytes; assert.ok(resident + lane.stats().reservedBytes <= 100); return "ok"; },
  });
  await flush();
  assert.equal(await degraded, "ok");
  assert.equal(reservation, 60);
  const impossible = lane.request(job("too-large", 1, 80));
  await flush();
  assert.equal(await impossible, "cancelled");
  resident = 0;
  const throws = lane.request({ ...job("throw"), run: () => { throw new Error("synchronous render failure"); } });
  await flush();
  assert.equal(await throws, "failed");
  assert.equal(lane.stats().reservedBytes, 0);

  // Fling/tracking admit no work. Settling resumes current work; document
  // teardown settles paused requests as canceled without a RAF leak.
  concurrency = 0;
  const paused = lane.request(job("paused"));
  await flush();
  assert.equal(lane.stats().running, 0);
  assert.equal(lane.stats().queued, 1);
  concurrency = 1;
  lane.replan();
  await flush();
  assert.equal(await paused, "paused");
  concurrency = 0;
  const teardown = lane.request(job("destroyed"));
  lane.reset();
  assert.equal(await teardown, "cancelled");
  assert.equal(lane.stats().queued, 0);

  // Giant pages, fractional CSS dimensions, DPR and extreme aspect ratios.
  for (const [w, h] of [[100000, 200000], [900.99, 1200.99], [1e9, 0.01], [0.01, 1e9]]) {
    for (const dpr of [1, 2, 3]) {
      const size = rasterSize(w, h, dpr, 2 * 1024 * 1024);
      assert.ok(size.width * size.height <= 2 * 1024 * 1024);
      assert.ok(size.width >= 1 && size.height >= 1);
    }
  }
  assert.throws(() => rasterSize(NaN, 100, 1, 100));
  assert.throws(() => rasterSize(100, 0, 1, 100));

  // Both directions, reversal, jump and clamped prediction. Units are px/ms.
  const motion = new ScrollMotion();
  motion.update(0, 1, 1000, 100000);
  motion.update(500, 17, 1000, 100000);
  assert.equal(motion.phase, "Fling");
  assert.equal(motion.direction, 1);
  assert.ok(motion.predicted > motion.offset);
  motion.update(100, 33, 1000, 100000);
  assert.equal(motion.direction, -1);
  assert.equal(motion.generation, 1);
  assert.equal(motion.predicted, 0);
  motion.update(90000, 49, 1000, 100000);
  assert.equal(motion.generation, 2);
  assert.equal(motion.phase, "Fling");
  assert.equal(motion.predicted, 90000);
  motion.settle();
  assert.equal(motion.phase, "Settling");
  motion.idle();
  assert.equal(motion.phase, "Idle");
  console.log("raster scheduler: priority, dedupe, jumps, cancellation, reservations, downgrade, teardown, giant pages and motion passed");
}
