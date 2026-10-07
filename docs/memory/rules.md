# Memory rules

Rules for code that allocates surfaces, caches, timers, or workers in this
repository. Each rule names the failure that produced it. Violations show up
as a footprint that does not come back at idle; the diagnosis path is at the
bottom.

## 1. The unit of release is the frame, not the object graph

WASM linear memory only grows. Rust `Drop` runs destructors inside it but
never returns pages to the browser, and a JS realm keeps every module
instance ever loaded into it. Memory comes back when the browsing context
dies: Library and Reader host have separate route iframe/WASM artifacts;
each Reader document has an independently owned child iframe. Disposal
removes those frames ([pane-runtimes.md](../pane-runtimes.md)). Shell contains
neither runtime implementation. Library return must remove the Reader host
AND every live/incoming/retiring child, with no retained or prewarmed Reader
behind Library. Entering Reader must likewise dispose/remove the Library
realm and its cover-bake queue/page. Neither route may warm or recycle;
returning creates a fresh realm from durable data. Settings/persistence
authority stays in Shell. Design
teardown so the frame can die; do not keep graphs that outlive it.

## 2. Never start expensive work for an item the strip is sweeping past

A page crossing the visible band during a fling is not commitment. The
fling gate defers unpainted pages to the scroll settle; the in-view
exemption renders at once at reading speed and only after a short dwell
mid-fling ([fling-gate.md](fling-gate.md)). Full surfaces created and discarded every
few frames push the webview's resource cache — and the footprint latched
onto it — to a high-water mark that does not come back at idle. Any new
exemption from a gate must carry a time condition or an equivalent
commitment check.

## 3. Band-crossing signals need a commitment check, and a guaranteed wake

Cheap reactions (a CSS class) may fire on the crossing itself.
Surface-allocating work needs evidence the reader will stay: low scroll
speed, or continuous duration in the band. A deferral must carry its own
wake (a timer or trigger), never wait on a later event that may not come.

## 4. Sweep on quiescence, not on motion

Document-wide cleanup (pdf.js `cleanup()`, scratch and pool drains,
snapshot release) runs at settle points: the render cadence
(`CLEANUP_EVERY`), the idle timer (`SWEEP_IDLE_MS`), the reader's
scroll-idle hook, zoom landings, and mode flips. Per-unmount sweeps during
scroll re-parse the pages the next scroll remounts. When removing a sweep
call site, name the quiescence paths that replace it in the commit.

## 5. Every cache and pool states its bound and its drain

A cache without a bound or a drain is a leak with extra steps. Existing
bounds: canvas pool `POOL_MAX = 6` with oversized-return guard, LUT cache
`LUT_CACHE_MAX = 8`, thumbnail cache `THUMB_CACHE_MAX`, zombies
`MAX_ZOMBIES = 12` / 120 ms grace, the rail's 6 cells / 4 frames capped at
`FRAME_CEILING_MS` a frame, page lane `PAGE_RENDER_LIMIT = 2` and
realm cap `REALM_PAGE_LIMIT = 2`, and host cap `WINDOW_RASTER_LIMIT = 2`
with at most two requests per pane realm (weak host wakes, cancelled on
session teardown, reclaimed by scoped nonce on pane removal, and reclaimed
for all descendants on Reader-host removal). Raw canvases
survive `RAW_IDLE_MS = 2000` after a theme change and no longer. A new cache needs both numbers in the
same comment.

## 6. Canvas release means zeroing the backing store

Removing a canvas from the DOM does not free it: WKWebView keeps the
IOSurface until the backing store is zeroed, and Chromium returns it
promptly only when dimensions drop to zero. Use `releaseCanvas` (engine) /
`remove_snapshots` (Rust) — never rely on DOM removal or dereference alone.
This applies to pooled canvases, snapshots, scratch pads, and any offscreen
canvas a code path creates.

## 7. Module-level state holds sessions weakly

The realm outlives every session. Any module-scoped collection that keys on
or captures a session (`EngineSession`, `PdfSession`) must use `WeakRef`
and prune dead entries, or be emptied deterministically at the top of
teardown before any failable step. A strong module reference to a session
pins its surfaces, caches, and pdf proxy past its close.

## 8. Queued work re-checks liveness at the rAF edge and at the lane front

A job queued for a page or a session can outlive both. The drop checks are
`st.dead || s.disposed || st.queueGen !== gen` in
`public/engine/renderer.ts`; the thumbnail lane uses epoch + generation the
same way. New lanes copy the pattern; do not write a lane that starts work
on a dead owner.

## 9. Teardown is observable or it is not done

Every paired resource has counters or gauges the test suite reads:
`sessionsOpened == sessionsDestroyed`, `workersCreated == workersTerminated`,
render and prefetch pairings, drained lane gauges, `retainedVirtualItems`.
A teardown change must keep the paired numbers balanced in
`scripts/test-engine-smoke.js` and the browser lifecycle baseline.

## 10. Claims of release need measurements

"The component unmounted" is not a measurement. Record the workload, the
metric, before/after values, and the reporting limits of the runtime
(WebKit in particular does not return process memory promptly). The replay
harnesses (`tools/measure-split-return.mjs`, `tools/measure-split-cycles.mjs`)
and `PDFReader.stats()` are the instruments. Note that the shipped replay
scrolls once per pane: it measures teardown, not churn. A change that
affects what renders during motion needs a scroll-heavy workload before its
numbers can be trusted.

## Diagnosis path

Symptom: memory high at idle after a change to a rendering or scroll path.

1. Run `PDFReader.stats()` in the live frame. `sessionsLive > 0` at the
   library route means a session is still held open; the gauges
   (`pageCanvasBytesEst`, `thumbnailRasterBytesEst`, `rawRetentionBytesEst`,
   `pooledIntermediateBytesEst`) name what it holds.
2. `sessionsLive == 0` with memory still high points at a latched browser
   footprint (rule 2) or runtime-side retention (WebKit).
3. For motion-path changes, enumerate the surfaces created per item swept
   past and multiply by scroll throughput; reproduce with sustained
   scrolling, then sample before, during, and 60 s after.

