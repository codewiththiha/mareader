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
| Page render lane | `public/engine/renderer.ts`, `state.ts`, `public/rasterLane.ts` | Pass | Session/realm cap 2 plus window cap 2; host holds weak wakes/plain keys; session cancels pending permits; post-permit liveness check; frame removal reclaims only its nonce |
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
| Route realm policy | `src/app/manager.rs`, `frame.rs` | Pass | Library has its own WASM; every Library return removes the Reader host and all live/incoming/retiring document realms. No Reader prewarm, recycle, empty retention or idle eviction window; only Library may wait behind Reader. Browser cancellation/mixed-close proof and four current Chromium/WebKit return samples pass. |
| Raw canvas retention | `public/engine/state.ts` | Pass | `dropRawIfIdle` after `RAW_IDLE_MS = 2000`, no-op while scrubbing or the appearance menu is open, cleared outright on teardown |

## Teardown fix from the audit

`destroySession` cleared `scrub.entrySnapshots` by dropping the map, which
dereferences the snapshot canvases without zeroing them. On WKWebView the
backing store survives DOM removal until GC. Teardown now calls
`releaseAllEntrySnapshots`, the same zero-and-remove path the scrub exit
uses.

## Historical split-theme memory measurement (before disposable Reader hosts)

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

## Document-frame completion audit (2026-10-02)

The starting build linked 29 `PDFReader` references in both PDF and reflow
WASM glue, despite only the PDF page loading the JS engine. Feature-isolated
builds now prohibit those imports in text (and retain the existing Shell
prohibition); shared document/report DTOs no longer pull in browser code.

Source inspection found/fixed these persistent-host lifetime issues:

- A retirement deadline swept every retiring frame. It now removes only
  its own nonce; a newer realm keeps its own disposal deadline.
- Final JSON reports accumulated in an unbounded vector. They now reduce
  to one plain-data record, preserving lifetime pairs and failed evidence.
- Animation-loop trampolines captured their own Rc slot. The slot is weak;
  grab cleanup explicitly removes listeners and cancels its hold/fling.
- Thumbnail bitmap creation/delivery could resume after cancellation and
  repopulate a zeroed canvas. Both sides re-check ownership and close late
  or untransferred bitmaps; proxy backing stores are always zeroed.
- Reflow stream measurements skipped during a fit tween had no completion
  wake. The measurement effect now tracks zoom completion, preserving the
  mid-tween guard while settling real row heights after fit/resize. Desktop
  and narrow browser checks require reported rows not to overlap.

**Verification method:** GitHub Actions production artifacts and existing
Chromium/WebKit split-return/close-cycle replay, plus the real-browser pane
regression stage and desktop/narrow captures. No local compiler/build,
Node/Rust/browser dependency installation, or local process-memory test.
Pending run results must not be described as measured savings. The replay
is teardown-oriented (one scroll per pane); it does not prove a scroll-heavy
churn/peak improvement. Browser process PSS also includes shared resource
caches and does not imply instantaneous WASM/IOSurface collection.


## Disposable route-realm audit (2026-10-02)

Measured runtime revision: `3448499b5e315b6d6086a69b5d35ac5edd83d3e1`.
[CI](https://github.com/codewiththiha/mareader/actions/runs/37005308362)
and [Deep CI](https://github.com/codewiththiha/mareader/actions/runs/37005308324)
passed for that exact revision. The dist artifact is `11225702961`; memory
replay evidence is `11226421164`. The old pinned replay artifacts had expired,
so these are current-build workload/return measurements, not an architectural
before/after comparison. Later documentation-only amendments do not change
the measured runtime code.

Five artifact types are loaded in separate realms: persistent Shell,
Library, disposable Reader host, PDF document and Markdown/TXT document.
Shell links neither route implementation. Reader host and text glue contain
no PDF engine. The Reader host owns global grain once; document frames own
no redundant grain layer. A DOM kind marker is lower-case, so the physical
census includes every Reader host's descendants, not just its visible pane.

The browser suite observed 1,743 nonblank, exclusive-visible route-handoff
samples. Actual blocked Reader-WASM navigation was cancelled by browser
Back; a blocked reflow-WASM replacement was discarded by closing a mixed
workspace. Late responses resurrected neither host nor panes, and Shell's
identity survived. Mixed close returned four opened/four destroyed PDF
sessions and zero active/queued/owned raster permits. Desktop 1,400 px and
narrow 640 px captures were reviewed: three placed panes, no viewport
overflow, measured reflow rows not overlapping, and text without PDF code.

### Four-pane return replay

GitHub Actions Linux production build, 1,600 × 1,000 viewport, three PDF
panes plus one Markdown pane, one scroll per pane, return to Library with
hands off or the pointer continuously over the shelf. PSS below sums browser
content processes in MiB; it is not WASM heap size or total application RAM.

| Engine / pointer | Fresh Library | Reading four panes | +2 s | +20 s | +70 s | GC / settle |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| Chromium / hands off | 168.1 | 347.9 | 257.6 | 145.0 | 141.6 | 138.5 |
| Chromium / shelf intent | 167.9 | 343.4 | 253.2 | 142.5 | 141.6 | 138.0 |
| WebKit / hands off | 471.7 | 823.0 | 777.1 | 771.0 | 771.9 | 771.5 |
| WebKit / shelf intent | 434.7 | 806.7 | 753.3 | 746.4 | 746.5 | 746.4 |

In every scenario Reader hosts and document frames were **zero by +2 s**,
baseline was true, and the raster lane had zero owners/active/queued work
through +70 s. The +0 s sample may still include the bounded retiring host:
Library is already visible while explicit graceful disposal finishes.
WebKit's last column is a settle sample, not forced GC. Linux Playwright
WebKit is not a native macOS WKWebView memory measurement.

### Continuous-Reader cycle limitation

The additional split/close replay keeps one PDF open while opening and
closing a neighbor: 16 Chromium cycles and 40 WebKit cycles. It does not
exercise whole-Reader Library return on each cycle. Final live session and
worker counts stayed at one, DOM counts were stable, and no closed pane
remained, but process PSS was not flat:

- Chromium: 2.934 MiB/cycle slope; 259.1 MiB after the last close,
  234.6 MiB after 30 s; JS heap slope −0.030 MiB/cycle.
- WebKit: 19.054 MiB/cycle slope; 1,513.7 MiB after the last close,
  1,381.8 MiB after 30 s and 1,659.8 MiB after allocation pressure.
- The surviving Reader-host WASM gauge grew 0.014 MiB/cycle; it is not a
  census of every pane's memory. WebKit supplied no JS heap measurement.

**Limit:** realm removal and balanced teardown are proved; immediate/full
process-memory recovery, flat WebKit repeated-pane memory, and a quantified
improvement over the prior architecture are not. The WebKit residual needs
separate heap/native-cache attribution before calling it harmless or fully
fixed. Do not reinterpret green lifecycle counters as a full-RAM-release
claim. No local compiler, bundler, dependency/browser installation or
process-memory test was used.
