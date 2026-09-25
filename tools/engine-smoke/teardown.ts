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
  // Prefetch is lane work now: a warmup/idle prefetch must be visible as
  // active work while it runs and resolved (completed or dropped) after the
  // document dies — an unaccounted side channel here is exactly the leak
  // shape the baseline exists to catch.
  await PDFReader.prefetchThumb(2, 0.25);
  const midPrefetchStats = PDFReader.stats();
  if (midPrefetchStats.prefetchesStarted < 1) {
    throw new Error("prefetch was not counted as started work");
  }
  PDFReader.unregisterPage("reopen-0-cv");
  await PDFReader.destroy();
  assertDrained(PDFReader.stats(), "rapid reopen + prefetch + close");

  // quiesce is the work-stop half of a close intent: the cancel a
  // boundary-crossing teardown cannot make in time, run in the click's own
  // task. An unawaited render is in flight when it lands; it must resolve
  // cancelled/dropped AND be counted, while the session survives — the one
  // teardown stays destroy's, and destroy after quiesce must still drain
  // with every counter pair balanced (the shared sweep counts nothing
  // twice). A facade member missing from the bundle throws HERE rather than
  // silently skipping in the reader.
  const quiesceOpen = await PDFReader.open("/fake/book.pdf");
  if (!quiesceOpen.ok) throw new Error(`quiesce open failed: ${quiesceOpen.error.message}`);
  PDFReader.registerPage(1, "quiesce-0-cv", "quiesce-0");
  const beforeQuiesce = PDFReader.stats();
  const inFlight = PDFReader.renderPage("quiesce-0-cv", 1.0, false);
  PDFReader.quiesce();
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
  PDFReader.quiesce(); // a repeated close intent is a no-op, not a second sweep
  PDFReader.unregisterPage("quiesce-0-cv");
  await PDFReader.destroy();
  assertDrained(PDFReader.stats(), "quiesce + close");

  // destroy() with nothing open (the open flow runs it as its first act):
  // a no-op dispose must not count a session that never existed. quiesce
  // guards the same way: without a document it is nothing, counts nothing.
  PDFReader.quiesce();
  await PDFReader.destroy();
  assertDrained(PDFReader.stats(), "destroy on an empty session");
}
