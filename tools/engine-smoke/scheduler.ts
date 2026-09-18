// The render scheduler: what a published scroll frame does to the queue.
//
// This is the half of the motion model that lives in the engine, and the half
// that is easiest to get subtly wrong — a scheduler that drops the wrong job
// shows up as a blank page in the reader, not as a failed assertion anywhere.
// So the scenarios below assert on the two things a reader would notice: which
// request wins a single lane, and which requests never run at all.

import type { EngineResult, RenderPayload } from "./harness.js";
import { PDFReader, rootProperty } from "./harness.js";

const fmt = (r: EngineResult<RenderPayload>): string => (r.ok ? "ok" : r.error.name);

/** Wait for a condition the engine reaches on its own clock (the representative
 *  page is built a beat after open, on purpose). */
async function waitFor(probe: () => boolean, ms = 1_500): Promise<boolean> {
  const deadline = Date.now() + ms;
  while (Date.now() < deadline) {
    if (probe()) return true;
    await new Promise((resolve) => setTimeout(resolve, 25));
  }
  return probe();
}

export async function run(): Promise<void> {
  PDFReader.configureMotion(64 * 1024 * 1024, 8 * 1024 * 1024, 4, 0.4);

  // The harness's document has five pages, so the windows below are a five-page
  // document's: a reader near the end, projected onto the last page.
  // --- one lane goes to the destination, not to the page being left ---------
  PDFReader.registerPage(1, "cont-30-cv", "cont-30-pg");
  PDFReader.registerPage(5, "cont-41-cv", "cont-41-pg");
  const before = PDFReader.stats().scheduler;
  // A fling down the document, projected to land on page 5: the full window is
  // 4-5, the preview ring 2-5, and ONE lane to spend.
  PDFReader.setScrollMotion("fling", 1, 5, 4, 5, 2, 5, 0, 1);
  const [outrun, destination] = await Promise.all([
    PDFReader.renderPage("cont-30-cv", 1.0, false),
    PDFReader.renderPage("cont-41-cv", 1.0, false),
  ]);
  if (fmt(destination) !== "ok") {
    throw new Error("the predicted page did not render: " + fmt(destination));
  }
  if (fmt(outrun) !== "cancelled") {
    throw new Error("a page outside every published window still rendered: " + fmt(outrun));
  }
  const after = PDFReader.stats().scheduler;
  if (after.dropped <= before.dropped) {
    throw new Error("the outrun request was not dropped before it ran");
  }
  if (after.savedPixels <= before.savedPixels) {
    throw new Error("a dropped request did not record the work it saved");
  }
  console.log("scheduler ok: the destination took the lane, the page behind was dropped");

  // --- pacing: a request superseded inside the delay never runs -------------
  PDFReader.registerPage(4, "cont-42-cv", "cont-42-pg");
  PDFReader.setScrollMotion("fast", 1, 4, 3, 5, 2, 5, 25, 1);
  const pacedBefore = PDFReader.stats().scheduler;
  const [superseded, paced] = await Promise.all([
    PDFReader.renderPage("cont-42-cv", 1.0, false),
    PDFReader.renderPage("cont-42-cv", 1.1, false),
  ]);
  if (fmt(superseded) !== "cancelled") {
    throw new Error("the superseded request was not resolved: " + fmt(superseded));
  }
  if (fmt(paced) !== "ok") throw new Error("the paced request never ran: " + fmt(paced));
  if (PDFReader.stats().scheduler.superseded <= pacedBefore.superseded) {
    throw new Error("coalescing two requests for one canvas did not supersede the first");
  }
  console.log("scheduler ok: one canvas, one raster — the superseded request never ran");

  // --- the preview tier is its own tier, and a full render leaves it --------
  PDFReader.setScrollMotion("fast", 1, 4, 3, 5, 2, 5, 0, 1);
  const preview = await PDFReader.renderPage("cont-42-cv", 1.0, false, true);
  if (fmt(preview) !== "ok") throw new Error("preview render failed: " + fmt(preview));
  if (PDFReader.stats().memory.previews !== 1) {
    throw new Error("the preview tier did not record its surface");
  }
  const promoted = await PDFReader.renderPage("cont-42-cv", 1.0, true, false);
  if (fmt(promoted) !== "ok") throw new Error("promotion failed: " + fmt(promoted));
  if (PDFReader.stats().memory.previews !== 0) {
    throw new Error("a full render left the page counted as a preview");
  }
  console.log("scheduler ok: preview and full are separate tiers, and promotion leaves the ring");

  // --- a settled reader is graded on where they stopped ---------------------
  const predictionsBefore = PDFReader.stats().predictions;
  PDFReader.setScrollMotion("fling", 1, 5, 4, 5, 2, 5, 0, 2);
  PDFReader.setScrollMotion("idle", 0, 5, 4, 5, 4, 5, 0, 2);
  const graded = PDFReader.stats().predictions;
  if (graded.made <= predictionsBefore.made || graded.hits <= predictionsBefore.hits) {
    throw new Error(
      `the arrival was not graded against the prediction: ${JSON.stringify(graded)}`,
    );
  }
  console.log("prediction telemetry ok:", JSON.stringify(graded));

  // --- the representative page: one miniature for every placeholder ---------
  const published = await waitFor(() => rootProperty("--pdf-placeholder").startsWith("url("));
  if (!published) {
    throw new Error(
      "no representative page was published: " + JSON.stringify(rootProperty("--pdf-placeholder")),
    );
  }
  console.log("placeholder ok:", rootProperty("--pdf-placeholder").slice(0, 32) + "…");

  // Leave the engine settled: a published window that outlives this scenario
  // would pace the teardown's own calls, and nothing after this point is a
  // strip.
  PDFReader.setScrollMotion("idle", 0, 0, 0, 0, 0, 0, 0, 0);
  PDFReader.unregisterPage("cont-30-cv");
  PDFReader.unregisterPage("cont-41-cv");
  PDFReader.unregisterPage("cont-42-cv");
}
