# Session ownership (PDF / Markdown / TXT)

Who owns every piece of document-format state, how long it lives, and what
ends it. Written as the inventory **before** the per-pane session migration
and kept as the ownership record after it: a new piece of module-level state
in any of the listed areas needs a row here, and `tools/check-session-ownership.mjs`
refuses the shapes this migration removed.

The model after the migration:

```text
ReaderRuntime (one per reader realm; dispose cascades)
└── ReaderHost
    ├── Pane (PDF)  -> PaneCell.session = FormatSession::Pdf(PdfSession)
    │                    PdfSession (crates/pdf-engine/src/session/mod.rs)
    │                      ├── sid  ── window.PDFReader session sid
    │                      │           EngineSession (public/engine/state.ts)
    │                      │             document proxy + loading task (its pdf.js worker)
    │                      │             page registry, page lane, render trace entries
    │                      │             thumbnail cache + thumb lane + prefetch era
    │                      │             paper stash, raster theme (scrub, chain, snapshots)
    │                      │             search context + active match
    │                      ├── paper session (per-page palette ladders, look-ahead)
    │                      └── search scope + index
    ├── Pane (MD)   -> FormatSession::Markdown(MdSession)
    └── Pane (TXT)  -> FormatSession::Text(TxtSession)
```

A PDF document gets a **fresh** `PdfSession` (a fresh sid) on every open. The
sid is the session generation: it is minted once, never reused, and the JS
registry refuses every call naming a sid it does not hold — so work captured
against a replaced or disposed session cannot reach the next one.

**Element lookup is per session.** A page registers with its canvas and
host ELEMENTS (the Rust side resolves them inside its own pane root), so the
engine never resolves a page by a realm-wide id, and every canvas an engine
session sweeps carries `data-engine-sid`. Two panes showing the same page
number in one realm — the split workspace — never reach each other's
elements.

## Production call graph

```text
frame.rs on_init -> ReaderHost::new -> PaneManager::create -> DocumentPane::create
  ReaderState::new(handle)                      state carries its pane
  paper_settings(ctx)                           settings -> pane.pdf().paper_configure
Launch -> open_with_launch                      pane.claim_generation()
  PDF  -> open_pdf
           PdfSession::create()                 the ONLY creation site
           pane.replace_document(Pdf)           replaced session disposed HERE -> Retiring
           retiring.settled().await             THIS pane's old document released first
           owns_generation? -> pane.pdf().open(path) -> PdfSession::open -> PDFReader.open(sid, …)
           owns_generation? -> seed: configure_session, pane.pdf().paper_document_open
                              outline / cover / warm-up: pane.pdf().* + owns_generation
           failure -> pane.abandon_document()   the half-opened session goes too
  MD/TXT -> open_reflowable
           pane.replace_document(Markdown|Text) replaced session disposed HERE -> Retiring
           drop_retained_search()
           retiring.settled().await; owns_generation? -> read + parse (raw text dropped
                                                 after the parse); failure -> abandon_document()
Page canvas / thumbnail cell: MountedPdf::bind() at mount
           -> register_page / render_page / blit_thumb / render_thumb / cancel_* on THAT session
Search, sweeps, prefetch, zoom, mode flip, blend geometry: state.pane.pdf().*
Pane lifecycle: Suspended -> suspend_prefetches; Ready -> present + resume_prefetches
Pane leave:   prepare_leave -> pane.quiesce()   (the session stops its own in-flight work)
Pane dispose: end_document() = claim_generation + take_session().dispose()
           the returned Retiring awaited in the tail (settled already for MD/TXT)
           (paper invalidated, search retained, PDFReader.destroySession(sid))
ReaderRuntime dispose -> PaneManager::dispose_all -> every pane's dispose (above)
```

There is no compatibility adapter: the pre-session free functions
(`pdf_engine::api::{open, render_page, …}`, `backdrop::{document_open, …}`)
are deleted, and `tools/check-session-ownership.mjs` keeps them from
returning.

## Replacing a document: one path, per pane

Every way a document ends goes through `FormatSession::dispose(self) ->
Retiring` (`crates/reader-runtime/src/pane/session.rs`), whatever the format
or view mode:

- **at the call** the session refuses every operation: a Markdown/text
  session has already released the pane's reflow content; a PDF session has
  stopped accepting, invalidated its paper and retained its search index;
- **`Retiring`** is only what is still in flight — a PDF's engine teardown
  (document, pdf.js worker, rasters, caches). It is settled at once for a
  reflowable session and for an empty pane.

Its three callers decide when that memory must be back:
`replace_document` (the next open in the SAME pane awaits it before
loading), `abandon_document` (a failed open detaches it), and `end_document`
(the pane's dispose awaits it in its tail). Awaiting it never waits on
another pane's session, and nothing serializes panes realm-wide, so split
panes open, replace and close independently. `tools/check-session-ownership.mjs`
keeps the slot's raw moves (`install_session`, `take_session`) inside
`PaneHandle`.

## Inventory

Columns: OWNER · LIFETIME · MUTABLE? · SHARED INTENTIONALLY? · DISPOSAL ·
ASYNC WORK? · CAN TWO PANES EXIST (before) · VERDICT.

### PDF JavaScript engine (`public/pdfEngine.ts`, `public/engine/*`)

| Item | Owner | Lifetime | Mut | Shared? | Disposal | Async | Two panes before | Verdict |
|---|---|---|---|---|---|---|---|---|
| `state.ts` `session` (the one `EngineSession`: pdf proxy, loadingTask, numPages, currentPath, detectedPaper, `stateByCanvasId`, thumb cache/tasks/cancelled/live, search query + active match, renderCount, lifecycle counters, scrub flags, appearance-menu flag, idle + raw timers, document-gone waiters) | module | realm | yes | no — accident of one document | `destroy()` resets fields | yes (renders, prefetch, idle timer) | **no**: a second open destroys the first | **session-owned**: `EngineSession` per sid in a registry; the singleton export is removed |
| `EngineSession.setDetectedPaper` writing `--pdf-paper` on `documentElement` | session → root | document | yes | the root style is one host backdrop | cleared on destroy | no | two sessions would fight | **session-owned value, single publisher**: each session keeps its paper; only the publishing session (latest opened, or `presentSession`) writes the root; a destroyed publisher clears it |
| `state.ts` `lifecycleLog` | module | realm | yes | yes — a dev narration switch | none needed | no | yes | **retained** (developer switch, no document data) |
| `renderer.ts` `pageActive`, `pageQueue` (page lane) | module | realm | yes | no | `drainPageLane()` in destroy | yes | lane shared, drained by the other's destroy | **session-owned** (`EngineSession.pageLane`); starts are additionally capped realm-wide (`REALM_PAGE_LIMIT`, `realmLane`) because a raster is main-thread work wherever it runs — the queue and its drain stay per session, the cap only decides when a queued job may start |
| `state.ts` `realmLane`, `lanePumpSessions` (realm page-lane cap + pump registry) | module | realm | yes | yes — the pacing contract itself | a session's entry unregisters FIRST in its destroy; the registry holds sessions by `WeakRef`, prunes retired/collected ones on every pump, and the counter is released by each settling raster | yes | n/a (added with the cap) | **realm-shared by design**: full-page rasters are main-thread work, so their concurrency is bounded across sessions; a freed slot re-offers the lane to every registered queue. The registry can never pin a session past its teardown |
| `renderer.ts` `renderTrace`, `renderGeneration` | module | realm | yes | diagnostics ring | bounded ring (128) | no | mixed entries, no identity | **retained, bounded**; entries now carry `sid` (diagnostics only, never a staleness guard) |
| `thumbnails.ts` `thumbActive`, `thumbQueue`, `prefetchInFlight`, `thumbGeneration`, `thumbLaneOpen`, `thumbLaneEpoch`, `epochWaiters`, `prefetchEra`, `prefetchSuspended`, `eraWaiters` | module | realm | yes | no | `resetThumbLane()` / `suspendPrefetches()` | yes (queued renders, prefetch awaits) | one pane's suspend/teardown stopped the other's prefetch | **session-owned** (`EngineSession.thumbLane`) |
| `paper.ts` `stash`, `active` | module | realm | yes | no | `resetPaperForDocument()` | no | frames of two documents mixed by canvas id | **session-owned** (`EngineSession.paperStash`, `paperActive`) |
| `paper.ts` `scratch` (≤96px downscale canvas) | module | realm | yes | yes — transient allocator | overwritten each use | no | yes (synchronous use only) | **retained**: holds pixels only inside one synchronous `downscale`; the frame handed out is a copy |
| `canvas.ts` `canvasPool`, `scratch`, `scratchInUse` | module | realm | yes | yes — allocation recycler | `disposeScratch()` (idle sweep, any destroy, pagehide/hidden) | no | yes (acquire/release is synchronous around one bake) | **retained**: a buffer is only borrowed inside one bake; no canvas is ever handed to a session to keep; pooled canvases are parked at 1×1 |
| `loader.ts` `workerSrcConfigured`, `pdfjsLoading` | module | realm | yes (once) | yes — cached module code | never (code cache) | load once | yes | **retained**: executable code, not session state |
| `loader.ts` `destroyedTasks` (WeakSet) | module | realm | yes | yes — idempotence guard | weak | no | yes | **retained**: weak, holds nothing alive |
| pdf.js worker (one per `LoadingTask`) | the task that created it | document | — | no | `destroyTask` → `task.destroy()` | yes | one per open | **session-owned**: the session's `loadingTask`; `destroySession` destroys it and counts `workersTerminated` |
| cover worker (`coverDataUrl` transient task) | the call | the call | — | no | destroyed in `finally` | yes | yes | **session-scoped call**: counted on the calling session, destroyed before the call returns |
| `theme/bake.ts` `bakeWorker`, `bakeWorkerFailed`, `bakeSeq`, `pendingBakes` | module | realm | yes | **yes** — stateless pixel kernel | pending map drains per reply | yes | yes | **retained**: the worker keeps no buffer after a reply (the buffer is transferred there and back); `pendingBakes` only holds in-flight resolvers whose callers re-check their page's liveness |
| `theme/filterKernel.ts` `lutCache` | module | realm | memo | yes | bounded memo keyed by filter token | no | yes | **retained**: pure function memo of global appearance input |
| `theme/pipeline.ts` `pipelineCache`, `lastInputs` | module | realm | yes | yes — derived from global appearance CSS | `invalidatePipeline()` | no | yes | **retained**: global appearance (settings) input, no document data |
| `theme/paper.ts` `paperWatcher`, `paperWatchLast` | module | realm | yes | yes — watches root appearance tokens | never (one observer) | microtask | yes | **retained**: republishes the *publisher session's* baked paper |
| `theme/scrub.ts` `lastBakedFingerprint`, `entryPrepare`, `entrySnapshots` | module | realm | yes | no | scrub exit | yes | one pane's scrub state applied to the other's canvases | **session-owned** (`EngineSession.scrub`) |
| `pdfEngine.ts` `themeChain` | module | realm | yes | no | never | yes | one chain for all | **session-owned** (`EngineSession.themeChain`); the appearance broadcast enqueues on every live session |
| `appearance-scrubbing` class on `documentElement` | scrub.ts | scrub window | yes | global appearance UI | removed at scrub exit | no | toggled per document | **retained at realm level**, toggled once by the appearance broadcast |
| `pagehide` / `visibilitychange` listeners | module | realm | — | yes | never | no | acted on the one session | **retained**; now walk every live session |
| `reader/selection.ts` selection tracker | reader bundle | realm | yes | yes — format-agnostic window selection | listeners | no | yes (one window selection) | **retained**: there is one DOM selection per window |

### PDF Rust engine (`crates/pdf-engine`)

| Item | Owner | Lifetime | Mut | Shared? | Disposal | Async | Two panes before | Verdict |
|---|---|---|---|---|---|---|---|---|
| `bridge.rs` session-less `window.PDFReader` externs | crate | realm | — | — | — | — | every call hit the one document | **replaced**: every document extern takes `sid` first |
| `api::{open, destroy, outline, cover_data_url, register_page, unregister_page, cancel_page_renders, render_page, render_thumb, cancel_thumb, has_thumb, blit_thumb, prefetch_thumb, suspend_prefetches, resume_prefetches, build_search_index, search, set_active_match, clear_highlights, sweep, sweep_snapshots, take_paper_frame, sample_paper_page, set_paper, set_paper_active}` free fns | crate | realm | — | — | — | yes | implicit singleton target | **removed**: methods on `PdfSession` |
| `api::search` `INDEX`, `SCOPED`, `BUILT` thread-locals | module | realm | yes | no | re-scope on open | yes (index build) | one index for all | **session-owned** (`PdfSession` search scope); cross-open reuse moves to an explicit `RETAINED` cache (1 entry, keyed by content fingerprint + page count, filled at session dispose, taken at scope, dropped when a reflowable document opens) |
| `api::search` `BUILD_ACTIVE` | module | realm | counter | diagnostics | guard drop | — | summed | **retained as aggregate gauge**; each session also counts its own |
| `backdrop` `SESSION` thread-local (paper state machine: config, blend, doc path, palettes, interim, published, position, sampling, epoch) | module | realm | yes | no | `document_close()` | yes (samples) | one palette for all | **session-owned** (`PdfSession` paper session); tasks hold their session and re-check its epoch + liveness |
| `api::mod` `js_keys!` thread-locals | crate | realm | no | yes — interned strings | never | no | yes | **retained**: immutable interned keys |
| `api::diagnostics::engine_stats` | crate | realm | — | diagnostics | — | — | one session | **retained as aggregate**; `PdfSession::stats` is per session |
| `api::theme::{refresh_theme, set_scrub_mode, set_appearance_menu_open}` | crate | realm | — | global appearance | — | yes | one session | **retained as an appearance broadcast** (no document identity; each live session re-derives its own raster theme) |

### Reader runtime (`crates/reader-runtime`)

| Item | Owner | Lifetime | Mut | Shared? | Disposal | Async | Two panes before | Verdict |
|---|---|---|---|---|---|---|---|---|
| `services/document/session.rs` `SESSION` (open/close stamp) | module | realm | yes | no | never | guards every open hop | **no**: pane A's open made pane B's in-flight open stale | **pane-owned**: each `PaneCell` holds its document generation; the module keeps only the id mint (`next_generation`, never reused) and the diagnostics epoch |
| `pane/engine.rs` engine handle (lifecycle flags only, forwarded to the singleton) | pane | per use | no | — | — | — | — | **`PdfPane`: carries the pane's `PdfSession`** captured at `pane.pdf()` with the pane/runtime gates; `MountedPdf` binds a page canvas or thumbnail cell to the session it mounted for (`PaneHandle::pdf_for`: work only while the pane still holds that exact session) |
| `PaneResources.document_session: bool` | pane | pane | yes | no | pane dispose | — | yes | **replaced** by `PaneCell.session: FormatSession` (the owning object); `holds_document_session()` derives from it |
| `effects/reader/reflow_measure.rs` batch identity (the block list's `Arc` pointer) | pane | batch | yes | no | flush | debounce | per pane, but a freed pointer could be reused by the next document | **session-owned**: a batch is keyed by the `MdSession`/`TxtSession` id captured with the measurement, and the flush lands it only while `pane.admits_reflow(id)` |
| `state::ReaderState` (document signals, reflow content: blocks, headings, heights, cuts, block_page, geometry, stream, resume fraction, stream total; measure inbox) | pane (`DocumentPane::create`) | pane | yes | no | owner cleanup | measure debounce | **yes**, already per pane | **storage stays per pane**; the document lifetime over it is owned by `MdSession` / `TxtSession` (generation, dispose resets it) |
| reflow open read + parse (`open/reflow.rs`) | spawn_local | one open | — | no | stamp check | yes | stamp was realm-wide | **session-owned**: guarded by the pane generation; the `MdSession`/`TxtSession` is installed when the parse lands |
| page geometry reports (`components/formats/pdf/strip.rs`), search tails (`effects/reader/search.rs`, `floating_search.rs`) | component / spawn_local | one report / one run | — | no | epoch check | yes | the realm epoch: another pane's open stood them down | **pane-owned**: stamped with `pane.generation()`, checked with `owns_generation` |
| `effects/reader/shortcuts/navigation.rs` `HOLD_*` | module | realm | yes | yes — one keyboard | `end_key_hold()` on blur | rAF | one keyboard hold at a time | **retained**: input gesture, not document state |
| `diagnostics.rs` counters, `LIVE_VIRTUALIZERS`, `SESSION_FACTS`, `RUNTIME_VIEW`, `HOST_PROBE` | module | realm | yes | diagnostics | reset per runtime | no | yes | **retained**: diagnostics |
| `frame.rs` `API`, `SESSION_ID`, `RESOLVES`, `PENDING_OPENS`; `lib.rs` `LIVE_SESSION`, `SESSION`, `NEXT_ID`, `PENDING_DISPOSE` | module | realm | yes | the reader runtime itself | runtime dispose | yes | one runtime per realm | **retained**: runtime/transport owners (Phase 3), not document state |
| `components/ai/gloss/selection_mode.rs` `UNDO_GEN` | module | realm | counter | id mint | never | no | yes | **retained**: id generator |

### Pure crates

`pdf-core`, `pdf-paper`, `reader-core`, `md-core`, `txt-core`, `reflow-core`:
no module-level mutable state (no `static`, `thread_local!`, `OnceCell`).
They stay pure; the session boundary lives in `pdf-engine` and
`reader-runtime`.

## Async identity

A result commits only when all four still hold:

1. **runtime** — `ReaderRuntime::lifecycle().admits_work()` (captured by `pane.pdf()`),
2. **pane** — the pane lifecycle (captured by `pane.pdf()`), plus the pane's
   document generation for the open flow (`PaneHandle::owns_generation`),
3. **session** — the captured `PdfSession` is live (Rust), and the sid is
   still registered (JS refuses unknown sids with `no_session`),
4. **operation** — the existing per-operation guards, unchanged and now
   per session: the page's `queueGen`, the thumb generation per canvas, the
   thumb-lane epoch, the prefetch era, the paper session's epoch, the search
   scope key.

No counter is realm-wide any more except id mints and diagnostics.

## Mechanical check

`tools/check-session-ownership.mjs` (CI: "Session ownership") fails the build
when:

1. reader code names a `pdf_engine::api` function other than the realm-wide
   ones (appearance broadcasts, `engine_stats`, `set_lifecycle_log`, the
   error/stats types) — document work goes through `pane.pdf()`;
2. reader code names a `pdf_engine::backdrop` function other than the
   `pending_samples` gauge;
3. `PdfSession::create` appears outside `services/document/open/mod.rs`,
   `replace_document` outside the open flow, or `install_session` /
   `take_session` outside `PaneHandle`;
4. `current_epoch` appears outside diagnostics — async stamps are per pane;
5. a legacy ownership name returns (`note_document_session`,
   `session::claim`/`owns`, `scope_to_document`, `document_close`,
   `__pdfDestroy`);
6. a `window.PDFReader` extern other than the realm-wide six does not take
   `sid: u32` first;
7. `crates/pdf-engine` declares a `static` outside the classified realm-shared
   set (`NEXT_SID`, `SAMPLES_IN_FLIGHT`, `RETAINED`, `BUILD_ACTIVE`).

Test code (`#[cfg(test)]` modules and files) is exempt.
