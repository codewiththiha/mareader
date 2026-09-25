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
| Look-ahead (paper colour) | `crates/pdf-engine/src/backdrop/mod.rs` thread-local `Session` (`sampling` set + per-area palettes); tasks via `spawn_engine` | `document_open` / scroll ticks | `document_close` resets state; epoch token invalidates in-flight samples |
| Thumbnail prefetch/warmup | `src/services/document/open/warmup.rs` — an UNOWNED 1.5s timer + spawn_local chain | after open settles | runs to completion (bounded: ≤16 pages); guarded only by the engine answering for whatever is open |
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
- Thumbnail warmup (`open/warmup.rs`): UNOWNED by design — a fire that must
  not reach into a possibly-disposed reader; bounded at 16 prefetches.
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
├── destroyTask(loadingTask)            — worker death (idempotent)
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
