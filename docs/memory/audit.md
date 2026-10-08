# Memory audit

Scope: every subsystem that allocates surfaces, caches, timers, or
workers, checked against [rules.md](rules.md). Method: read each owner's
allocation, release, and teardown paths; verify bounds in code; verify
teardown in the smoke suite and the browser lifecycle baseline.

## Verdicts

| Subsystem | Owner | Verdict | Evidence |
| --- | --- | --- | --- |
| Virtualizer windowing and retention | `crates/virtual-list-leptos` | Pass | Window = viewport + overscan; zombies bounded and motion-gated (`MAX_ZOMBIES = 12`; a bridge is granted only while the strip seeks, one `FRAME_CEILING_MS = 120` frame per eviction); `dispose()` clears the retention timer (`virtualizer.rs:454`) |
| Fling gate and in-view exemption | `components/formats/pdf/canvas.rs`, `strip.rs` | Pass | The page starts its raster only while the virtualizer's motion band calls it Active; no placeholder pixels; see [fling-gate.md](fling-gate.md) |
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
| Route realm policy | `src/app/manager.rs`, `frame.rs` | Pass | Library has its own WASM; every Library return removes the Reader host and all live/incoming/retiring document realms. No Reader prewarm, recycle, empty retention or idle eviction window; Library is likewise disposed/removed while reading, including cover-bake queue/task/page. Neither route warms/recycles; both return fresh. Symmetric physical residency, cancelled boots, fresh Library identity and scoped bake cancellation are asserted by the browser/memory harnesses. |
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


## Historical Reader-only route teardown audit (2026-10-02)

Measured runtime revision: `3448499b5e315b6d6086a69b5d35ac5edd83d3e1`.
This replay predates symmetric Library teardown: that version retained Library
behind Reader. These numbers are historical workload evidence, not measurements
of the current unload-both policy.
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

## Symmetric route ownership

Both route artifacts now use the same lifetime: incoming Ready/Painted,
reveal, outgoing Dispose/ack, remove the iframe. There is no warm/recycle
slot or rearm protocol. Reading must report zero Library frames, balanced
Library creates/disposals and no Library-owned cover-bake page; returning
must instantiate a fresh Library generation and discard all Reader realms.
The Shell and durable data survive. A brief bounded handoff overlap is not
background retention.

Library teardown cancels its queued/in-flight/idle cover page. Abort stops
cover loading/render tasks and zeros the offscreen backing store before
frame removal; late cover/boot answers are refused by owner/generation.
Its deferred startup timer and scoped effects/listeners are cancelled on
unmount. Cancelling an incoming Library preserves the Reader that remained
visible; returning to it restores the route phase without a window reload.

The existing Actions memory replay records Library frame residency and
create/dispose counts during four-pane reading and continuous-reader cycles,
and requires a new Library identity plus zero Reader/document frames on
return. No process-RAM savings are inferred from the source change alone;
WebKit process retention from prior audits remains unresolved until measured.

## Cleanup ownership review

Launch resolution has one registry, not a ticket map plus an open map. Its
future owns a weak registry reference; dropping it removes its request and
waker. Answers and disposal remove entries before waking continuations, and
late replies retain no abandoned descriptor. Reader checks requester liveness
after awaiting. Duplicate session launch snapshots and empty document-realm
preboots were removed; per-pane signals remain authoritative.

The current titlebar now installs its native window-state updater. Probes are
coalesced and use scoped `try_` reads/writes after awaits. A disposed native
subscription is inert immediately, but a registration still in flight keeps
its callback until it can unlisten; unlisten always precedes callback release.
These ownership rules are checked against real Library WASM using a controlled
native boundary in the browser suite. This does not assert full process-RAM
recovery or resolve the previously measured WebKit retention.

## Vocabulary dataset review

The vocabulary feature's memory surface was re-read against `rules.md` after
its review pass: the pane's level cache (whole-clear at `LEVEL_CACHE_CAP`),
the row-text LRU (`SCAN_CAP`), the dataset manager's three loaded things (the
SQLite handle, the tagger model, the phase probe), and the frontend's
per-realm mirror and tap. Each has one owner and one drop: a document change
resets the pane's cache, removing the dataset drops the connection, the tagger
and the probe together, the realm clears its mirror on cleanup, and the tap
installs once per realm. The painters re-plan only on the generation they
already watch, so no work starts on a dead owner.

Two behaviours were changed in the review rather than written around. A click
no longer fetches the multi-megabyte POS model by itself: the settings panel
states the model's absence (or its pause) and carries its own ask, so the
first byte of a download is always a visible one. And the download now ends in
a terminal snapshot every consumer sees — `done` with the finished path,
`failed` with its reason — so the manager starts the database rebuild from a
hook instead of polling, and a refused start cannot leave a sheet on a bar
that never moves. The transport's resumability (mirror rotation, `If-Range`,
416, stall, and cancel on a paused record) is exercised against a loopback
server in that crate's own tests rather than asserted here.

An adversarial pass followed that review, and three of its findings were
bugs, not style. The transport read its resume validator from the partial
rather than from the destination, where it is written, so every proven
partial looked unproven and every resume restarted from zero — the loopback
suite now proves a paused transfer continues from its byte count. A directory
part of `""` passed the bare-name check and quietly landed in a directory the
caller did not name; it is refused now. And a request whose file is already
at its destination is answered as `done`, with the path and without a
request, which is what makes the destination the crate's cache and a rebuild
retry cost no bytes. One unlisted event went out as well: the app's `Host`
emitted `download-progress` with no window listening, so it no longer
publishes at all — the manager's hook is the channel, and the terminal
snapshot it receives is what starts the rebuild.

The merged branch then met the gate this section had not: Deep CI's browser
lifecycle baseline, red on `main` at the large-PDF stage. The realm's mirror was
bound only in a Tauri build, so in a browser the first asker created the handle
— and the first asker was a *page host*, recycled on every long scroll. The
thread-local kept that dead signal, and the next read of it panicked
(`reactive_graph`: a disposed reactive value) — the frontend reading of rule 7,
"module-level state holds sessions weakly". The mirror is now bound by the
install scope in every build, an unbound ask gets a private handle that is never
cached, the two derived readiness signals read with `try_with`, and the
scheduled measurement in `pdf.rs` re-checks its scale and mark handles at the
frame edge (rule 8) instead of assuming they live.

This entry is a code audit, not a measurement: no memory replay was taken of
the marking sweep, and no process-RAM claim is made for it. What it does
establish is that the feature adds no unbounded structure — its caches are
bounded and drop with their owner, and nothing is retained per frame — which
is the property the rules require before a release claim could be measured.
