# Branch state — `split-wasm-modules-t10`

Single source of truth for what this branch is and where it stands. Read this
before planning or editing; when it disagrees with memory, this file wins.
How the branch got here — the three route-split designs, what each cost and
why only the third holds — is `docs/route-split-retrospective.md`.

## What this branch is

Team 10's migration branch for the runtime split mandated by `AGENTS.md` and
`wasm-runtime-migration.md`. Base: `main`. Commits follow the conventional
format in `AGENTS.md` (subject ≤ 72 chars); author is the team identity.

## Migration status

| Phase | State |
| --- | --- |
| 0 — memory/lifecycle baseline | **done**: browser lifecycle suite (`tests/browser/lifecycle.mjs`), counters in `crates/reader-runtime/src/diagnostics.rs`, results in `docs/memory-baseline.md` |
| 1 — explicit runtime/session lifecycle | **done**: `ReaderRuntime` state machine, generations, resource registry, observable disposal (`crates/reader-runtime/src/runtime.rs`) |
| 2 — Shell / Library / Reader split | **done**: three WASM artifacts, shell-owned iframes with a `MessageChannel` handshake, `frame-transport` crate, dispose-as-a-unit |
| 3 — Reader Host & panes | **done**: `ReaderHost` + `PaneManager` own the workspace (`crates/reader-runtime/src/host/`), the document pane owns one session (`crates/reader-runtime/src/pane/`); `ReaderPage` removed. Map in `docs/lifecycle-ownership.md` (Phase 3 section); what is still a bridge: [Phase 3 bridges](#phase-3-bridges-what-phase-45-inherit) |
| 4+ — session-scoped engines, split mode, … | **not started** — waiting on the phase guide |

## Architecture as built (do not re-derive)

- Shell (`src/`) owns routing, the runtime manager, diagnostics, persistence.
- Reader and Library boot inside shell-owned iframes (`src/app/frame.rs`),
  each with its own document, JS realm and WASM instance; the Shell never
  imports their state, and disposal removes the frame as a unit.
- Frame roots must carry `h-full w-full`: a mount with `height: auto` gives
  the virtualizer an indefinite viewport and every page mounts at once
  (the "peak N page hosts" browser failure).
- The PDF engine is still a module-global under `PdfSessionHandle`; true
  session-scoped engines are Phase 4, not a defect to "fix" opportunistically.
  Until then production runs exactly ONE pane per reader session, even
  though the pane manager's API never assumes one.
- Inside the reader frame: `start_session` (composition root) → runtime →
  `ReaderHost` (chrome placement, `ShellController`, settings modal
  placement, focus/active pane, bounds, status reports) → `PaneManager`
  (ids, lifecycle, create/focus/resize/close/dispose_all) → the document
  pane (its own `ReaderState`, effects, virtualizers, document session).
  The host never names a PDF type or a pane's state
  (`tools/check-host-boundary.mjs`, CI lint lane); pane content reaches the
  chrome only through `ChromeSlot` views. Panes never see `AppState` or
  Shell state; the Shell reads the host through the `host` block of the
  diagnostics digest.
- The Shell loads **no engine**: `index.html` carries no pdf.js / engine /
  reader-bundle scripts and the root crate has no `pdf-engine` dependency
  (`tools/check-dependency-gate.mjs` forbids it, and the whole reader-only
  set — `reader-runtime`, `reflow-core`, `md-core`, `txt-core`,
  `virtual-list*`, `leptos-md` — for both the Shell and `library-runtime`).
  Shelf cover bakes never touch a reader: library `BakeCover` → the Shell's
  own bake page (`src/app/bake.rs` mounts a hidden `public/bake.html`, whose
  script `public/coverBake.ts` is pdf.js plus the engine's cover render — no
  wasm, no runtime) → `window.postMessage` ask/answer → the asking shelf's
  `CoverBaked`. One bake in flight; the page is removed 5 s after its queue
  drains, so at rest nothing but the shelf is resident. A standalone
  `library.html` boot has no Shell and never drains its queue (the shelf
  shows placeholder art).
- `*_bg.wasm` is wasm-bindgen's file naming (`<name>.js` glue +
  `<name>_bg.wasm` module), not an extra module: there are exactly three —
  shell, library, reader.

## Warm slot (why a route switch is no longer a boot)

`src/app/manager.rs::run_start` used to call `dispose_active().await` BEFORE
the replacement existed, and `start_serialized` blocked concurrent starts on
top of that: every transition was destroy-A → create-B → boot-B, so the user
paid a full artifact boot (fetch JS, fetch and instantiate WASM, mount) on
every click.

The manager now owns three slot states — `Active`, `Warm`, `Retiring`
(`src/app/frame.rs::FrameSlot`) — and keeps at most one `Warm` frame behind
whatever is on screen:

- The two runtimes warm asymmetrically. 700ms after the READER goes active
  (`WARM_DELAY_MS`), the Shell boots the shelf into a hidden slot. The
  reader is booted behind the shelf only on the shelf's **intent signal** —
  `RuntimeFrame::ExpectReader`, sent (throttled to one a second) when the
  pointer is over or moving across `#library-level`, or a card is pressed
  or focused — never on the shelf's paint. A boot stops at `Ready`: **a warm
  boot never opens a document**, so the PDF machinery is never paid for
  twice and never paid for a book the user did not ask for.
- A warm reader is **evicted when the shelf goes quiet**: each intent signal
  restarts a `WARM_READER_IDLE_MS` (60 s) clock; when it runs out with the
  shelf still on screen, the reader is disposed (the full §12 exchange,
  counted in `readerDisposesCompleted`) and its frame removed
  (`evict_idle_warm_reader`). This is the one path that removes a reader
  frame while the user stays on the shelf, and it is what makes the library
  route's memory the library's alone: `readerFramesResident` (the probe's
  count of reader frames in the DOM, any slot) is `0` at rest. The suite
  shortens the window with `?warmIdleMs=` (read once, at manager
  construction, off the boot URL).
- A navigation for a kind whose warm frame is ready is a **promotion, not a
  boot**: same element, same document, same realm, same WASM instance. The
  reveal is the frame's `data-mareader-slot` flipping to `active` (CSS
  `z-index`), which is why the frame painted while it waited — there is no
  cover to hold and nothing to await.
- A click that beats the warm boot waits out its REMAINDER
  (`wait_verdict()`), never a second boot of the same artifact.
- The runtime it displaced goes `Retiring` in the same synchronous block as
  the reveal and is **recycled** behind it (`retire_or_recycle`): it claims
  the warm lane, stays intact for `RECYCLE_DELAY_MS` (1.2s — a straight
  return takes that very session back), then its session is disposed (the
  full §12 exchange, `DisposeComplete`, counted) and `ShellFrame::Rearm`
  mounts a fresh warm session in the SAME document. No page load, no wasm
  fetch/compile, no pdf.js load after the first two boots.
- A reader frame whose last digest reports `heapHighWaterBytes` above
  `READER_RECYCLE_HEAP_MAX` (320 MiB) is retired and removed instead: linear
  memory never shrinks, so dropping the frame is the only way to return it.
  The HIGH-WATER mark, not `wasmHeapBytes`: the live heap is low again after
  every close and would keep every frame.
- Module-level state that outlives a session in a recycled frame must be
  reset or released per session (below). The reflow spot memo and the
  reflow measurement queue are no longer module-level at all: they are the
  pane's state (`GlossState.spots`, `ReaderState.measure`) and die with its
  owner. The search index deliberately survives a close (a bounded cache
  of the last book's text, cheaper than re-extraction); the eviction bounds
  its life instead.
- Hidden frames tell their documents so: `app_ui::frame_theme::
  mark_frame_hidden` puts `html.frame-hidden` on a warm or rearmed runtime
  document (cleared by `Launch` / `Refresh`), and `styles/noise.css` pauses
  the animated grain under it — a hidden frame is `visibility: hidden`, which
  stops paint but not animation.
- pdf.js is loaded on the first PDF open (`public/engine/loader.ts::
  ensurePdfjs`, a dynamic `import()`), not by a `reader.html` script tag: a
  warm reader, or a Markdown/text session, never fetches or holds it.
- Module-level state that outlives a session in a recycled frame must be
  reset or released per session: the shelf's cover ledger
  (`covers::reset_ledger`), and every Tauri listener (`tauri_listen` now
  unlistens on owner cleanup — Tauri's registry lives in the host window).
- A warm frame that died on the way up (`ready_outcome()` is an error or a
  timeout) is torn down and the transition falls back to `cold_start` — the
  warm slot must never cost the user the runtime.
- The library's heavy startup passes (migrate, measure, cover backfill,
  rescan) park in `DEFERRED` during a warm boot and run just after the
  reveal paints (`after_reveal`, 160ms), so the shelf the user left is
  refreshed rather than replayed from boot time. `Refresh` re-reads only the
  stores whose stamp moved (`storage::{library,covers,settings}_stamp`):
  the cover map is megabytes of data URLs and re-setting it re-rendered
  every cover at the moment of the reveal.
- The Idle-bounce guard is two facts: `reader_launched` (a launch was sent)
  and `reader_armed` (that session then reported Opening/Ready). A promoted
  warm reader's boot-time `Idle` can land after the promotion and must not
  send the user back to the shelf.

## Invariants (enforced by tests — never weaken them)

- Shell diagnostic counters are authoritative; runtime digests merge in only
  keys the shell does not already own (`src/diagnostics.rs`, unit-tested).
- Per kind, `created - completed == (active is X) + (warm-ready is X)`. The
  old `created == completed` form is only true when the warm slot is EMPTY,
  and it is never empty while the shell is warm — a warm runtime counts as
  created the moment it answers `Ready`.
- Leaving the reader cancels in-flight page renders synchronously with the
  click (the active pane's `PaneCommand::PrepareLeave` →
  `cancel_page_renders`) before the navigate
  command crosses the frame channel; the session destroy during disposal
  remains the single teardown path.
- Browser peaks: page hosts ≤ render window + zombie cap, active renders ≤
  page-lane slots, counters drain to zero at baseline.
- At most one frame is visible at any instant, and it is the `active` one. A
  hidden slot is `visibility: hidden` — **never `display: none`**, which
  starves the iframe of `requestAnimationFrame` and would therefore never
  produce `Painted`.
- Two frames may differ, two VISIBLE frames may not: hiding the outgoing
  frame is part of the reveal, not a follow-up task.
- Nothing is disposed while it is on screen (`run_retire` re-asserts this for
  any retirement that did not come from a reveal).
- Per kind, `created − completed == (active is X) + (warm-ready is X)`: the
  retired session's disposal still runs to completion, just not in front of
  the handoff.
- A warmed frame is the frame that gets revealed — `backSlots.active ===
  warmShelf.warm` — and the revealed generation is the warmed generation.
  Rebooting at promotion time is a failure, not an optimisation.
- Disposal epoch is frame-instance-local: a fresh frame opens at 1, a
  recycled frame opens at its last close + 1, and every close claims exactly
  one past its open. The reported runtime generation is Shell-owned — the
  reader-session count, advancing once per reader session (a rearm is a new
  session) across frames.
- The theme pipeline's `gen` (public/engine/theme/pipeline.ts) moves only
  when a bake INPUT moves (filter | blend | `--color-paper`), never on a
  root-style write alone: the engine's own `--pdf-paper*` publications
  during a zoom used to invalidate every in-flight bake and loop re-renders
  (the zoom flicker).
- The film-grain `.noise-overlay` lives in each runtime document (created by
  `install_frame_theme`), next to the body classes that drive it — never in
  the Shell, whose body classes the frames do not share.

## Untweened zoom (animations off)

- One discrete commit (`zoom/animation.rs::commit_instant`): detached
  rescale, `display = committed = to` in one batch, then the deferred scroll
  write, then `finish_transition`. No FrameLoop, no frame-count holds.
- The old bitmap stays up until the commit's render blits in
  (`renderer.ts` scratch + blit); stale completions record geometry for the
  stretch but never write the rendered-scale host or strip report.
- The one-frame misaligned landing was CSS, not scheduling: the motion nets
  give every element `transition-duration: 0.01ms`, and
  `transition-property` defaults to `all`, so wrapper `top` and host size
  painted one frame at the old value while `scrollTop` did not.
  `[data-page-strip]` removes transitions under both nets
  (styles/components/animations.css). The browser suite's zoom-off stage
  asserts no mid-scale frame, no blank frame, the final offset, and a
  landing frame identical to the settled one.
- Noise: the four-state matrix (animations × reduced motion) is asserted in
  the active and warm frames from computed `::after` state. Only reduced
  motion stops the grain; the app's animations-off switch keeps it moving.

## CI is the only build

No Rust is compiled in the dev sandbox (disk limits). Push and let GitHub
Actions judge: `CI` (format, clippy+wasm check+dependency gate, `cargo test`,
web contracts, macOS shell) on every push; `Deep CI` (browser lifecycle
baseline + Tauri boot smoke) on pushes touching app/engine paths. Watch the
run's job logs (`ci_watch.py` at the workspace root polls them), fix, squash
fixups, force-push. No lane reads `docs/**`, so a docs-only push runs neither
workflow.

## CI is skippable where it is not needed

- `CI` ignores pushes that only touch `docs/**` — no lane reads those files
  as input (the contract scripts parse SOURCE comments, never the documents
  they point at). Any push touching code runs the whole matrix.
- `Deep CI`'s two 45-minute lanes honour `[skip deep]` in the commit subject;
  a `workflow_dispatch` can narrow the run to one lane or override the
  marker. The nightly cron ignores it, so a skip is never the last word.

## Measured, not assumed

The warm slot is proven by the browser lifecycle baseline, not by reasoning:
`rapidTransitions` reports `reusedWarmFrame: true` for all four back-to-back
handoffs, the host sampler's `peakFrames` is 2 (one on screen, one behind
it — never a third on screen), and every memory trend is unchanged
(`slope 0 B/cycle, drift 0 B` across normal, rapid and same-page cycles).
A click that outran the warm boot boots the lane on demand instead of
falling back to a covered cold start, so the runtime the user is leaving
stays on screen for the boot either way. The eviction has its own stage
(`stage0-idle-eviction`, `?warmIdleMs=2000`): an untouched shelf boots no
reader (`frames === 1`, `readerFramesResident 0`), intent boots one, silence
evicts it (`warmReaderEvictions`, session balance, `frames === 1`,
`atBaseline`), renewed intent boots a fresh generation and the click reveals
it, and after a read-and-close the recycled reader is evicted the same way.
The cover bake is proven in the same run without any reader resident
(`coverBake.covers`, `bakeFrameResident` back to `false`).

## Phase 3 bridges (what Phase 4/5 inherit)

Phase 3 made the host the workspace owner and the pane the owner of one
document session. What is still a BRIDGE — correct for one pane per
session, and named here so the later phases replace it deliberately:

- **Scoped already (do not regress):** every pane-owned DOM lookup goes
  through the pane's root (`crates/reader-runtime/src/pane/dom.rs`,
  `ReaderState.dom`) or a `NodeRef`; the reflow measurement queue and the
  spot memo are pane state, not thread-locals; focus is REQUESTED by the
  pane (`PaneEnv::request_focus`) and decided by the manager; bounds are
  published per pane (`PaneManager::bounds_of`) and position the host's
  entry and size the pane's root; the host suspends panes while the frame
  is off screen and resumes them on reveal; Cmd/Ctrl+O and the frame's
  resolved open reach the host (`PaneEnv::open` → `ReaderHost::open`);
  Escape closes the rail through the host's `ShellController`; the
  descriptor's document, page, format (via the injected `PaneClassifier`)
  and zoom are honoured by the pane; a pane's gloss marks are written by
  the Shell (`ShellApi::save_gloss`, the list crossing as JSON because
  `runtime-contract` may not depend on `ai-core`) and Reload Window asks
  the Shell (`ShellApi::reload`) instead of reloading the frame's own
  document. `tools/check-host-boundary.mjs` fails on any other store write
  or window reload in reader code outside `context.rs`'s `StandaloneApi`.
- **Phase 4 (session-scoped engines):** the JS PDF engine session is one per
  realm (`public/engine/state.ts`), so the prefetch switch the pane's
  lifecycle drives is realm-wide, the diagnostics `engine` counters
  (renders, prefetches, look-ahead samples) are realm totals that
  `PaneResourceCounts` cannot attribute (it counts virtualizers and the
  document session only), the disposal epoch
  (`crates/reader-runtime/src/services/document/session.rs`) is one claim
  stamp per realm, and the look-ahead paper session and search index are
  realm thread-locals.
- **Phase 5 (split mode):** every pane's box is still the whole
  `#viewer-slot` (the host has one layout); the chrome slots (title,
  view menu, rail, settings) are filled by the ACTIVE pane only; the
  floating document title is portaled at window level and placed from the
  pane root's box; the key-hold engine is window-level (one keyboard) and
  captures the active pane's strip per hold; `PaneRequest.initial_zoom`
  has no host-side source yet (a duplicated or split pane will supply it);
  the focus request rides bubbling `pointerdown`/`focusin`, so a control
  that stops propagation inside a background pane will need a capture
  listener once two panes are visible.

## Known follow-ups (do not silently expand scope)

- Measured on a2aa19a (Deep CI #242): the cover bake landed from the Shell's
  bake page with no reader resident (`coverBake.covers` 1 in ~5 s,
  `sawBakeFrame` true), `reusedWarmFrame` 4/4, `peakFrames` 2, idle
  eviction in 2019 ms for a 2000 ms window with no forced removal,
  rapid-reopen and same-page slope/drift 0 B, `samePageRecycledOpens` 9/10.
  The e8b1d18 relay numbers recorded here before (`coverRelay.covers` 1 in
  1 ms) were the reader's own cover write, not a relay: the relay's ask was
  dropped before the shelf was registered, which is why the bake now waits
  for admission in `src/app/bake.rs`.
- The Shell accepts a digest from a recycled frame only while its kept
  session lives (recycle Pending/Disposing); a `BakeCover` ask is routed
  ahead of both the registry and the live gate, because a cold shelf asks
  from inside its own mount.
- Pane teardown is observable: every digest carries `host` (lifecycle,
  activePane, per-pane lifecycle/bounds/resources, panesCreated) and the
  browser suite asserts one ready, focused pane with a fresh id per open
  and an empty, disposed workspace after close (`assertHostWorkspace`,
  `assertHostDisposed` in `tests/browser/lifecycle.mjs`).
- Diagnostics hardening: a reader that existed but never reported a terminal
  digest should fail `atBaseline` closed (currently only "last digest says
  drained" is required).
- Two app-lifetime pieces have had no caller since the split (8bbda0a) and
  are kept for the day they are re-armed, not deleted: `DragOverlay`
  (`crates/app-ui/src/components/app_overlays/drag_overlay.rs`, with no
  `tauri://drag-drop` listener in any runtime — drag-and-drop opening is
  currently unwired) and `install_window_state_bridge`
  (`crates/app-ui/src/window_bridge.rs`, so the frameless caption's
  maximize/restore glyph never follows the window). Both need the Tauri
  event relay to reach the runtime frames.
