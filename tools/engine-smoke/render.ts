import { EngineResult, RenderPayload, created, getEl, setFakeComputed, trackCreatedCanvases, R } from "./harness.js";

export async function run(): Promise<void> {
  // Register and render a page, identity pipeline first.
  R.registerPage(1, "cont-0-cv", "cont-0-pg");
  const page0 = getEl("cont-0-pg");
  (page0 as unknown as { querySelector: () => { classList: { toggle(): void } } }).querySelector = () => ({ classList: { toggle() {} } });
  const r1 = await R.renderPage("cont-0-cv", 1.5, true);
  if (!r1.ok) throw new Error("render failed: " + JSON.stringify(r1));
  console.log("render ok (identity):", r1.width, "x", r1.height);

  // The render must stash its raw frame for the Rust paper session to drain.
  const frame = R.takePaperFrame("cont-0-cv");
  if (!frame || !frame.data || frame.data[0] !== 255 || frame.data[1] !== 255 || frame.data[2] !== 255) {
    throw new Error("render did not stash a white raw paper frame: " + JSON.stringify(frame && { page: frame.page, width: frame.width, px: frame.data[0] }));
  }
  console.log("paper frame ok: raw white frame stashed for the session");

  // Light theme over pure white allocates zero page-sized bake canvases.
  trackCreatedCanvases();
  setFakeComputed({ "--canvas-filter": "none", "--canvas-blend": "multiply" });
  R.registerPage(1, "cont-0-cv", "cont-0-pg");
  await R.renderPage("cont-0-cv", 1.5, true);
  const bakeCanvases = created.filter((el) => el.tagName === "CANVAS" && el.width > 10).length;
  if (bakeCanvases !== 0) throw new Error("identity pipeline allocated bake canvases: " + bakeCanvases);
  console.log("identity fast path ok (0 bake canvases)");

  // Burst coalescing.
  const p1 = R.renderPage("cont-0-cv", 1.0, true);
  const p2 = R.renderPage("cont-0-cv", 1.2, true);
  const p3 = R.renderPage("cont-0-cv", 1.4, true);
  const [a, b, c] = await Promise.all([p1, p2, p3]);
  const fmt = (r: EngineResult<RenderPayload>): string => r.ok ? "ok" : r.error.name;
  console.log("burst coalesce:", fmt(a), fmt(b), fmt(c));

}
