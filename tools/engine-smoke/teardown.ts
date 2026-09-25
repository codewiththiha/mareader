import { PDFReader, type StatsPayload } from "./harness.js";

/** The engine half of the post-close baseline: every live gauge empty, and
 *  every lifecycle counter pair balanced — every session and worker died
 *  once, every started render resolved. A violation names the exact gauge,
 *  so a leaked page surface reads differently from an orphaned worker. */
function assertDrained(stats: StatsPayload, label: string): void {
  const problems: string[] = [];
  if (stats.pages !== 0) problems.push(`pages=${stats.pages}`);
  if (stats.thumbs !== 0) problems.push(`thumbs=${stats.thumbs}`);
  if (stats.thumbTasks !== 0) problems.push(`thumbTasks=${stats.thumbTasks}`);
  if (stats.activeRenders !== 0) problems.push(`activeRenders=${stats.activeRenders}`);
  if (stats.hasDocument) problems.push("hasDocument");
  if (stats.hasLoadingTask) problems.push("hasLoadingTask");
  if (stats.sessionsOpened !== stats.sessionsDestroyed) {
    problems.push(`sessions ${stats.sessionsOpened} opened vs ${stats.sessionsDestroyed} destroyed`);
  }
  if (stats.workersCreated !== stats.workersTerminated) {
    problems.push(`workers ${stats.workersCreated} created vs ${stats.workersTerminated} terminated`);
  }
  const resolved = stats.rendersCompleted + stats.rendersCancelled + stats.rendersFailed;
  if (stats.rendersStarted !== resolved) {
    problems.push(`renders ${stats.rendersStarted} started vs ${resolved} resolved`);
  }
  if (problems.length > 0) {
    throw new Error(`${label}: engine not drained after destroy — ${problems.join(", ")}`);
  }
  console.log(`${label}: drained (${JSON.stringify(stats)})`);
}

export async function run(): Promise<void> {
  // The lifecycle narration is dev-only wiring; flip it on and off here so
  // the facade member and the flag are exercised, not just compiled.
  PDFReader.setLifecycleLog(true);
  PDFReader.setLifecycleLog(false);

  // The settled-work sweep: the advisory worker cleanup plus the drop of any
  // zoom mask a superseded render left on a host. The stub hosts carry no
  // masks, so this walks the wiring — a facade member missing from the bundle
  // throws HERE rather than silently skipping in the reader.
  PDFReader.sweep();
  PDFReader.sweepSnapshots();
  PDFReader.unregisterPage("cont-0-cv");
  PDFReader.unregisterPage("cont-1-cv");
  await PDFReader.destroy();
  // Both are idempotent on an empty session: the shelf sweep after a close
  // runs them with nothing registered and no document.
  PDFReader.sweep();
  PDFReader.sweepSnapshots();
  assertDrained(PDFReader.stats(), "first close");

  // Rapid reopen: a second open/destroy cycle on the SAME session state.
  // The session and worker counters must stay balanced after a reopen, not
  // just after the first close — a monotonic drift here is exactly the
  // open/close-cycle leak the baseline workloads look for.
  const reopened = await PDFReader.open("/fake/book.pdf");
  if (!reopened.ok) throw new Error(`reopen failed: ${reopened.error.message}`);
  PDFReader.registerPage(1, "reopen-0-cv", "reopen-0");
  const rerendered = await PDFReader.renderPage("reopen-0-cv", 1.0, false);
  if (!rerendered.ok && rerendered.error.name !== "cancelled") {
    throw new Error(`reopen render failed: ${rerendered.error.message}`);
  }
  PDFReader.unregisterPage("reopen-0-cv");
  await PDFReader.destroy();
  assertDrained(PDFReader.stats(), "rapid reopen + close");

  // destroy() with nothing open (the open flow runs it as its first act):
  // a no-op dispose must not count a session that never existed.
  await PDFReader.destroy();
  assertDrained(PDFReader.stats(), "destroy on an empty session");
}
