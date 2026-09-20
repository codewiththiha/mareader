import { test } from "node:test";
import assert from "node:assert/strict";
import { build } from "esbuild";
async function source(path) {
  const output = await build({ entryPoints: [path], bundle: true, write: false, format: "esm", platform: "node" });
  return import(`data:text/javascript;base64,${Buffer.from(output.outputFiles[0].text).toString("base64")}`);
}
const { HostRuntimeManager } = await source("host/lifecycle.ts");
const { readerConfig, envelope } = await source("host/protocol.ts");
const config = { bookId: "b1", path: "/books/book.pdf", format: "pdf", title: null, cover: null, resumePage: 1, resumeFraction: null, settings: {} };
const tick = () => new Promise((resolve) => setImmediate(resolve));
function deferred() { let resolve, reject; const promise = new Promise((a,b) => { resolve=a; reject=b; }); return { promise, resolve, reject }; }
function harness() {
  const runtimes = [], states = [], events = [];
  let live = 0, high = 0;
  const manager = new HostRuntimeManager((config, event) => {
    live++; high = Math.max(high, live);
    const ready = deferred(), closed = deferred();
    const runtime = { config, event, sent: [], disposing: 0, removed: false,
      ready: () => ready.promise,
      command(command) { this.sent.push(command); },
      dispose() { if (!this.disposing++) ready.reject(new Error("cancelled")); return closed.promise; },
      boot() { ready.resolve(); },
      finish() { if (!this.removed) { live--; this.removed = true; } closed.resolve(); },
    };
    runtimes.push(runtime); return runtime;
  }, (event) => events.push(event), (state) => states.push(state));
  return { manager, runtimes, states, events, get high() { return high; }, get live() { return live; } };
}

test("protocol rejects unknown versions, malformed IDs and broad configs", () => {
  assert(readerConfig(config));
  for (const value of [null, {}, { ...config, format: "epub" }, { ...config, resumePage: 0 }, { ...config, resumeFraction: Infinity }, { ...config, settings: [] }]) assert(!readerConfig(value));
  assert(envelope({ version: 1, requestId: 1, payload: { type: "ready" } }));
  for (const value of [{ version: 2, payload: { type: "ready" } }, { version: 1, requestId: -1, payload: { type: "ready" } }, { version: 1, payload: null }]) assert(!envelope(value));
});

test("open waits for readiness and close waits for acknowledged destruction", async () => {
  const h = harness(); const opening = h.manager.open(config); await tick();
  const runtime = h.runtimes[0]; assert.equal(h.manager.state, "loading"); assert.equal(runtime.sent.length, 0);
  runtime.boot(); await opening; assert.equal(h.manager.state, "ready"); assert.deepEqual(runtime.sent[0], { type: "open", config });
  const closing = h.manager.close(); await tick(); assert.equal(h.manager.state, "closing"); assert.equal(h.live, 1);
  runtime.event({ type: "progress", page: 42 }); assert.equal(h.events.at(-1).page, 42);
  runtime.finish(); await closing; assert.equal(h.manager.state, "disposed"); assert.equal(h.live, 0);
});

test("rapid A/B/C opens keep one frame and discard stale navigation", async () => {
  const h = harness(); const a = h.manager.open(config); await tick(); const first = h.runtimes[0];
  const b = h.manager.open({ ...config, bookId: "b2" });
  const c = h.manager.open({ ...config, bookId: "b3" });
  first.event({ type: "close-request" }); assert.equal(h.events.length, 0);
  first.event({ type: "progress", page: 7 }); assert.equal(h.events.length, 1);
  assert.equal(h.runtimes.length, 1); first.finish(); await tick();
  assert.equal(h.runtimes.length, 2); const last = h.runtimes[1]; assert.equal(last.config.bookId, "b3");
  last.boot(); await Promise.all([a,b,c]); assert.equal(h.high, 1); assert.equal(h.manager.state, "ready");
  const close = h.manager.close(); last.finish(); await close;
});

test("close while creating cancels startup and never sends OPEN", async () => {
  const h = harness(); const open = h.manager.open(config); await tick(); const runtime = h.runtimes[0];
  const close = h.manager.close(); runtime.finish(); await Promise.all([open, close]);
  runtime.boot(); assert.equal(runtime.sent.length, 0); assert.equal(h.manager.state, "disposed");
});

test("repeated close shares teardown and emits no late state changes", async () => {
  const h = harness(); const open = h.manager.open(config); await tick(); h.runtimes[0].boot(); await open;
  const a = h.manager.close(), b = h.manager.close(); h.runtimes[0].finish(); await Promise.all([a,b]);
  assert.equal(h.runtimes[0].disposing, 1); assert.equal(h.manager.state, "disposed");
});

test("ten read cycles leave no active runtime references", async () => {
  const h = harness();
  for (let i = 0; i < 10; i++) {
    const open = h.manager.open({ ...config, bookId: `b${i}` }); await tick(); const runtime = h.runtimes.at(-1);
    runtime.boot(); await open; const close = h.manager.close(); runtime.finish(); await close;
    assert.equal(h.live, 0); assert.equal(h.manager.state, "disposed");
  }
  assert.equal(h.high, 1);
});

const { TauriScope } = await source("host/tauri.ts");
test("native scopes unlisten even when registration completes after disposal", async () => {
  const registration = deferred(); let stops = 0; const events = [];
  globalThis.window = { __TAURI__: { event: { listen: () => registration.promise } } };
  const scope = new TauriScope(config.path, (event) => events.push(event), async () => {});
  const subscription = scope.call("listen", "ai-stream-chunk");
  scope.dispose(); registration.resolve(() => { stops++; }); await subscription;
  assert.equal(stops, 1); scope.dispose(); assert.equal(stops, 1); assert.deepEqual(events, []);
  delete globalThis.window;
});
test("native scopes reject foreign file reads and unknown capabilities", async () => {
  const calls = [];
  globalThis.window = { __TAURI__: { core: { invoke: async (...args) => { calls.push(args); return "text"; } } } };
  const scope = new TauriScope(config.path, () => {}, async () => {});
  await assert.rejects(scope.call("invoke", { command: "read_file_text", args: { path: "/other.txt" } }));
  await assert.rejects(scope.call("invoke", { command: "delete_file", args: { path: config.path } }));
  await assert.rejects(scope.call("listen", "document-open-file"));
  assert.equal(await scope.call("invoke", { command: "read_file_text", args: { path: config.path } }), "text");
  assert.equal(calls.length, 1);
  scope.dispose(); await assert.rejects(scope.call("invoke", { command: "read_file_text", args: { path: config.path } }));
  delete globalThis.window;
});
