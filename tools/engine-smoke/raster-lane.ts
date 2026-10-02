// Run the HOST bundle, not a copied scheduler. The browser stage separately
// proves PDF render jobs acquire these permits across actual iframe realms.
import { readFileSync } from "node:fs";
import vm from "node:vm";

type Lane = {
  request(owner: string, id: string, wake: () => void): boolean;
  cancel(owner: string, id: string): void;
  release(owner: string, id: string): void;
  retire(owner: string): void;
  snapshot(): { limit: number; active: number; queued: number; owners: number; peakActive: number };
};

export async function run(): Promise<void> {
  const source = readFileSync(new URL("../../public/rasterLane.js", import.meta.url), "utf8");
  const window = {} as { __mareaderRasterLane: Lane };
  vm.runInNewContext(source, { window, WeakRef });
  const lane = window.__mareaderRasterLane;
  const woke: string[] = [];
  // These jobs belong to the pretend frames, just as real session waiters do.
  const jobs = ["a1", "a2", "b1", "b2", "cancelled"].map((id) => () => woke.push(id));
  const check = (active: number, queued: number, label: string) => {
    const s = lane.snapshot();
    if (s.limit !== 2 || s.active !== active || s.queued !== queued || s.peakActive > 2) {
      throw new Error(`${label}: window raster lane ${JSON.stringify(s)}`);
    }
  };
  if (!lane.request("a", "1", jobs[0]) || !lane.request("a", "2", jobs[1])) {
    throw new Error("first pane's two leases were refused");
  }
  if (!lane.request("b", "1", jobs[2]) || !lane.request("b", "2", jobs[3])) {
    throw new Error("second pane's two queued requests were refused");
  }
  check(2, 2, "two realms");
  if (woke.join() !== "a1,a2") throw new Error("queued jobs started above the window cap");
  if (lane.request("b", "3", jobs[4])) throw new Error("a frame exceeded its two-request bound");
  const waiting = (lane as unknown as { waiting: { owner: string; id: string; wake: WeakRef<() => void> }[] }).waiting;
  if (!waiting.every((entry) => entry.wake instanceof WeakRef && typeof entry.owner === "string")) {
    throw new Error("the persistent host holds a strong pane wake");
  }
  lane.cancel("b", "2");
  check(2, 1, "cancel one waiting job");
  lane.retire("a");
  check(1, 0, "retire only the outgoing realm");
  if (woke.join() !== "a1,a2,b1") throw new Error("retirement revived a cancelled job");
  lane.release("a", "1");
  check(1, 0, "a late old release cannot free a new realm's lease");
  lane.release("b", "1");
  check(0, 0, "all jobs settled");
  if (lane.snapshot().owners !== 0) throw new Error("the lane retains closed frame keys");
  // Failed wakes also return their slot; a dead realm cannot starve a sibling.
  lane.request("dead", "1", () => { throw new Error("realm gone"); });
  check(0, 0, "failed wake releases its slot");
  console.log("window raster lane ok: two slots, weak wakes, targeted retirement and no stale leases");
}
