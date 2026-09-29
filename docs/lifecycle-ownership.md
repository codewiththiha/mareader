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

(That tree is the Phase 0 app; `ReaderPage` has since been replaced by the
reader host and its panes — see the Phase 3 section at the end.)

Route changes mount and unmount component trees, but the STATE and the
engine session are app-lifetime singletons — that is exactly what the later
phases replace. The inventory below is what exists now.

## Resource owners, traced

| Resource | Owner today | Created | Released |
| --- | --- | --- | --- |
| Document session (pdf.js proxy, loading task, page surfaces, thumb cache, search highlight state) | the pane's `PdfSession` (`crates/pdf-engine/src/session/mod.rs`) → one `EngineSession` per sid in the engine's registry (`public/engine/state.ts`); a fresh session per opened document | `PdfSession::create` in the open flow, then `open(sid, path)` | `destroySession(sid)` — the pane's dispose (awaited) or the pane's next open replacing it, whatever the next document's format (`PaneHandle::replace_document`: disposed at the replace, its release awaited before the new document loads, so a pane never holds two documents; another pane's is never awaited); see `docs/session-ownership.md` |
| pdf.js worker | Created inside `getDocument` per `LoadingTask`; the task is the handle | `openTask()` / `coverDataUrl()` own-task | `destroyTask()` — the single choke point for `task.destroy()` |
| pdf.js module | `globalThis.pdfjsLib`, loaded by `ensurePdfjs()` in `public/engine/loader.ts` on the first PDF open (the Shell's bake page loads it with a script tag) | first `getDocument` | with the frame: a warm reader idle behind the shelf is evicted (`WARM_READER_IDLE_MS`), and the module goes with its realm |
| Page render tasks | `PageState.renderTask` on the owning engine session, one bounded lane per session (`PAGE_RENDER_LIMIT = 2`) | `renderPageInternal` | cancel-on-supersede, `unregisterPage`, `destroySession` |
| Page hosts (canvas + host registration) | the session's `stateByCanvasId`, keyed by canvas id | `registerPage(sid, …)` through the canvas's `MountedPdf` (`crates/reader-runtime/src/components/formats/pdf/canvas.rs`) | `unregisterPage` (component `on_cleanup`), `destroySession` |
| Thumbnails | the session's `thumbCache` (LRU ≤ 16 pairs), `thumbTasks`, thumb lane | `renderThumb`/`prefetchThumb` | LRU eviction, `destroySession` |
| Rust search index | the `PdfSession`'s search scope (`crates/pdf-engine/src/session/search.rs`); at the session's dispose it moves to the one-entry `RETAINED` cache, keyed by content fingerprint + page count | first search of a document | a session opening the SAME content adopts it; a different document replaces it; `drop_retained_search()` when a reflowable document opens; with the frame when an idle warm reader is evicted |
| Reflow spot memo | the PANE's `GlossState.spots` (`crates/reader-runtime/src/state/gloss.rs` `SpotMemo`, capped), read through `reflow_anchor::parse_spot` | first mark resolve | `GlossState::reset` per document; dropped with the pane's reactive owner at its dispose — no thread-local, so a recycled frame carries nothing over |
| Look-ahead (paper colour) | the `PdfSession`'s paper state machine (`crates/pdf-engine/src/backdrop/mod.rs`: `sampling` set + per-area palettes); tasks via `spawn_engine` hold their session | `paper_document_open` / scroll ticks | the session's dispose invalidates it; the session epoch + liveness drop in-flight samples; the realm in-flight count is `backdrop::pending_samples()` (snapshot's `lookaheadSamplesActive`) |
| Thumbnail prefetch/warmup | `crates/reader-runtime/src/services/document/open/warmup.rs` fires a bounded timer bound to the pane's session and generation; each prefetch queues in THAT session's bounded thumbnail lane | after open settles | the session's teardown cancels in-flight prefetch tasks and drops queued/awaited ones — a disposed session can never file into the next; lifecycle visible in `stats()` |
| Virtualizers (page strips, stream, thumbs grid) | `virtual_list_leptos::Virtualizer` handles held by components; bindings (listeners, ResizeObserver, timers) inside `VirtualizerInner` | `use_virtualizer` | `dispose()` via the hook's `on_cleanup` |
| Virtualizer measurement store | the PANE's `DocumentState.content.metrics.css_heights` / `intrinsic` | open seeds | rewritten by the next document's seed within the pane; released with the pane's reactive owner at its dispose |
| Reader reactive state | the PANE's `ReaderState` (`crates/reader-runtime/src/state/*`), every signal an arena node of the pane's owner | the pane's create | the pane's dispose (`owner.cleanup()`); within a live pane a new document re-seeds it (`open/enter.rs`: identity, gloss, search, position) and the replaced session releases its own content |
| Gloss marks (in-memory) | the PANE's `GlossState.marks`; the durable copy per row id is written by the Shell (`ShellApi::save_gloss` → `storage::persist_encoded_gloss`) | open loads (a read) | `GlossState::reset` at the next open in the pane; with the pane's owner at its dispose (disk copy persists by design) |
| Covers | `AppState.library.covers` + `services/library/covers.rs` cache (quota-capped) | import / open tail | persists across sessions by design (library state) |
| Backdrop publication | `--pdf-paper` custom property on `<html>`, written only by the PRESENTING session (latest opened, or `presentSession` — a pane going Ready presents) | the session's paper state machine | a destroyed publisher clears it |
| Theme bake worker | `public/engine/theme/bake.ts` — ONE stateless worker per realm (every buffer is transferred in and back out; it holds no document data), created lazily by the first bake | first worker bake | `releaseBakeWorker()` when the realm's last engine session is retired (none live or draining) and no bake is in flight; the next bake creates a fresh one |
| App overlays, toasts, sidebar | `AppState.ui` | bootstrap | app lifetime (correct — shell chrome) |

## Global / static / module-level owners (the retention inventory)

These outlive every component and are the reasons a route change alone can
never release reader resources:

1. `public/engine/state.ts` session REGISTRY — one `EngineSession` per live
   sid, each owned by a pane's `PdfSession` and destroyed with it; the
   registry itself holds no document once every session is retired.
2. `crates/reader-runtime/src/services/document/session.rs` — the realm's
   generation MINT only (ids never reused; also the diagnostics disposal
   epoch). Ownership stamps are per pane (`PaneHandle::claim_generation`).
3. `crates/pdf-engine/src/session/search.rs` `RETAINED` — one retained
   search index, BY DESIGN (a reopen of the same bytes adopts it).
4. `crates/pdf-engine/src/backdrop/mod.rs` `SAMPLES_IN_FLIGHT` — a gauge;
   the paper state machine itself is per session.
5. `src/effects/app/library.rs`, `src/effects/appearance/mod.rs`
   thread-locals — app-lifetime effect bookkeeping (debounce cells). The
   reflow measurement queue is the pane's own now (`ReaderState.measure`,
   `crates/reader-runtime/src/effects/reader/reflow_measure.rs`), and the
   key-hold engine (`crates/reader-runtime/src/effects/reader/shortcuts/navigation.rs`)
   is window-level by design — one keyboard — capturing its target strip
   from the ACTIVE pane's root per hold and ending on that pane's blur or
   teardown.
6. `src/services/library/covers.rs` and `import/claim.rs` thread-locals —
   import/cover queues (library-side by design).
7. `crates/reader-runtime/src/components/viewer/shells/scroll_shell.rs`
   thread-local — a constant listener-options object (stateless). The
   reflow spot memo that used to sit beside it is per pane now (row above).
8. `crates/app-chrome/src/floating/dismiss.rs` — topmost-overlay registry
   (shell scope).
9. `public/pdfEngine.ts` module state: the `pagehide`/`visibilitychange`
   listeners (they walk every live session) and the `watchPaperTokens`
   mutation observer — installed once at bundle evaluation, never removed.
   The theme chain is per session now.
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

## The disposal sequence as it existed at Phase 0

Kept as the Phase 0 record. The current sequence is the pane's dispose
("Disposal" below) ending its `PdfSession` — see `docs/session-ownership.md`.

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
  `ReaderRuntime::dispose`. The close button is a Shell command: the
  reading position is flushed across the boundary and the reader returns to
  the shelf, and the route flip disposes the runtime as a unit. There is no
  second teardown path — a runtime that stayed `Ready` behind a closed
  document would be exactly the hidden retention §12 rules out.
- **Disposal order** (adapted to the dependency graph Phase 0 mapped):
  enter `Disposing` (work refused) → claim the session stamp and flush the
  read point if a document is open → take the registered resources out of
  the registry → tail: close the document session (destroy awaited, sweeps)
  → dispose every registered virtualizer → mark `Disposed`
  (generation-guarded) → report disposal completion from outside the
  disposed arena. The reader slices are not reset here: the whole state is
  dropped with the route, and a reset would be a second teardown path. The
  engine destroy is the long pole; everything after it is local and
  synchronous.
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

## What Phase 3 adds: the reader host and its panes

The production path is now `/reader → ReaderRuntime → ReaderHost →
PaneManager → document pane` (`crates/reader-runtime/src/lib.rs::start_session`
composes it). The old `ReaderPage` monolith is gone; each of its
responsibilities moved to exactly one owner:

| Responsibility (was `ReaderPage`) | Owner now |
| --- | --- |
| Title bar composition, left cluster (sidebar toggle, Library) | host (`host/view.rs`) |
| `ShellController` (rail open/close machine, layout truth) | host |
| Settings-open signal and the modal's placement | host (content: active pane's `Settings` slot) |
| Rail mount points (`PushRail`/`OverlayRail`) | host (content: active pane's `Rail` slot) |
| `.reader-bg` backdrop + blend class, `main#viewer-slot` | host |
| Appearance menu; motion switches resolved from settings | host (appearance boundary pushes `PaneAppearance`) |
| Doc-status and page reports to the Shell | host (from the ACTIVE pane's `PaneSurface`) |
| Library button's leave: read point + render cancel | pane (`PaneCommand::PrepareLeave`), then the host's navigate |
| AI chunk bridge (window Tauri listener) | session (composition root) |
| Frame theme / settings persistence, appearance raster hooks | session (composition root) → Shell persists |
| Gloss marks' durable copy | pane edits the list → Shell writes it (`ShellApi::save_gloss`) |
| Reload Window (reader menu) | pane flushes its read point → Shell reloads (`ShellApi::reload`) |
| Reader state (`ReaderState`), `ReaderContext` | pane (one per pane, never global) |
| Document open / session / close, paper settings, prefetch gate | pane |
| Virtualizers, zoom controller, navigation sync, reading progress, reflow pipeline, mode change, first paint, blend geometry | pane (installed at mount, in the pane's owner) |
| Keyboard shortcuts (window listeners) | pane, gated on the host's `active` |
| Viewer, first-paint cover, floating title, page pill, bottom bar, find bar, selection pill, gloss popover | pane content |
| Document title, view menu | pane (`TitleCenter` / `TitleTrailing` slots) |

- **Identity.** `PaneId` is minted by the manager core from a monotonic
  counter, never reused, never a path, a document id or an index. The
  descriptor (`host/model.rs::PaneDescriptor`) is data only.
- **Lifecycle.** `New → Mounting → Ready → (Suspended) → Disposing →
  Disposed`, decided by the pure `PaneManagerCore` (unit-tested on the host)
  and mirrored into the pane's handle for its work gates. A disposed pane is
  a tombstone: every operation on it answers `Gone`.
- **Focus.** One authority: `PaneManager::set_active` asks the core, which
  names the pane to blur and the pane to focus; the manager blurs, publishes
  the ONE `active` signal, then focuses. Panes derive `active` from it, and
  REQUEST focus through `PaneEnv::request_focus` (a pointerdown or focusin
  on the pane's root → `PaneManager::focus_request` → `set_active`). A
  pane's blur stops its auto-scroll and ends a key hold.
- **Bounds.** The host measures `#viewer-slot` (the host's own element, the
  one document-wide id lookup left) and hands each pane its box (no split:
  every pane fills it). The manager publishes each live pane's box
  (`PaneManager::bounds_of`); the host's entry is positioned by it, and the
  pane's root (`PaneDom`, `crates/reader-runtime/src/pane/dom.rs`) is sized
  by the copy `mount`/`resize` hand it. The pane budgets its startup fit
  against that box and owns everything inside it; every pane-owned DOM
  lookup is scoped to its root (`PaneDom::by_id`/`select`/`page_list`).
- **Descriptor.** The host records a `PaneRequest` with the document, its
  format (named by the injected `PaneClassifier`, never by the host), the
  resume page and an optional initial zoom. The pane opens the launch only
  when the descriptor names a document, resumes at the descriptor's page,
  and seeds its first document at the descriptor's zoom (consumed once).
- **Suspension.** The host follows the frame's slot (`use_frame_active`):
  off screen every placed pane is `Suspended` (document kept, no new work);
  back on screen they resume. A pane that becomes ready off screen is parked
  at once; `ReaderHost::open` resumes a parked pane for the command. The
  pane mirrors the lifecycle into ITS session's thumbnail prefetch switch
  (and a pane going Ready presents its session's paper).
- **Workspace commands.** A pane's open (Cmd/Ctrl+O, the frame's resolved
  open) goes to the host through `PaneEnv::open` → `ReaderHost::open`;
  Escape closes the rail through the host's `ShellController`, never by
  writing the sidebar store.
- **Resources** (`pane/handle.rs::PaneCell`, in the host's arena so it
  outlives the pane's owner mid-dispose): the virtualizers (including the
  reflow stream's and the thumbnail rail's) and the pane's document
  session — `FormatSession::{Pdf(PdfSession), Markdown(MdSession),
  Text(TxtSession)}`, one per opened document. Render work, prefetch,
  thumbnails, page registration and look-ahead belong to the `PdfSession`,
  reached only through `pane.pdf()` / `MountedPdf` and released by its
  dispose; listeners, observers and timers live in the pane's owner.
- **Disposal** is the pane's `dispose`: read point → `end_document`
  (generation claimed, session taken out; Markdown/text disposed on the
  spot) → virtualizers taken → owner cleanup (sync; the per-pane memos go
  with it) → tail (the PDF session's teardown awaited, virtualizer
  dispose, completion). The host's workspace disposal
  runs it for every pane while the session is alive; the runtime awaits
  the tails.
- **Lifetime (what keeps memory alive).** The session's reactive owner is
  single-rooted, exactly as it was before the host existed: the unmount
  handle holds the only strong reference inside the frame (the live-session
  thread-local's copy, used to re-enter the owner for in-session commands,
  is taken out before the handle drops). Nothing in the session's arena
  holds the owner — the manager builds panes under the caller's owner and
  refuses (`PaneError::Unowned`) outside one — so the unmount alone
  releases the whole tree even if the explicit disposal never ran. The
  manager drops a pane object the moment its `dispose` returns; the tail
  captures only the session's teardown, the virtualizers, the generation
  and the pane handle, and releases the handle's arena slot when it finishes. The
  diagnostics host probe holds the manager state weakly and is settled
  into a plain final snapshot at the end of the teardown, so a recycled
  frame keeps nothing of its last host between sessions.
- **Boundary.** `tools/check-host-boundary.mjs` (CI lint lane) fails if
  anything under `host/` names the PDF engine, a format renderer, the pane
  implementation or a pane's reader state, if `ReaderPage` reappears, or
  if reader code outside `context.rs` (the Shell-less `StandaloneApi`)
  names a `storage::` function that is not on its read allowlist or reloads
  the window itself — durable writes and the window are the Shell's.

Session-scoped since Phase 4 (`docs/session-ownership.md`): the PDF
engine's document state, lanes, caches, page registry, raster theme, paper
state machine and search scope are each owned by one pane's `PdfSession`,
Markdown/TXT documents by an `MdSession`/`TxtSession`, and async stamps by
the pane's generation. Realm-wide by design: id mints, the appearance
broadcast, the retained search index (content-keyed), code and allocation
caches, and the diagnostics totals (renders, prefetches, look-ahead samples
— summed across sessions in the diagnostics `engine` block, per session in
`sessionStats`). Page elements are pinned per session (Phase 5), so
several panes share the realm. Session-level by
design, not pane state: the frame's port and parked opens (`frame.rs`), the
live-session record (`lib.rs`), the diagnostics probes, and the host's
`#viewer-slot` measurement.
