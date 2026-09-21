import { test } from "node:test";
import assert from "node:assert/strict";
import { build } from "esbuild";
async function source(path) {
  const output = await build({ entryPoints: [path], bundle: true, write: false, format: "esm", platform: "node" });
  return import(`data:text/javascript;base64,${Buffer.from(output.outputFiles[0].text).toString("base64")}`);
}
const { panesPayload } = await source("host/panes.ts");
test("panes payload carries the active pane, the blend paper and per-pane status", () => {
  const payload = panesPayload("p2", [
    { id: "p1", bookId: "b1", title: "Dune", format: "pdf", loading: false, closing: false },
    { id: "p2", bookId: "b2", title: null, format: "md", loading: true, closing: false },
    { id: "p3", bookId: "b3", title: "Notes", format: "txt", loading: false, closing: true },
  ], "#e9e1d5");
  assert.equal(payload.type, "panes");
  assert.equal(payload.activePaneId, "p2");
  assert.equal(payload.blendPaper, "#e9e1d5");
  assert.deepEqual(payload.panes, [
    { paneId: "p1", bookId: "b1", title: "Dune", format: "pdf", status: "ready" },
    { paneId: "p2", bookId: "b2", title: null, format: "md", status: "loading" },
    { paneId: "p3", bookId: "b3", title: "Notes", format: "txt", status: "closing" },
  ]);
});
test("an empty registry publishes an empty set with no blend paper", () => {
  assert.deepEqual(panesPayload(null, [], null), { type: "panes", activePaneId: null, blendPaper: null, panes: [] });
});
