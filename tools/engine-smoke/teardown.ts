import { PDFReader, type StatsPayload, R, openDoc, closeDoc } from "./harness.js";

/** The post-close baseline: every gauge empty, every counter pair balanced. */
function assertDrained(stats: StatsPayload, label: string): void {
  const problems: string[] = [];
  if (stats.pages !== 0) problems.push(`pages=${stats.pages}`);
  if (stats.thumbs !== 0) problems.push(`thumbs=${stats.thumbs}`);
  if (stats.thumbTasks !== 0) problems.push(`thumbTasks=${stats.thumbTasks}`);
  if (stats.activeRenders !== 0) problems.push(`activeRenders=${stats.activeRenders}`);
  if (stats.pageQueue !== 0) problems.push(`pageQueue=${stats.pageQueue}`);
  if (stats.pageActive !== 0) problems.push(`pageActive=${stats.pageActive}`);
  if (stats.thumbQueue !== 0) problems.push(`thumbQueue=${stats.thumbQueue}`);
  if (stats.thumbActive !== 0) problems.push(`thumbActive=${stats.thumbActive}`);
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
  if (stats.activePrefetches !== 0) problems.push(`activePrefetches=${stats.activePrefetches}`);
  if (stats.thumbGenerationSize !== 0) problems.push(`thumbGenerationSize=${stats.thumbGenerationSize}`);
  if (stats.rawRetentionTimers !== 0) problems.push(`rawRetentionTimers=${stats.rawRetentionTimers}`);
  if (stats.sweepTimerArmed !== 0) problems.push(`sweepTimerArmed=${stats.sweepTimerArmed}`);
  const prefetched = stats.prefetchesCompleted + stats.prefetchesDropped;
  if (stats.prefetchesStarted !== prefetched) {
    problems.push(`prefetches ${stats.prefetchesStarted} started vs ${prefetched} resolved`);
  }
  if (problems.length > 0) {
    throw new Error(`${label}: engine not drained after destroy — ${problems.join(", ")}`);
  }
  console.log(`${label}: drained (${JSON.stringify(stats)})`);
}

export async function run(): Promise<void> {
  // The narration is dev-only wiring: flip it on and off to exercise the flag.
  PDFReader.setLifecycleLog(true);
  PDFReader.setLifecycleLog(false);

  // The sweep walks the wiring: a member missing from the bundle throws here.
  R.sweep();
  R.sweepSnapshots();
  R.unregisterPage("cont-0-cv");
  R.unregisterPage("cont-1-cv");
  await closeDoc();
  // Both are idempotent on an empty session, as the shelf sweep shows.
  R.sweep();
  R.sweepSnapshots();
  assertDrained(PDFReader.stats(), "first close");

  // Rapid reopen: session and worker counters must stay balanced too.
  const reopened = await openDoc("/fake/book.pdf");
  if (!reopened.ok) throw new Error(`reopen failed: ${reopened.error.message}`);
  R.registerPage(1, "reopen-0-cv", "reopen-0");
  const rerendered = await R.renderPage("reopen-0-cv", 1.0, false);
  if (!rerendered.ok && rerendered.error.name !== "cancelled") {
    throw new Error(`reopen render failed: ${rerendered.error.message}`);
  }
  // Prefetch is lane work: visible while it runs, resolved when it dies.
  await R.prefetchThumb(2, 0.25);
  const midPrefetchStats = PDFReader.stats();
  if (midPrefetchStats.prefetchesStarted < 1) {
    throw new Error("prefetch was not counted as started work");
  }
  R.unregisterPage("reopen-0-cv");
  await closeDoc();
  assertDrained(PDFReader.stats(), "rapid reopen + prefetch + close");

  // quiesce is the work-stop half of a close: in-flight work is cancelled.
  const quiesceOpen = await openDoc("/fake/book.pdf");
  if (!quiesceOpen.ok) throw new Error(`quiesce open failed: ${quiesceOpen.error.message}`);
  R.registerPage(1, "quiesce-0-cv", "quiesce-0");
  const beforeQuiesce = PDFReader.stats();
  const inFlight = R.renderPage("quiesce-0-cv", 1.0, false);
  R.quiesce();
  const settled = await inFlight;
  if (settled.ok || settled.error.name !== "cancelled") {
    throw new Error("quiesce did not stop the in-flight render");
  }
  const afterQuiesce = PDFReader.stats();
  const stopped =
    afterQuiesce.rendersCancelled +
    afterQuiesce.rendersDropped -
    (beforeQuiesce.rendersCancelled + beforeQuiesce.rendersDropped);
  if (stopped < 1) throw new Error("quiesce stopped a render without counting it");
  if (!afterQuiesce.hasDocument) throw new Error("quiesce tore the session down");
  R.quiesce(); // a repeated close intent is a no-op, not a second sweep
  R.unregisterPage("quiesce-0-cv");
  await closeDoc();
  assertDrained(PDFReader.stats(), "quiesce + close");

  // destroy() with nothing open counts no session; quiesce guards the same way.
  R.quiesce();
  await closeDoc();
  assertDrained(PDFReader.stats(), "destroy on an empty session");
}
