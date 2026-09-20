import { test } from "node:test";
import assert from "node:assert/strict";
import { build } from "esbuild";
async function source(path) {
  const output = await build({ entryPoints: [path], bundle: true, write: false, format: "esm", platform: "node" });
  return import(`data:text/javascript;base64,${Buffer.from(output.outputFiles[0].text).toString("base64")}`);
}
const { PaneRuntimeManager } = await source("host/panes.ts");
const { readerConfig, envelope } = await source("host/protocol.ts");
const { place, remove, leaves, rectangles, internalDrop, hit } = await source("host/layout.ts");
const config = { bookId: "b1", path: "/books/book.pdf", format: "pdf", title: null, cover: null, resumePage: 1, resumeFraction: null, settings: {} };
const tick = () => new Promise((resolve) => setImmediate(resolve));
function deferred() { let resolve, reject; const promise = new Promise((a,b) => { resolve=a; reject=b; }); return { promise, resolve, reject }; }
function harness() {
  const runtimes = [], events = [];
  const manager = new PaneRuntimeManager((id, config, event) => {
    const ready = deferred(), closed = deferred();
    const runtime = { id, config, event, sent: [], disposing: 0,
      ready: () => ready.promise,
      command(command) { this.sent.push(command); },
      dispose() { if (!this.disposing++) ready.reject(new Error("cancelled")); return closed.promise; },
      boot() { ready.resolve(); }, finish() { closed.resolve(); },
    };
    runtimes.push(runtime); return runtime;
  }, (id, event) => events.push({ id, ...event }));
  return { manager, runtimes, events };
}
test("protocol rejects malformed configs and envelopes", () => {
  assert(readerConfig(config));
  for (const value of [null, {}, { ...config, format: "epub" }, { ...config, resumePage: 0 }, { ...config, resumeFraction: Infinity }, { ...config, settings: [] }]) assert(!readerConfig(value));
  assert(envelope({ version: 1, requestId: 1, payload: { type: "ready" } }));
  assert(!envelope({ version: 2, payload: { type: "ready" } }));
});
test("four independent frames, acknowledged close and no late events", async () => {
  const h = harness();
  const opening = Array.from({length: 4}, (_, i) => h.manager.open(`p${i}`, config));
  assert.equal(h.manager.panes.size, 4);
  await assert.rejects(h.manager.open("p4", config));
  for (const r of h.runtimes) { assert.deepEqual(r.sent, []); r.boot(); }
  await Promise.all(opening);
  const r = h.runtimes[1];
  const closing = h.manager.close("p1");
  assert.equal(h.manager.panes.size, 4);
  assert.equal(h.manager.close("p1"), closing);
  r.event({ type: "progress", page: 42 }); assert.equal(h.events.at(-1).id, "p1");
  r.finish(); await closing;
  assert.equal(h.manager.panes.size, 3);
  const count = h.events.length; r.event({ type: "focus" }); assert.equal(h.events.length, count);
  const all = h.manager.closeAll(); h.runtimes.forEach((r) => r.finish()); await all;
  assert.equal(h.manager.panes.size, 0);
});
test("closing during startup sends no OPEN and cleans the map", async () => {
  const h = harness(); const opening = h.manager.open("p1", config); const rejected = assert.rejects(opening);
  const closing = h.manager.close("p1"); h.runtimes[0].finish();
  await Promise.all([closing, rejected]); await tick();
  assert.equal(h.manager.panes.size, 0); assert.deepEqual(h.runtimes[0].sent, []);
});
test("preview is immutable, nested, limited to four, and identical to commit", () => {
  let tree = place(null, null, "a");
  const before = JSON.stringify(tree);
  const preview = place(tree, { pane: "a", edge: "right" }, "b");
  assert.equal(JSON.stringify(tree), before);
  assert.deepEqual(preview, place(tree, { pane: "a", edge: "right" }, "b"));
  tree = place(preview, { pane: "b", edge: "bottom" }, "c");
  tree = place(tree, { pane: "a", edge: "left" }, "d");
  assert.throws(() => place(tree, { pane: "a", edge: "top" }, "e"));
  tree = place(tree, { pane: "b", edge: "center" }, "e");
  assert.equal(leaves(tree).length, 4);
  for (const id of ["a", "c", "d"]) tree = remove(tree, id);
  assert.deepEqual(tree, { pane: "e" });
  assert.equal(remove(tree, "e"), null);
  const rects = rectangles(preview, {x: 0, y: 0, width: 800, height: 600});
  assert.equal(rects.get("a").width, 400); assert.equal(rects.get("b").x, 400);
  assert.deepEqual(hit(preview, {x: 0, y: 0, width: 800, height: 600}, 790, 300), {pane: "b", edge: "right"});
});
test("split drag accepts only a live authenticated library session", () => {
  const session = { token: "one-use", bookId: "b42" };
  const good = { version: 1, source: "library-sidebar", ...session };
  assert(internalDrop(good, session));
  for (const bad of [null, "/book.pdf", { ...good, source: "Files" }, { ...good, token: "old" }, { ...good, bookId: "b43" }]) assert(!internalDrop(bad, session));
  assert(!internalDrop(good, null));
});
