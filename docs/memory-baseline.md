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
(`pages`, `thumbs`, `thumbTasks`, `activeRenders`, `hasDocument`,
`hasLoadingTask`) and the monotonic lifecycle counters with pairing rules:

```text
sessionsOpened == sessionsDestroyed
workersCreated == workersTerminated
rendersStarted == rendersCompleted + rendersCancelled + rendersFailed
```

The narration (`pdf_session:*`, `pdf_worker:*`, `render:*` events) is
opt-in and silent in normal operation; the counters are always on.

### The dev probe

In the app webview console:

```js
__mareaderDiagnostics()   // prints + returns the full JSON snapshot,
                          // and turns lifecycle narration on
```

Off the webview (host tests) the surface is inert; the counters and the
`at_baseline()` check are still unit-tested on the host.

### CI enforcement

The engine smoke suite (`tools/engine-smoke/teardown.ts`, run by the web CI
lane against the bundle built from current source) asserts the baseline:
after `destroy()`, after a rapid reopen+close, and after a no-op destroy,
every live gauge must be empty and every counter pair balanced. This is the
"open -> use -> dispose -> reopen" harness, executed on every PR.

## The manual benchmark procedure

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

Recorded per workload; fill each table from the snapshot JSON (one row per
cycle) when running against a given build. The structural findings below
are already established by the ownership map and hold for any build until
the later phases change them.

### Expected post-close baseline (every workload)

```text
reader_runtime_live = false        panes_live = 0
virtualizer_live = 0               retained_virtual_items = 0
engine.hasDocument = false         engine.hasLoadingTask = false
engine.pages = 0                   engine.activeRenders = 0
engine.thumbs = 0                  engine.thumbTasks = 0
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
