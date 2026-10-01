# Memory audit

Scope: every subsystem that allocates surfaces, caches, timers, or
workers, checked against [rules.md](rules.md). Method: read each owner's
allocation, release, and teardown paths; verify bounds in code; verify
teardown in the smoke suite and the browser lifecycle baseline.

## Verdicts

| Subsystem | Owner | Verdict | Evidence |
| --- | --- | --- | --- |
| Virtualizer windowing and retention | `crates/virtual-list-leptos` | Pass | Window = viewport + overscan; zombies bounded (`MAX_ZOMBIES = 12`, 120 ms grace); `dispose()` takes the retention timer (`virtualizer.rs:357`) |
| Fling gate and in-view exemption | `components/formats/pdf/canvas.rs`, `strip.rs` | Pass | Speed-aware exemption with a timer wake; no placeholder pixels; see [fling-gate.md](fling-gate.md) |
| Page render lane | `public/engine/renderer.ts`, `state.ts` | Pass | Per-session queue + realm cap 2; drained unconditionally on teardown; queued jobs drop on `st.dead / s.disposed / queueGen` |
| Lane pump registry | `public/engine/state.ts` | Pass (fixed) | `WeakRef` entries, pruned on every pump; session entry dropped first in `destroySession` |
| Thumbnail lane and cache | `public/engine/thumbnails.ts` | Pass | Cache capped (`THUMB_CACHE_MAX`), entries released bitmap-first (`releaseThumbEntry`), prefetch epoch-guarded and cancelled on teardown |
| Canvas pool and scratch | `public/engine/canvas.ts` | Pass | `POOL_MAX = 6`, oversized-return guard, `releaseCanvas` zeroes the backing store, `disposeScratch` drains on idle and on destroy |
| Bake worker | `public/engine/theme/bake.ts`, `bake.worker.ts` | Pass | Readbacks happen in the worker via transferred `ImageBitmap`, closed after the draw; dead worker rejects all pending; worker terminated when no session remains (`releaseBakeWorker`) |
| pdf.js document and worker | `public/engine/loader.ts`, `renderer.ts` | Pass | Sweeps on the render cadence, the 30 s idle timer, scroll-idle, zoom landings, and mode flips; `destroyTask` terminates the worker on session destroy |
| Scrub entry snapshots | `public/engine/theme/scrub.ts` | Pass (fixed) | Teardown zeroed and removed them via `releaseAllEntrySnapshots` instead of dereferencing the map; WKWebView keeps an IOSurface behind a canvas that is only removed from the DOM |
| Zoom masks (page snapshots) | `components/formats/pdf/surface.rs`, `state.ts` | Pass | `remove_snapshots` zeroes backing stores; swept at zoom landings, mode flips, and `sweepSnapshots` |
| View mode change | `effects/reader/mode_change.rs` | Pass | Fires `pdf.sweep()` and `pdf.sweep_snapshots()` at the flip — the outgoing view's rasters have no later render to sweep them |
| Paper backdrop and stash | `effects/reader/blend_backdrop.rs`, `engine/paper.ts` | Pass | Backdrop borrows, never clones, on scroll ticks; stash frames are ≤96 px, consumed once (`takePaperFrame`), and reset per document |
| Pane-scoped PDF theme pipeline and paper observers | `public/engine/state.ts`, `theme/pipeline.ts`, `theme/paper.ts` | Pass | `EngineSession` holds one bounded token/input cache + its pane-root reference; root observer is per session and disconnected at `destroySession` start; weak-map keys do not retain sessions; no subtree-wide observer; 1×1 scratch canvas is returned in `finally`. Two-session bake/paper isolation is covered by engine smoke. |
| LUT cache | `public/engine/theme/filterKernel.ts` | Pass | Capped at 8; the comment records why clear-beats-LRU here |
| Cover bake page | `src/app/bake.rs` | Pass | One page, deduplicated queue, removed seconds after the queue drains |
| Frame recycle/retire policy | `src/app/manager.rs` | Pass | Multi-pane or over-ceiling readers retire (frame removed); warm reader evicted after 60 s idle; heap ceiling 320 MiB |
| Raw canvas retention | `public/engine/state.ts` | Pass | `dropRawIfIdle` after `RAW_IDLE_MS = 2000`, no-op while scrubbing or the appearance menu is open, cleared outright on teardown |

## Teardown fix from the audit

`destroySession` cleared `scrub.entrySnapshots` by dropping the map, which
dereferences the snapshot canvases without zeroing them. On WKWebView the
backing store survives DOM removal until GC. Teardown now calls
`releaseAllEntrySnapshots`, the same zero-and-remove path the scrub exit
uses.

## Split-theme memory measurement

`tools/measure-split-return.mjs` in Deep CI, Chromium hands-off scenario: fresh library 181.2 MB renderer PSS; reading
with four panes 271.1 MB; +0 s after returning to Library 222.6 MB; +70 s
150.8 MB with `readerFramesResident = 0`; after forced GC 125.2 MB. The
reader WASM high-water gauge stayed 2.3 MB. Measurement is process PSS, not
a claim that the browser returned every allocation immediately. The same
replay completed for current and pinned baselines, Chromium/WebKit and both
pointer-intent modes (Deep CI `memory-replay` artifact). This change adds pane-root theme observations, not
retained raster copies or page canvases; local papers are computed on one
1×1 scratch buffer released in `finally`.

## Known residuals, not defects

- WebKit does not return process memory promptly. After-return PSS stays
  near the reading peak for minutes even when every holder is gone. The
  replay logs record this on every commit; it is runtime behaviour, not an
  engine retention.
- The shipped replay harness scrolls once per pane. It measures teardown,
  not churn. Motion-path changes need a scroll-heavy workload before their
  memory numbers mean anything ([rules.md](rules.md), rule 10).
