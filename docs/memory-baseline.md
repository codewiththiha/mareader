# MAReader memory and lifecycle baseline (Phase 0)

Phase 0 deliverable: the repeatable procedure, the instrumentation it reads,
and what the ownership map says to expect before the runtime migration
changes anything. Companion to `docs/lifecycle-ownership.md`.

This phase is instrumentation and evidence. Nothing here redesigns the
architecture; the numbers below exist so the later phases' claims about
teardown and retention are checkable instead of narrative.

## The instrumentation

### Rust surface (`src/diagnostics.rs`)

- Lifecycle counters: reader runtime create/dispose (one pair per
  open/close flow), pane create/dispose (the `/reader` surface), virtualizer
  create/dispose with live window and zombie counts (the four reader-surface
  strips register themselves).
- `disposalEpoch` — the document session's claim stamp; it moves on every
  open and close, so two snapshots can never be confused about which moment
  they describe.
- Wasm heap size and high-water mark (fed by the existing `src/memory.rs`
  probe, which still logs its `[mem]` lines at open/close/zoom/search).
- The engine's half, read live at snapshot time:
  `pdf_engine::api::engine_stats()`.

### Engine surface (`public/engine/*`)

`PDFReader.stats()` now reports the full resource picture: live gauges
(`pages`, `thumbs`, `thumbTasks`, `activeRenders`, `activePrefetches`,
`hasDocument`, `hasLoadingTask`) and the monotonic lifecycle counters with
pairing rules:

```text
sessionsOpened    == sessionsDestroyed
workersCreated    == workersTerminated
rendersStarted    == rendersCompleted + rendersCancelled + rendersFailed
prefetchesStarted == prefetchesCompleted + prefetchesDropped
```

`workersTerminated` is counted only after pdf.js's worker shutdown round
trip resolves, and `destroy()` awaits it — so a balanced worker pair means
the worker is actually dead, not that death was scheduled. Thumbnail
prefetch (the warmup and the idle cache fills) is lane work: bounded by the
thumbnail lane, registered in `thumbTasks` under `prefetch-<page>` ids so
teardown cancels it, epoch-guarded at every await so a stale prefetch never
lands in the next document, and fully counted.

The narration (`pdf_session:*`, `pdf_worker:*`, `render:*`,
`thumb_prefetch:*` events) is opt-in and silent in normal operation; the
counters are always on.

The look-ahead's active work is visible from the Rust side:
`pdf_engine::backdrop::pending_samples()` feeds the snapshot's
`lookaheadSamplesActive` — pages whose offscreen colour sample is in
flight — and it must read zero after a dispose.

### The dev probe

In the app webview console:

```js
__mareaderDiagnostics()   // prints + returns the full JSON snapshot,
                          // with an `atBaseline` verdict, and turns
                          // lifecycle narration on
```

Off the webview (host tests) the surface is inert; the counters and the
`at_baseline()` check are still unit-tested on the host.

A note on the reader-side counters' semantics: `readerRuntimesCreated`
counts OPEN ATTEMPTS that claimed the document state — the boundary hook
today's architecture has for "a runtime began". A failed open is a create
whose dispose never needs to run, so the pairing these counters prove is
the close path's completion, not liveness; the liveness truth is
`readerRuntimeLive` plus the engine's `hasDocument`. Phase 1's explicit
runtime object replaces this hook with a real lifetime.

### CI enforcement

Two automated layers:

1. **Engine smoke suite** (`tools/engine-smoke/teardown.ts`, the web lane,
   every PR): against the bundle built from current source, after
   `destroy()`, after a rapid reopen + prefetch + close, and after a no-op
   destroy — every live gauge empty, every counter pair balanced,
   prefetches included.
2. **Browser lifecycle baseline** (`tests/browser/`, the Deep CI lane):
   the REAL built app in a REAL Chromium — real wasm, real pdf.js worker,
   real renders — driven through opening a shipped sample book with blend
   (look-ahead) enabled, the warmup prefetch, fast navigation with the
   look-ahead observed ACTIVE, zoom pressure, a close during active work,
   waiting for `atBaseline`, asserting every reader/engine counter drained
   and balanced, and a reopen that repeats the cycle. The workload the
   guide demands, automated against the production build; its per-stage
   measurement table is the recorded baseline below.

## The benchmark procedures

### Automated (Deep CI, every run)

The browser lane runs the documented workload matrix end to end and prints
the per-stage measurement table (`=== PHASE0 BROWSER BASELINE ===`): after
open, after the warmup, during scroll, after zoom, during fast jumps, after
each raced close (render / prefetch / search), and the final state after
the normal x10, large x5 and rapid-reopen x10 cycles, plus a summary line
with the fast-jump render deltas and the reopen heap steps. Those tables
are the recorded baseline — same command, same environment, every run,
comparable across commits.

### Manual (Tauri/WKWebView, per significant change)

Run against a dev build (`trunk serve`, or `cargo tauri dev`); record a
`__mareaderDiagnostics()` snapshot at every marked point, plus the process
RSS from the OS (Task Manager / `ps` / Activity Monitor) as the second,
non-interchangeable signal. Look-ahead stays ENABLED in every workload.

Measure, per snapshot: `wasmHeapBytes`, `heapHighWaterBytes`,
`engine.pages`, `engine.activeRenders`, `engine.thumbs`, `engine.thumbTasks`,
`engine.hasDocument`, `engine.hasLoadingTask`, `paneLive`, `virtualizerLive`,
`retainedVirtualItems`, `disposalEpoch`, and process RSS. Browser/WebView
APIs and OS measurements do not return memory to the OS immediately — read
them as different signals, and compare only like with like on the same build.

### Workloads

**A. Normal PDF lifecycle (x10)** — Library -> open a medium PDF -> read a
few pages -> close -> Library. Record: before open, after open settles,
after close settles. Expectation: every counter except the wasm heap and the
monotonic counters returns to its pre-open value; `disposalEpoch` advances
by exactly 1 per cycle.

**B. Large PDF lifecycle (x5)** — same, scrolling through the document.
Watch `retainedVirtualItems` during scroll (zombies are bounded and
transient — they must drain within the grace period after scrolling
settles).

**C. Fast-scroll pressure** — open a large PDF, fast-scroll between distant
ranges, pause, repeat, close. Record during (peaks are the point), after the
pause, and after close. `engine.pages` stays at the RENDER_BUDGET ceiling
during; returns to 0 after close.

**D. Zoom pressure** — rapid zoom changes, scroll, return near original
zoom, close. The zoom transients (scratch, bake output, snapshot masks) are
the peak to watch: `heapHighWaterBytes` records the latch; `pages` and
`activeRenders` must drain after close.

**E. Look-ahead pressure** — move steadily so look-ahead is active, close
DURING active prefetch. The close tail must complete (`disposalEpoch`
advances, `dispose_complete` fires) and the engine must drain despite the
in-flight work.

**F. Close during active work** — close while a page renders, a thumbnail
loads, search builds, look-ahead samples. Each variant: the counters must
balance (a cancelled render counts as `rendersCancelled`), and nothing
reader-owned survives.

**G. Rapid reopen (x10)** — open -> close -> reopen the same PDF. The
monotonic counters advance in lockstep; the interesting number is the wasm
heap: it ratchets by allocation (the platform never shrinks it), so judge
LEAK versus LATCH by whether the per-cycle step GROWS, not by whether the
level rises.

## Baseline results

### Recorded: browser lifecycle baseline (automated)

Environment: GitHub Actions `ubuntu-24.04`, headless Chromium (Playwright),
the production `trunk build --release` output served statically, sample
book *Programming Pearls (2nd Edition)* opened through the web test hook
with blend (look-ahead) enabled. The numbers below are pasted verbatim from
the `=== PHASE0 BROWSER BASELINE ===` tables the workflow prints; rerun the
lane to reproduce or to compare a change against it.

Recorded from Deep CI run **35983608664** (branch `split-wasm-modules-t10`,
commit `2b58f11`, 2026-09-24) — the full matrix with all three raced closes,
the fast-jump page-identity proof, the same-page reopen workload, and
fail-closed accounting, zero wasm traps::

```text
sessionsOpened    == sessionsDestroyed
workersCreated    == workersTerminated
rendersStarted    == rendersCompleted + rendersCancelled + rendersFailed
prefetchesStarted == prefetchesCompleted + prefetchesDropped
```

`workersTerminated` is counted only after pdf.js's worker shutdown round
trip resolves, and `destroy()` awaits it — so a balanced worker pair means
the worker is actually dead, not that death was scheduled. Thumbnail
prefetch (the warmup and the idle cache fills) is lane work: bounded by the
thumbnail lane, registered in `thumbTasks` under `prefetch-<page>` ids so
teardown cancels it, epoch-guarded at every await so a stale prefetch never
lands in the next document, and fully counted.

The narration (`pdf_session:*`, `pdf_worker:*`, `render:*`,
`thumb_prefetch:*` events) is opt-in and silent in normal operation; the
counters are always on.

The look-ahead's active work is visible from the Rust side:
`pdf_engine::backdrop::pending_samples()` feeds the snapshot's
`lookaheadSamplesActive` — pages whose offscreen colour sample is in
flight — and it must read zero after a dispose.

### The dev probe

In the app webview console:

```js
__mareaderDiagnostics()   // prints + returns the full JSON snapshot,
                          // with an `atBaseline` verdict, and turns
                          // lifecycle narration on
```

Off the webview (host tests) the surface is inert; the counters and the
`at_baseline()` check are still unit-tested on the host.

A note on the reader-side counters' semantics: `readerRuntimesCreated`
counts OPEN ATTEMPTS that claimed the document state — the boundary hook
today's architecture has for "a runtime began". A failed open is a create
whose dispose never needs to run, so the pairing these counters prove is
the close path's completion, not liveness; the liveness truth is
`readerRuntimeLive` plus the engine's `hasDocument`. Phase 1's explicit
runtime object replaces this hook with a real lifetime.

### CI enforcement

Two automated layers:

1. **Engine smoke suite** (`tools/engine-smoke/teardown.ts`, the web lane,
   every PR): against the bundle built from current source, after
   `destroy()`, after a rapid reopen + prefetch + close, and after a no-op
   destroy — every live gauge empty, every counter pair balanced,
   prefetches included.
2. **Browser lifecycle baseline** (`tests/browser/`, the Deep CI lane):
   the REAL built app in a REAL Chromium — real wasm, real pdf.js worker,
   real renders — driven through opening a shipped sample book with blend
   (look-ahead) enabled, the warmup prefetch, fast navigation with the
   look-ahead observed ACTIVE, zoom pressure, a close during active work,
   waiting for `atBaseline`, asserting every reader/engine counter drained
   and balanced, and a reopen that repeats the cycle. The workload the
   guide demands, automated against the production build; its per-stage
   measurement table is the recorded baseline below.

## The benchmark procedures

### Automated (Deep CI, every run)

The browser lane runs the documented workload matrix end to end and prints
the per-stage measurement table (`=== PHASE0 BROWSER BASELINE ===`): after
open, after the warmup, during scroll, after zoom, during fast jumps, after
each raced close (render / prefetch / search), and the final state after
the normal x10, large x5 and rapid-reopen x10 cycles, plus a summary line
with the fast-jump render deltas and the reopen heap steps. Those tables
are the recorded baseline — same command, same environment, every run,
comparable across commits.

### Manual (Tauri/WKWebView, per significant change)

Run against a dev build (`trunk serve`, or `cargo tauri dev`); record a
`__mareaderDiagnostics()` snapshot at every marked point, plus the process
RSS from the OS (Task Manager / `ps` / Activity Monitor) as the second,
non-interchangeable signal. Look-ahead stays ENABLED in every workload.

Measure, per snapshot: `wasmHeapBytes`, `heapHighWaterBytes`,
`engine.pages`, `engine.activeRenders`, `engine.thumbs`, `engine.thumbTasks`,
`engine.hasDocument`, `engine.hasLoadingTask`, `paneLive`, `virtualizerLive`,
`retainedVirtualItems`, `disposalEpoch`, and process RSS. Browser/WebView
APIs and OS measurements do not return memory to the OS immediately — read
them as different signals, and compare only like with like on the same build.

### Workloads

**A. Normal PDF lifecycle (x10)** — Library -> open a medium PDF -> read a
few pages -> close -> Library. Record: before open, after open settles,
after close settles. Expectation: every counter except the wasm heap and the
monotonic counters returns to its pre-open value; `disposalEpoch` advances
by exactly 1 per cycle.

**B. Large PDF lifecycle (x5)** — same, scrolling through the document.
Watch `retainedVirtualItems` during scroll (zombies are bounded and
transient — they must drain within the grace period after scrolling
settles).

**C. Fast-scroll pressure** — open a large PDF, fast-scroll between distant
ranges, pause, repeat, close. Record during (peaks are the point), after the
pause, and after close. `engine.pages` stays at the RENDER_BUDGET ceiling
during; returns to 0 after close.

**D. Zoom pressure** — rapid zoom changes, scroll, return near original
zoom, close. The zoom transients (scratch, bake output, snapshot masks) are
the peak to watch: `heapHighWaterBytes` records the latch; `pages` and
`activeRenders` must drain after close.

**E. Look-ahead pressure** — move steadily so look-ahead is active, close
DURING active prefetch. The close tail must complete (`disposalEpoch`
advances, `dispose_complete` fires) and the engine must drain despite the
in-flight work.

**F. Close during active work** — close while a page renders, a thumbnail
loads, search builds, look-ahead samples. Each variant: the counters must
balance (a cancelled render counts as `rendersCancelled`), and nothing
reader-owned survives.

**G. Rapid reopen (x10)** — open -> close -> reopen the same PDF. The
monotonic counters advance in lockstep; the interesting number is the wasm
heap: it ratchets by allocation (the platform never shrinks it), so judge
LEAK versus LATCH by whether the per-cycle step GROWS, not by whether the
level rises.

## Baseline results

### Recorded: browser lifecycle baseline (automated)

Environment: GitHub Actions `ubuntu-24.04`, headless Chromium (Playwright),
the production `trunk build --release` output served statically, sample
book *Programming Pearls (2nd Edition)* opened through the web test hook
with blend (look-ahead) enabled. The numbers below are pasted verbatim from
the `=== PHASE0 BROWSER BASELINE ===` tables the workflow prints; rerun the
lane to reproduce or to compare a change against it.

Recorded from Deep CI run **35977320462** (branch `split-wasm-modules-t10`,
commit `7db2d91`, 2026-09-24) — the full matrix with all three raced closes,
zero wasm traps, verified stable across a repeat run of the same commit:

```text
=== PHASE0 BROWSER BASELINE (chromium, release wasm build) ===
policy: window ceiling 3 | zombie cap 12 | page lane 2 | thumb lane 3 | min fixture pages 40
--- afterOpen ---
{"disposalEpoch":1,"readerPage":1,"readerRuntimeLive":true,"paneLive":1,"virtualizerLive":3,"virtualizerListeners":2,"virtualizerObservers":2,"virtualizerTimers":0,"liveWindowItems":19,"retainedVirtualItems":0,"lookaheadSamplesActive":0,"engine":{"activePrefetches":0,"activeRenders":0,"documentPages":40,"hasDocument":true,"hasLoadingTask":true,"pageActive":0,"pageCanvasBytesEst":3877632,"pageQueue":0,"pages":2,"pooledIntermediateBytesEst":4,"prefetchesCompleted":0,"prefetchesDropped":0,"prefetchesStarted":0,"rawRetentionBytesEst":0,"rawRetentionTimers":0,"rendersCancelled":0,"rendersCompleted":2,"rendersDropped":0,"rendersFailed":0,"rendersQueued":2,"rendersStarted":2,"searchActive":0,"sessionsDestroyed":0,"sessionsOpened":1,"sweepTimerArmed":1,"thumbActive":0,"thumbGenerationSize":0,"thumbLimit":16,"thumbQueue":0,"thumbTasks":0,"thumbnailRasterBytesEst":0,"thumbs":0,"workersCreated":1,"workersTerminated":0},"wasmHeapBytes":1835008,"heapHighWaterBytes":1835008,"liveCanvasBytes":3877632,"jsHeapBytes":10000000,"atBaseline":false}
--- afterWarmup ---
{"disposalEpoch":1,"readerPage":1,"readerRuntimeLive":true,"paneLive":1,"virtualizerLive":3,"virtualizerListeners":2,"virtualizerObservers":2,"virtualizerTimers":0,"liveWindowItems":19,"retainedVirtualItems":0,"lookaheadSamplesActive":0,"engine":{"activePrefetches":0,"activeRenders":0,"documentPages":40,"hasDocument":true,"hasLoadingTask":true,"pageActive":0,"pageCanvasBytesEst":3877632,"pageQueue":0,"pages":2,"pooledIntermediateBytesEst":4,"prefetchesCompleted":16,"prefetchesDropped":0,"prefetchesStarted":16,"rawRetentionBytesEst":0,"rawRetentionTimers":0,"rendersCancelled":0,"rendersCompleted":2,"rendersDropped":0,"rendersFailed":0,"rendersQueued":2,"rendersStarted":2,"searchActive":0,"sessionsDestroyed":0,"sessionsOpened":1,"sweepTimerArmed":1,"thumbActive":0,"thumbGenerationSize":0,"thumbLimit":16,"thumbQueue":0,"thumbTasks":0,"thumbnailRasterBytesEst":3877632,"thumbs":16,"workersCreated":1,"workersTerminated":0},"wasmHeapBytes":1835008,"heapHighWaterBytes":1835008,"liveCanvasBytes":3877632,"jsHeapBytes":10000000,"atBaseline":false}
--- duringScroll ---
{"disposalEpoch":1,"readerPage":18,"readerRuntimeLive":true,"paneLive":1,"virtualizerLive":3,"virtualizerListeners":2,"virtualizerObservers":2,"virtualizerTimers":2,"liveWindowItems":20,"retainedVirtualItems":1,"lookaheadSamplesActive":0,"engine":{"activePrefetches":0,"activeRenders":0,"documentPages":40,"hasDocument":true,"hasLoadingTask":true,"pageActive":0,"pageCanvasBytesEst":2478816,"pageQueue":0,"pages":4,"pooledIntermediateBytesEst":4,"prefetchesCompleted":16,"prefetchesDropped":0,"prefetchesStarted":16,"rawRetentionBytesEst":0,"rawRetentionTimers":0,"rendersCancelled":0,"rendersCompleted":14,"rendersDropped":0,"rendersFailed":0,"rendersQueued":14,"rendersStarted":14,"searchActive":0,"sessionsDestroyed":0,"sessionsOpened":1,"sweepTimerArmed":1,"thumbActive":0,"thumbGenerationSize":0,"thumbLimit":16,"thumbQueue":0,"thumbTasks":0,"thumbnailRasterBytesEst":3877632,"thumbs":16,"workersCreated":1,"workersTerminated":0},"wasmHeapBytes":1835008,"heapHighWaterBytes":1835008,"liveCanvasBytes":2478816,"jsHeapBytes":10000000,"atBaseline":false}
--- afterZoom ---
{"disposalEpoch":1,"readerPage":18,"readerRuntimeLive":true,"paneLive":1,"virtualizerLive":3,"virtualizerListeners":2,"virtualizerObservers":2,"virtualizerTimers":2,"liveWindowItems":20,"retainedVirtualItems":1,"lookaheadSamplesActive":0,"engine":{"activePrefetches":0,"activeRenders":0,"documentPages":40,"hasDocument":true,"hasLoadingTask":true,"pageActive":0,"pageCanvasBytesEst":7852944,"pageQueue":0,"pages":4,"pooledIntermediateBytesEst":4,"prefetchesCompleted":16,"prefetchesDropped":0,"prefetchesStarted":16,"rawRetentionBytesEst":0,"rawRetentionTimers":0,"rendersCancelled":0,"rendersCompleted":28,"rendersDropped":0,"rendersFailed":0,"rendersQueued":28,"rendersStarted":28,"searchActive":0,"sessionsDestroyed":0,"sessionsOpened":1,"sweepTimerArmed":1,"thumbActive":0,"thumbGenerationSize":0,"thumbLimit":16,"thumbQueue":0,"thumbTasks":0,"thumbnailRasterBytesEst":3877632,"thumbs":16,"workersCreated":1,"workersTerminated":0},"wasmHeapBytes":1835008,"heapHighWaterBytes":1835008,"liveCanvasBytes":7852944,"jsHeapBytes":10000000,"atBaseline":false}
--- duringFastJump ---
{"disposalEpoch":1,"readerPage":11,"readerRuntimeLive":true,"paneLive":1,"virtualizerLive":3,"virtualizerListeners":2,"virtualizerObservers":2,"virtualizerTimers":1,"liveWindowItems":20,"retainedVirtualItems":0,"lookaheadSamplesActive":0,"engine":{"activePrefetches":0,"activeRenders":0,"documentPages":40,"hasDocument":true,"hasLoadingTask":true,"pageActive":0,"pageCanvasBytesEst":5816448,"pageQueue":0,"pages":3,"pooledIntermediateBytesEst":4,"prefetchesCompleted":16,"prefetchesDropped":0,"prefetchesStarted":16,"rawRetentionBytesEst":0,"rawRetentionTimers":0,"rendersCancelled":0,"rendersCompleted":35,"rendersDropped":0,"rendersFailed":0,"rendersQueued":35,"rendersStarted":35,"searchActive":0,"sessionsDestroyed":0,"sessionsOpened":1,"sweepTimerArmed":1,"thumbActive":0,"thumbGenerationSize":0,"thumbLimit":16,"thumbQueue":0,"thumbTasks":0,"thumbnailRasterBytesEst":3877632,"thumbs":16,"workersCreated":1,"workersTerminated":0},"wasmHeapBytes":1835008,"heapHighWaterBytes":1835008,"liveCanvasBytes":5816448,"jsHeapBytes":10000000,"atBaseline":false}
--- afterCloseDuringRender ---
{"disposalEpoch":2,"readerPage":1,"readerRuntimeLive":false,"paneLive":0,"virtualizerLive":0,"virtualizerListeners":0,"virtualizerObservers":0,"virtualizerTimers":0,"liveWindowItems":0,"retainedVirtualItems":0,"lookaheadSamplesActive":0,"engine":{"activePrefetches":0,"activeRenders":0,"documentPages":0,"hasDocument":false,"hasLoadingTask":false,"pageActive":0,"pageCanvasBytesEst":0,"pageQueue":0,"pages":0,"pooledIntermediateBytesEst":0,"prefetchesCompleted":16,"prefetchesDropped":0,"prefetchesStarted":16,"rawRetentionBytesEst":0,"rawRetentionTimers":0,"rendersCancelled":2,"rendersCompleted":35,"rendersDropped":1,"rendersFailed":0,"rendersQueued":38,"rendersStarted":37,"searchActive":0,"sessionsDestroyed":1,"sessionsOpened":1,"sweepTimerArmed":0,"thumbActive":0,"thumbGenerationSize":0,"thumbLimit":16,"thumbQueue":0,"thumbTasks":0,"thumbnailRasterBytesEst":0,"thumbs":0,"workersCreated":1,"workersTerminated":1},"wasmHeapBytes":1835008,"heapHighWaterBytes":1835008,"liveCanvasBytes":0,"jsHeapBytes":10000000,"atBaseline":true}
--- afterCloseDuringPrefetch ---
{"disposalEpoch":2,"readerPage":1,"readerRuntimeLive":false,"paneLive":0,"virtualizerLive":0,"virtualizerListeners":0,"virtualizerObservers":0,"virtualizerTimers":0,"liveWindowItems":0,"retainedVirtualItems":0,"lookaheadSamplesActive":0,"engine":{"activePrefetches":0,"activeRenders":0,"documentPages":0,"hasDocument":false,"hasLoadingTask":false,"pageActive":0,"pageCanvasBytesEst":0,"pageQueue":0,"pages":0,"pooledIntermediateBytesEst":0,"prefetchesCompleted":0,"prefetchesDropped":1,"prefetchesStarted":1,"rawRetentionBytesEst":0,"rawRetentionTimers":0,"rendersCancelled":0,"rendersCompleted":3,"rendersDropped":0,"rendersFailed":0,"rendersQueued":3,"rendersStarted":3,"searchActive":0,"sessionsDestroyed":1,"sessionsOpened":1,"sweepTimerArmed":0,"thumbActive":0,"thumbGenerationSize":0,"thumbLimit":16,"thumbQueue":0,"thumbTasks":0,"thumbnailRasterBytesEst":0,"thumbs":0,"workersCreated":1,"workersTerminated":1},"wasmHeapBytes":1835008,"heapHighWaterBytes":1835008,"liveCanvasBytes":0,"jsHeapBytes":10000000,"atBaseline":true}
--- afterCloseDuringSearch ---
{"disposalEpoch":2,"readerPage":1,"readerRuntimeLive":false,"paneLive":0,"virtualizerLive":0,"virtualizerListeners":0,"virtualizerObservers":0,"virtualizerTimers":0,"liveWindowItems":0,"retainedVirtualItems":0,"lookaheadSamplesActive":0,"engine":{"activePrefetches":0,"activeRenders":0,"documentPages":0,"hasDocument":false,"hasLoadingTask":false,"pageActive":0,"pageCanvasBytesEst":0,"pageQueue":0,"pages":0,"pooledIntermediateBytesEst":0,"prefetchesCompleted":0,"prefetchesDropped":0,"prefetchesStarted":0,"rawRetentionBytesEst":0,"rawRetentionTimers":0,"rendersCancelled":0,"rendersCompleted":3,"rendersDropped":0,"rendersFailed":0,"rendersQueued":3,"rendersStarted":3,"searchActive":0,"sessionsDestroyed":1,"sessionsOpened":1,"sweepTimerArmed":0,"thumbActive":0,"thumbGenerationSize":0,"thumbLimit":16,"thumbQueue":0,"thumbTasks":0,"thumbnailRasterBytesEst":0,"thumbs":0,"workersCreated":1,"workersTerminated":1},"wasmHeapBytes":1835008,"heapHighWaterBytes":1835008,"liveCanvasBytes":0,"jsHeapBytes":10000000,"atBaseline":true}
--- afterRapidReopen ---
{"disposalEpoch":2,"readerPage":1,"readerRuntimeLive":false,"paneLive":0,"virtualizerLive":0,"virtualizerListeners":0,"virtualizerObservers":0,"virtualizerTimers":0,"liveWindowItems":0,"retainedVirtualItems":0,"lookaheadSamplesActive":0,"engine":{"activePrefetches":0,"activeRenders":0,"documentPages":0,"hasDocument":false,"hasLoadingTask":false,"pageActive":0,"pageCanvasBytesEst":0,"pageQueue":0,"pages":0,"pooledIntermediateBytesEst":0,"prefetchesCompleted":0,"prefetchesDropped":0,"prefetchesStarted":0,"rawRetentionBytesEst":0,"rawRetentionTimers":0,"rendersCancelled":0,"rendersCompleted":3,"rendersDropped":0,"rendersFailed":0,"rendersQueued":3,"rendersStarted":3,"searchActive":0,"sessionsDestroyed":1,"sessionsOpened":1,"sweepTimerArmed":0,"thumbActive":0,"thumbGenerationSize":0,"thumbLimit":16,"thumbQueue":0,"thumbTasks":0,"thumbnailRasterBytesEst":0,"thumbs":0,"workersCreated":1,"workersTerminated":1},"wasmHeapBytes":1835008,"heapHighWaterBytes":1835008,"liveCanvasBytes":0,"jsHeapBytes":10000000,"atBaseline":true}
--- afterSamePage ---
{"disposalEpoch":22,"readerPage":1,"readerRuntimeLive":false,"paneLive":0,"virtualizerLive":0,"virtualizerListeners":0,"virtualizerObservers":0,"virtualizerTimers":0,"liveWindowItems":0,"retainedVirtualItems":0,"lookaheadSamplesActive":0,"engine":{"activePrefetches":0,"activeRenders":0,"documentPages":0,"hasDocument":false,"hasLoadingTask":false,"pageActive":0,"pageCanvasBytesEst":0,"pageQueue":0,"pages":0,"pooledIntermediateBytesEst":0,"prefetchesCompleted":0,"prefetchesDropped":0,"prefetchesStarted":0,"rawRetentionBytesEst":0,"rawRetentionTimers":0,"rendersCancelled":0,"rendersCompleted":32,"rendersDropped":0,"rendersFailed":0,"rendersQueued":32,"rendersStarted":32,"searchActive":0,"sessionsDestroyed":11,"sessionsOpened":11,"sweepTimerArmed":0,"thumbActive":0,"thumbGenerationSize":0,"thumbLimit":16,"thumbQueue":0,"thumbTasks":0,"thumbnailRasterBytesEst":0,"thumbs":0,"workersCreated":11,"workersTerminated":11},"wasmHeapBytes":2097152,"heapHighWaterBytes":2097152,"liveCanvasBytes":0,"jsHeapBytes":10000000,"atBaseline":true}
--- summary ---
{"fixturePages":{"pearls":40,"deepOutline":40},"fastJumpRenderDeltas":[2,2,3],"scrollPeaks":{"enginePages":4,"activeRenders":2,"pageActive":2,"pageQueue":1,"thumbActive":0,"thumbQueue":0,"retainedVirtualItems":1,"liveWindowItems":20,"lookaheadSamplesActive":1,"liveCanvasBytes":5996448,"wasmHeapBytes":1835008,"jsHeapBytes":10000000,"pageCanvasBytesEst":5996448,"thumbnailRasterBytesEst":3877632,"rawRetentionBytesEst":0,"pooledIntermediateBytesEst":4},"zoomPeaks":{"enginePages":4,"activeRenders":2,"pageActive":2,"pageQueue":0,"thumbActive":0,"thumbQueue":0,"retainedVirtualItems":2,"liveWindowItems":20,"lookaheadSamplesActive":0,"liveCanvasBytes":11875248,"wasmHeapBytes":1835008,"jsHeapBytes":10000000,"pageCanvasBytesEst":11875248,"thumbnailRasterBytesEst":3877632,"rawRetentionBytesEst":0,"pooledIntermediateBytesEst":4},"fastJumpPeaks":{"enginePages":5,"activeRenders":1,"pageActive":1,"pageQueue":0,"thumbActive":0,"thumbQueue":0,"retainedVirtualItems":3,"liveWindowItems":20,"lookaheadSamplesActive":0,"liveCanvasBytes":9694080,"wasmHeapBytes":1835008,"jsHeapBytes":10000000,"pageCanvasBytesEst":9694080,"thumbnailRasterBytesEst":3877632,"rawRetentionBytesEst":0,"pooledIntermediateBytesEst":4},"largeScrollPeaks":{"enginePages":6,"activeRenders":1,"pageActive":1,"pageQueue":0,"thumbActive":1,"thumbQueue":0,"retainedVirtualItems":3,"liveWindowItems":20,"lookaheadSamplesActive":3,"liveCanvasBytes":6356448,"wasmHeapBytes":2031616,"jsHeapBytes":10000000,"pageCanvasBytesEst":6356448,"thumbnailRasterBytesEst":3877632,"rawRetentionBytesEst":0,"pooledIntermediateBytesEst":4},"closeDuringRenderRaced":true,"closeDuringPrefetchDrops":1,"closeDuringSearchRaced":true,"rapidReopenHeaps":[1835008,1835008,1835008,1835008,1835008,1835008,1835008,1835008,1835008,1835008],"rapidReopenSlopeBytesPerCycle":0,"rapidReopenDriftBytes":0,"normalCycles":10,"largeCycles":5,"rapidCycles":10,"samePageOpenHeaps":[1900544,1900544,1900544,1966080,1966080,1966080,2031616,2031616,2031616,2097152],"samePagePooledBytes":[0,0,0,0,0,0,0,0,0,0],"samePageSlopeBytesPerCycle":21448,"samePageDriftBytes":196608,"samePageCycles":10}
PHASE0_BASELINE_JSON {"fixturePages":{"pearls":40,"deepOutline":40},"fastJumpRenderDeltas":[2,2,3],"scrollPeaks":{"enginePages":4,"activeRenders":2,"pageActive":2,"pageQueue":1,"thumbActive":0,"thumbQueue":0,"retainedVirtualItems":1,"liveWindowItems":20,"lookaheadSamplesActive":1,"liveCanvasBytes":5996448,"wasmHeapBytes":1835008,"jsHeapBytes":10000000,"pageCanvasBytesEst":5996448,"thumbnailRasterBytesEst":3877632,"rawRetentionBytesEst":0,"pooledIntermediateBytesEst":4},"zoomPeaks":{"enginePages":4,"activeRenders":2,"pageActive":2,"pageQueue":0,"thumbActive":0,"thumbQueue":0,"retainedVirtualItems":2,"liveWindowItems":20,"lookaheadSamplesActive":0,"liveCanvasBytes":11875248,"wasmHeapBytes":1835008,"jsHeapBytes":10000000,"pageCanvasBytesEst":11875248,"thumbnailRasterBytesEst":3877632,"rawRetentionBytesEst":0,"pooledIntermediateBytesEst":4},"fastJumpPeaks":{"enginePages":5,"activeRenders":1,"pageActive":1,"pageQueue":0,"thumbActive":0,"thumbQueue":0,"retainedVirtualItems":3,"liveWindowItems":20,"lookaheadSamplesActive":0,"liveCanvasBytes":9694080,"wasmHeapBytes":1835008,"jsHeapBytes":10000000,"pageCanvasBytesEst":9694080,"thumbnailRasterBytesEst":3877632,"rawRetentionBytesEst":0,"pooledIntermediateBytesEst":4},"largeScrollPeaks":{"enginePages":6,"activeRenders":1,"pageActive":1,"pageQueue":0,"thumbActive":1,"thumbQueue":0,"retainedVirtualItems":3,"liveWindowItems":20,"lookaheadSamplesActive":3,"liveCanvasBytes":6356448,"wasmHeapBytes":2031616,"jsHeapBytes":10000000,"pageCanvasBytesEst":6356448,"thumbnailRasterBytesEst":3877632,"rawRetentionBytesEst":0,"pooledIntermediateBytesEst":4},"closeDuringRenderRaced":true,"closeDuringPrefetchDrops":1,"closeDuringSearchRaced":true,"rapidReopenHeaps":[1835008,1835008,1835008,1835008,1835008,1835008,1835008,1835008,1835008,1835008],"rapidReopenSlopeBytesPerCycle":0,"rapidReopenDriftBytes":0,"normalCycles":10,"largeCycles":5,"rapidCycles":10,"samePageOpenHeaps":[1900544,1900544,1900544,1966080,1966080,1966080,2031616,2031616,2031616,2097152],"samePagePooledBytes":[0,0,0,0,0,0,0,0,0,0],"samePageSlopeBytesPerCycle":21448,"samePageDriftBytes":196608,"samePageCycles":10}
=== END PHASE0 BROWSER BASELINE ===
```

Read of the run: the warmup is hard — 16 prefetches start and all 16
complete. The three memory categories stay distinct throughout:
`wasmHeapBytes` is the wasm linear memory's ratchet, `liveCanvasBytes` is
the canvas backing stores the strip currently holds (the category the
original "blank screen, exploding RAM" complaint was about — it is NOT the
browser's whole footprint), and `jsHeapBytes` is the browser-reported JS
heap where the API is available (the lane records it best-effort and the
baseline stands when it is not). A jump across most of the 40-page
document costs 2–3 rasters (the destination window, not the pages flown
over) with the render budget
asserted from `RENDER_BUDGET`, not a loose page count. All three races
were won: close during a render left renders cancelled + dropped, close
during a prefetch dropped a warmup prefetch, close during a search build
drained the build gauge to 0 — and every raced close still reached the
full baseline with the lanes empty (`pageQueue`/`thumbQueue` 0,
`searchActive` 0) and every counter pairing intact. The rapid-reopen gate
is the headline: ten open/close cycles hold 1,835,008 bytes flat —
least-squares slope 0 B/cycle, drift 0 B, no ratchet, no per-cycle growth.

Four proofs ride the same run. PAGE IDENTITY: the fast jumps carry a
bounded engine render trace (generation, page, terminal phase) and each
jump asserted every rasterized page inside the destination window the
policy allows (window ceiling + zombie cap each side) — `18 -> 40` traced
`[39, 40]` inside 25..40, `40 -> 1` traced `[1, 2]` inside 1..16, `1 -> 11`
traced `[10, 11, 12]` inside 1..26 — render counts cannot name a stray
page; the trace does. SAME-PAGE LIFECYCLE: ten open/close cycles through
the real library row and toolbar close WITHOUT reloading the page — the
scope where the wasm module, the library caches and the recycler live —
each close returned everything to baseline and the disposal epoch advanced
exactly one claim per open and per close (base 2, closes at 4, 6, ... 22);
the heap climbed 21,448 B/cycle on average with 192 KiB total drift (an
allocator ratchet, far under the 256 KiB/cycle leak bound) and the raster
recycler drifted 0 B. FAIL-CLOSED ACCOUNTING: every post-close assertion
requires the cumulative pane/virtualizer create/dispose pairs to balance,
the live counts to equal created-minus-disposed, and the snapshot's
`accountingConsistent` to hold — a double dispose now fails the baseline
instead of reading as a quiet zero. RASTER BYTES: the engine exposes
estimated raster categories (page surfaces, thumbnail cache, retained
raws, bake recycler; w*h*4, labeled estimates, never a share of
wasmHeap/jsHeap/RSS) and the per-document categories must drain to ZERO
BYTES after every close, not merely to zero counts.

The zero-trap record held through the whole matrix and was re-verified by
a repeat run of the same commit after the last teardown class was closed:
callbacks and effects that ride LIVE strip-scope signals (geometry
reports, flush/scroll frames, the dominant-page sync) can be re-run one
teardown beat after the reader state is gone, where a read or write on the
purged signals panics. Every such site now probes liveness with the
non-panicking `try_*` reads — the strip report's document/epoch gates, the
virtualizer's flush/apply/publish entries, the estimate closures, the
navigation-sync arms, the auto-center steps, and the anchor/first-paint
rAF tails. The thumbnail generation bookkeeping gained the same shape: a
document's lane opens with its document and `resetThumbLane` closes it, so
a straggling request cannot reseed the map the teardown just cleared.

### Structural findings (hold for any build until later phases change them)


### Expected post-close baseline (every workload)

```text
reader_runtime_live = false        panes_live = 0
virtualizer_live = 0               retained_virtual_items = 0
engine.hasDocument = false         engine.hasLoadingTask = false
engine.pages = 0                   engine.activeRenders = 0
engine.thumbs = 0                  engine.thumbTasks = 0
panesCreated == panesDisposed      virtualizersCreated == virtualizersDisposed
paneLive == panesCreated - panesDisposed   accountingConsistent = true
engine.pageCanvasBytesEst = 0      engine.thumbnailRasterBytesEst = 0
engine.rawRetentionBytesEst = 0    (recycler: recorded, drift-gated)
sessionsOpened == sessionsDestroyed
workersCreated == workersTerminated
rendersStarted == rendersCompleted + rendersCancelled + rendersFailed
```

Exact wasm-heap level and OS RSS returning to pre-open values are NOT part
of the baseline: the wasm linear memory never shrinks, and WebView
allocators retain freed arenas. The success condition is the elimination of
retained OWNERSHIP, not a synchronous return of bytes to the OS.

### Known retention sources, by design (named, not guessed)

1. **The wasm heap never shrinks** — `Memory.grow` is monotonic. The
   search index retained across close (2), the parsed library blob and the
   heap's high-water allocations live inside that ratchet.
2. **The search index survives close** (`api/search.rs`, keyed by content
   fingerprint) so a reopen adopts it instead of re-extracting every page.
   A different book's open drops it. This is the deliberate trade the
   migration keeps until Phase 4 replaces it with an explicit
   session-scoped owner.
3. **The covers cache** persists for the app's lifetime (library state).
4. **The engine session object** is a module singleton — destroyed and
   re-created per open today; it stays reachable from the module, which is
   correct now and becomes the Phase 1/4 boundary.
5. **The paper/backdrop session** is a thread-local, reset on close but
   reachable from module scope — same trajectory.
6. **WebKit/WebView latching** — freed canvas IOSurfaces and JS arenas may
   not return to the OS; `releaseCanvas`/snapshot sweeping exist to hand
   the surfaces back, and the OS may still hold them under pressure.

### What the map says about ownership (the Phase 0 acceptance)

- The PDF session is owned by `public/engine/state.ts`'s `EngineSession`
  (module-global today), driven through `services/document/close.rs` and
  the next open's pre-destroy. Provable via `hasDocument` +
  `sessionsOpened/Destroyed`.
- Render tasks are owned by the engine's page lane
  (`renderPageInternal`/`st.renderTask`), provable via `activeRenders` and
  the started/resolved balance.
- Look-ahead/prefetch is owned by the backdrop thread-local `Session`
  (epoch-guarded spawns) and the bounded warmup fire — named, bounded, and
  epoch-invalidated.
- Virtualizer bindings are owned by `VirtualizerInner` and disposed in the
  hook's cleanup; the registry proves the count returns to zero.
- Reader disposal completion is detectable: the close tail emits
  `dispose_complete` AFTER the sweeps, and `disposalEpoch` names the moment.

## Proposed Phase 1 ownership boundary (from the evidence)

The migration's first ownership change should make the engine session an
explicit, instance-owned object rather than a module singleton, with the
close path owning its disposal as today but the lifetime no longer held by
module scope:

1. Introduce an explicit reader-runtime/session object on the Rust side that
   OWNS the open document's identity (the claim stamp becomes its handle)
   and the engine session's lifetime, replacing "whoever calls destroy
   eventually" with one owner.
2. Keep the search-index adoption as an explicit handover between sessions
   (a scoped transfer on close/open) instead of a thread-local survivor.
3. Move the backdrop session under the same explicit lifetime (epoch token
   already exists; make the owner explicit).
4. Leave routing, virtualization and appearance untouched — they are later
   phases (Shell/Library/Reader split, Host+Pane).

Success for that phase is exactly this document's post-close baseline,
enforced by the same counters, with the module-global owners from
`docs/lifecycle-ownership.md` gone or reduced to explicit, owned handles.
