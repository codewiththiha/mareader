# MAReader lifecycle and resource ownership map

Phase 0 deliverable. This is the measured starting point of the runtime
migration: who owns each resource today, traced from route mount to resource,
with every global/module-level owner and every piece of async work that can
outlive a route change named explicitly. It is the map the later phases
carve ownership boundaries along — and the checklist the disposal baseline
(`docs/memory-baseline.md`) asserts against.

## The shape of the app today

One root Leptos mount (`src/app`), one `AppState`
(`src/state/app.rs`) spanning library and reader, and a two-route shell:

```text
mount_to_body -> App
├── AppState (settings / reader / library / ui)  — one shared instance
├── Router
│   ├── "/"        -> LibraryPage (features/library)
│   └── "/reader"  -> ReaderPage (features/reader)   [the single reader "pane"]
└── app-lifetime effects (theme, motion, drag-drop, window bridge)
```

Route changes mount and unmount component trees, but the STATE and the
engine session are app-lifetime singletons — that is exactly what the later
phases replace. The inventory below is what exists now.

## Resource owners, traced

| Resource | Owner today | Created | Released |
| --- | --- | --- | --- |
| Document session (pdf.js proxy, loading task, page surfaces, thumb cache, search highlight state) | `public/engine/state.ts` `session` — one module-global `EngineSession` per webview | `open()` in `public/engine/loader.ts` | `destroy()` in `public/pdfEngine.ts` (called by `close_document` and by the next open) |
| pdf.js worker | Created inside `getDocument` per `LoadingTask`; the task is the handle | `openTask()` / `coverDataUrl()` own-task | `destroyTask()` — the single choke point for `task.destroy()` |
| Page render tasks | `PageState.renderTask` on the engine session, one bounded lane (`PAGE_RENDER_LIMIT = 2`) | `renderPageInternal` | cancel-on-supersede, `unregisterPage`, `destroy` |
| Page hosts (canvas + host registration) | `session.stateByCanvasId`, keyed by canvas id | `registerPage` (Rust: `src/components/formats/pdf/canvas.rs`) | `unregisterPage` (component `on_cleanup`), `destroy` |
| Thumbnails | `session.thumbCache` (LRU ≤ 16 pairs), `thumbTasks`, thumb lane | `renderThumb`/`prefetchThumb` | LRU eviction, `destroy` |
| Rust search index | `crates/pdf-engine/src/api/search.rs` thread-local — DELIBERATELY retained across close, keyed by content fingerprint | first search of a document | dropped when a DIFFERENT fingerprint is opened (`scope_to_document`) |
| Look-ahead (paper colour) | `crates/pdf-engine/src/backdrop/mod.rs` thread-local `Session` (`sampling` set + per-area palettes); tasks via `spawn_engine` | `document_open` / scroll ticks | `document_close` resets state; epoch token invalidates in-flight samples; in-flight count exposed as `backdrop::pending_samples()` (snapshot's `lookaheadSamplesActive`) |
| Thumbnail prefetch/warmup | `src/services/document/open/warmup.rs` fires a bounded timer; the ENGINE owns the work: each prefetch queues in the bounded thumbnail lane under the document's lane epoch | after open settles | teardown cancels in-flight prefetch tasks (registered under `prefetch-<page>` ids) and the epoch drops queued/awaited ones — never filed into the next document; lifecycle visible in `stats()` |
| Virtualizers (page strips, stream, thumbs grid) | `virtual_list_leptos::Virtualizer` handles held by components; bindings (listeners, ResizeObserver, timers) inside `VirtualizerInner` | `use_virtualizer` | `dispose()` via the hook's `on_cleanup` |
| Virtualizer measurement store | `DocumentState.content.metrics.css_heights` / `intrinsic` (app signals) | open seeds | `DocumentState::reset` |
| Reader reactive state | `AppState.reader` (`src/state/reader/*`) — app-lifetime signals, reset per close | bootstrap | reset by `close_document` (`DocumentState::reset`, `viewer.reset_position`, `search.reset`, `gloss.reset`, `ai_selection.reset`) |
| Gloss marks (in-memory) | `AppState.reader.gloss`, persisted per row id | open loads | `close_document` drops the copy (disk copy persists by design) |
| Covers | `AppState.library.covers` + `services/library/covers.rs` cache (quota-capped) | import / open tail | persists across sessions by design (library state) |
| Backdrop publication | `--pdf-paper` custom property on `<html>` + `pdf_engine::backdrop` published colour | paper session | `document_close` / `destroy()` republish to theme paper |
| Theme bake worker | `public/bake.worker.ts` — module-level worker per bake, created/terminated by the bake pipeline | bake start | pipeline end |
| App overlays, toasts, sidebar | `AppState.ui` | bootstrap | app lifetime (correct — shell chrome) |

## Global / static / module-level owners (the retention inventory)

These outlive every component and are the reasons a route change alone can
never release reader resources:

1. `public/engine/state.ts` `session` — the engine's one session object.
2. `src/services/document/session.rs` `SESSION` — the open/close claim stamp
   (also the diagnostics disposal epoch).
3. `crates/pdf-engine/src/api/search.rs` thread-local index + fingerprint
   scope — retained across close BY DESIGN (reopen adopts it).
4. `crates/pdf-engine/src/backdrop/mod.rs` thread-local paper `Session`.
5. `src/effects/app/library.rs`, `src/effects/reader/reflow_measure.rs`,
   `src/effects/app/shortcuts/navigation.rs`, `src/effects/appearance/mod.rs`
   thread-locals — app-lifetime effect bookkeeping (debounce cells, scroll
   state).
6. `src/services/library/covers.rs` and `import/claim.rs` thread-locals —
   import/cover queues (library-side by design).
7. `src/components/viewer/shells/scroll_shell.rs` thread-local,
   `src/components/ai/reflow_anchor.rs` thread-local — reader-surface
   bookkeeping that survives via module scope.
8. `crates/app-chrome/src/floating/dismiss.rs` — topmost-overlay registry
   (shell scope).
9. `public/pdfEngine.ts` module state: `themeChain` promise, the
   `pagehide`/`visibilitychange` listeners, `watchPaperTokens` mutation
   observer — installed once at bundle evaluation, never removed.
10. `src/memory.rs` probe + the new diagnostics counters
    (`src/diagnostics.rs`) — instrumentation is itself app-lifetime (bounded,
    numeric, and deliberately so).

## Async work that can outlive a route/component teardown

Every entry re-checks its guard after each `await`; none is cancelled by the
route change itself:

- Open tails (`src/services/document/open/*`): engine open, outline resolve,
  cover render, content seeding — all stamp-guarded
  (`services::document::session::owns`).
- Close tail (`src/services/document/close.rs`): `destroy().await` + sweeps +
  dispose-complete note — the one teardown that MUST finish; idempotent by
  design.
- Look-ahead samples (`backdrop::spawn_engine`): epoch-guarded, so a sample
  for one book never lands in the next.
- Thumbnail warmup (`open/warmup.rs`): unowned on the Rust side by design
  (a fire that must not reach into a possibly-disposed reader), but the
  ENGINE owns each prefetch as lane work: bounded by the lane limit,
  epoch-guarded at every await, cancelled by teardown, fully counted in
  `stats()` — the fire can therefore no longer touch the wrong document.
- Search index build (`src/effects/reader/search.rs`): one-build-at-a-time
  via `SearchState::building`; a close during a build leaves the index
  finishing into the retained slot (adoption contract above).
- Cover render queue (`services/library/covers.rs`) and import pipeline
  (`services/library/import/*`): library-side, run on the shelf by design.
- AI gloss chunk fetches (`services/ai.rs`, gloss controller): generation /
  owner-guarded against stale popovers.
- Engine-side: the theme mutation chain (`themeChain`), bake worker jobs,
  thumbnail lane queue, the 30s idle sweep timer (`EngineSession::idleTimer`)
  and the 2s raw-retention timers — all die with `destroy()`'s resets or are
  advisory.
- Virtualizer rAF coalescing and retention timer: owned by
  `VirtualizerInner`, cleared in `dispose()`.

## The disposal sequence as it exists today

```text
close_document (services/document/close.rs)
├── session::claim()                    — invalidate every in-flight open tail
├── diagnostics: dispose_begin          — Phase 0 instrumentation
├── flush_read_point                    — persist resume point synchronously
├── spawn_local:
│   ├── engine::destroy().await         — engine session teardown (below)
│   ├── engine::sweep()                 — advisory pdf.cleanup
│   ├── engine::sweep_snapshots()       — zoom-mask release
│   └── diagnostics: dispose_complete   — the moment the baseline asserts on
├── document.reset() / viewer.reset_position() / search.reset()
├── gloss.reset() / ai_selection.reset()
├── ui.sidebar = None
└── backdrop::document_close()

engine destroy (public/pdfEngine.ts)
├── session.sweepPdf()                  — pdf.cleanup while the doc is alive
├── cancel + release every page surface (render/text tasks, canvases, masks)
├── cancel thumb tasks, reset thumb lane, release thumb rasters
├── AWAIT destroyTask(loadingTask)      — the worker's actual shutdown, so
│                                         dispose_complete means "dead", not
│                                         "death scheduled" (idempotent)
└── finally: pdf/numPages/path nulled, paper republished, scratch drained
```

## What Phase 0 adds on top (and what it deliberately does not)

Phase 0 instruments this map — counters on the create/dispose edges, the
`window.__mareaderDiagnostics()` snapshot, and the smoke-test assertions
that the engine half drains. It does NOT change ownership: the single
`AppState`, the engine session singleton, and the retained search index are
exactly as they were, because they are the measured subject, not the fix.
The proposed Phase 1 boundary that follows from this map is in
`docs/memory-baseline.md`.

The snapshot also reads the bookkeeping this map says the owners hold, so a
dispose that left one behind is visible rather than inferred: the
virtualizers' listener bindings, `ResizeObserver` bindings and armed timers
(`virtualizerListeners` / `virtualizerObservers` / `virtualizerTimers`,
summed over the live handles — all zero once the reader is gone), the
engine's raw-retention timers and document-scoped idle sweeper
(`rawRetentionTimers` / `sweepTimerArmed`, both required 0 by the drained
gate), and the thumbnail generation map's size (`thumbGenerationSize`,
cleared at teardown — measured during long sessions so growth cannot go
unseen).

### Two teardown bugs the baseline caught (and their fixes)

The browser lane's raced closes — close in the same JS turn that observes
work in flight — found two real holes in this map, both fixed on this
branch. They are recorded because they shape Phase 1's boundary work:

1. **Prefetch born inside the destroy window.** A prefetch enqueued after
   the thumbnail lane's epoch bump but before the document nulls captures
   the NEW epoch, passes every epoch check, and awaits a pdf.js task on a
   worker whose death is already underway — a promise that never settles,
   so its active-prefetch slot never drains and the disposal baseline never
   arrives. The liveness of the document is now waited-on state on the
   session (`noteDocumentGone` fires the moment a destroy BEGINS; the
   prefetch's `getPage`/render awaits race it), so an await born after that
   moment wakes immediately instead of hanging.

2. **Overlay scrollbar listener outlived its owner.** The scrollbar parked
   its scroll `Closure` in a StoredValue and never removed the listener:
   reader dispose dropped the closure while `#page-list` stayed attached,
   and a zoom-settle scroll echo landing in that window dispatched into the
   dropped closure and trapped the wasm. The cleanup now removes the
   listener with the same closure identity (against the captured element,
   not a re-lookup) before dropping it. A repo-wide audit of
   `add_event_listener_with_callback` sites confirmed every other
   registration already paired with a removal.

Four more holes closed the same way — each found by asking what survives
the dispose rather than what it measures:

3. **The warmup timer had no owner.** The open flow's +1500ms warm-up timer
   captured only the page count, so open A → close A → open B within the
   delay meant A's timer prefetched A's pages into B's cache. The engine
   epoch cannot catch this — the fire is a brand-new prefetch of the
   current epoch — so the timer now carries the open flow's session stamp
   and re-verifies it at fire time and per page.
4. **A queued prefetch's cancellation waiters leaked.** Every prefetch
   raced its awaits against the epoch and the document-gone signal, but a
   prefetch that settled NORMALLY never unsubscribed: each successful
   prefetch parked two resolvers in two arrays for the document's whole
   life. The signals now return a subscription with an unsubscribe, and
   every await's `finally` removes its waiters.
5. **The lanes could be non-empty while reading "drained".** Queued page
   and thumbnail closures (with the caller resolvers they capture) sat in
   the module queues across a dispose until the FIFO reached them. Teardown
   now pumps both queues — every job's dead/epoch guard resolves it as a
   drop — and `stats()` exposes all four lane gauges, which the baseline
   requires at zero. Same family: cancelling a queued render's rAF orphaned
   the caller's promise entirely (an await that never settles, for a job
   the counters never saw), so teardown now lets the rAF fire into its
   dead-state guard.
6. **Document-scoped timers crossed documents.** The 30s idle sweeper
   survived `destroy()` and would later run `sweepPdf` over whatever
   document was open by then; the search index build is now gauged
   (`searchActive`) so a close landing mid-build is visible and the
   baseline requires the gauge empty. `destroy()` clears the idle timer.

### Notes the next phases must not lose

- `window.__mareaderDiagnostics` captures the app state through a
  window-owned closure. Fine while there is exactly one runtime; when
  Phase 2 splits library and reader runtimes, this surface needs explicit
  installation/removal tied to the runtime that owns it, or the shell
  window itself becomes the thing pinning a "disposable" reader open.
- `readerRuntimesCreated`/`readerDisposesCompleted` count document CLAIMS
  (open attempts), not reader-runtime instances — the runtime object does
  not exist yet. The names describe the shape Phase 1 wants to measure;
  until then they are claim counters and must not be read as an ownership
  boundary that already exists.

## What Phase 1 adds: the reader runtime as the lifecycle owner

Phase 1 turns the route boundary into the runtime boundary (`src/runtime.rs`,
`crate::runtime::ReaderRuntime`):

- **Lifecycle states** (`RuntimeLifecycle`): `New → Mounting → Ready →
  Disposing → Disposed`, one state machine (`RuntimeCore`) with the
  transitions and their guards unit-tested. Work-ops (opens, renders, page
  registrations, document close) are refused from `Disposing` on; teardown
  ops (destroy, sweeps) stay admitted until `Disposed`. A disposed runtime
  is never revived: the next `/reader` entry runs `begin_mount`, which
  starts a NEW generation — and a mount landing while a disposal tail is
  still awaiting the engine takes the stale resources away from that tail
  and orphans its completion via a generation guard (the fast close →
  reopen path, exercised by the same-page workload).
- **Route = runtime boundary**: `/reader` mounts the runtime
  (`begin_mount` → effects → `mark_ready`); leaving it runs
  `ReaderRuntime::dispose`. The close button is `document_close`: the
  document session tears down and the reader returns to the shelf, but the
  runtime stays `Ready`. The two operations are distinct by design.
- **Disposal order** (adapted to the dependency graph Phase 0 mapped):
  enter `Disposing` (work refused) → claim the session stamp and flush the
  read point if a document is open → reset the reader slices (the route
  flip follows the status immediately) → tail: close the document session
  (destroy awaited, sweeps) → dispose every registered virtualizer → mark
  `Disposed` (generation-guarded) → report disposal completion. The engine
  destroy is the long pole; everything after it is local and synchronous.
- **Resources owned by the runtime** (`ReaderResources`): the virtualizer
  registry — strips register where `use_virtualizer` returns and the
  runtime's dispose disposes them explicitly; component cleanup stays as
  the inner safety net. Engine document-level operations route through
  `ReaderRuntime::pdf()` (`PdfSessionHandle`), whose guards snapshot the
  lifecycle at capture: work ops no-op from `Disposing`, teardown ops until
  `Disposed`. The three reader-only event arms (link navigation, page
  selection, selection tracking) left the app-root bootstrap and are
  installed inside the runtime's scope.
- **Observable disposal** (§12): the runtime publishes every transition to
  the diagnostics surface (`runtime` in every snapshot: state, generation,
  activeDocument, activeRenderTasks, activePrefetch, registeredPages,
  virtualizerCount, listenerCount, timerCount, workerCount). The browser
  baseline asserts `runtime.state == "ready"` while reading and
  `== "disposed"` with an advancing generation after every close, plus one
  NEW runtime generation per same-page open.

Deliberately unchanged: the disposal-completion assertion and the epoch
stamping (`services::document::session`) are reused, not replaced; the
Phase 0 drain gates, counters and fail-closed accounting all still gate
every close; look-ahead, virtualization and retention behavior are
untouched. `AppState.reader` remains the signals bag (domain models stay in
`ReaderState`); the shell's `AppState.runtime` handle is coordination-only.
