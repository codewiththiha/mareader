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
| 4 — session-scoped PDF / Markdown / TXT engines | **done**: each pane owns one `FormatSession` per opened document — `PdfSession` (`crates/pdf-engine/src/session/`, one engine session per sid in `public/engine/state.ts`), `MdSession` / `TxtSession` (`crates/reader-runtime/src/pane/session.rs`); async stamps per pane; inventory, call graph and retained realm state in `docs/session-ownership.md`, enforced by `tools/check-session-ownership.mjs`; what Phase 5 inherits: [Phase 4 bridges](#phase-4-bridges-what-phase-5-inherits) |
| 5 — production split workspace | **done**: `/reader` runs a `PaneTree` (layout over pane ids only, `crates/reader-runtime/src/host/tree.rs`) under the `ReaderHost`; up to four live panes, each with its own `FormatSession`; `open_document(target)` places a document in a pane or beside it; host-owned dividers, focus outline and per-pane close; see [Phase 5: the split workspace](#phase-5-the-split-workspace) |
| 6 — smart document drag/drop | **implemented**: a file row of the reader's Library panel (the rail's third tab, `crates/reader-runtime/src/host/library/`) is the one split-drag source; targets come from the measured slot and pane boxes (`crates/reader-runtime/src/host/{geometry,drop_target,drag,commands}.rs`), the preview is geometry only, a drop is one `WorkspaceCommand`; OS file drops import into the library while it is on screen (`src/services/import_drop.rs`); see [Phase 6: document drag and drop](#phase-6-document-drag-and-drop) |
| 7+ — appearance blend, … | **not started** — waiting on the phase guide |

## Architecture as built (do not re-derive)

- Shell (`src/`) owns routing, the runtime manager, diagnostics, persistence.
- Reader and Library boot inside shell-owned iframes (`src/app/frame.rs`),
  each with its own document, JS realm and WASM instance; the Shell never
  imports their state, and disposal removes the frame as a unit.
- Frame roots must carry `h-full w-full`: a mount with `height: auto` gives
  the virtualizer an indefinite viewport and every page mounts at once
  (the "peak N page hosts" browser failure).
- The PDF engine is session-scoped: every document call names a session
  (`sid`), each pane's `PdfSession` owns one, and reader code reaches it only
  through `pane.pdf()` / `MountedPdf` (`crates/reader-runtime/src/pane/engine.rs`).
  Several document panes share one reader realm: a page's canvas and host
  are handed to the engine as ELEMENTS when the page registers (pinned to
  its session), and every canvas the engine sweeps carries
  `data-engine-sid`, so page-numbered ids in two panes never cross.
- Inside the reader frame: `start_session` (composition root) → runtime →
  `ReaderHost` (chrome placement, `ShellController`, settings modal
  placement, focus/active pane, bounds, status reports, the workspace's
  dividers / focus outline / pane close) → `PaneTree` (the split layout:
  splits, ratios and pane ids — never a document) → `PaneManager`
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
- A reader whose workspace held a split (the digest's `host.panesCreated`
  above `READER_RECYCLE_PANES_MAX`, 1) is retired and removed on the way back
  to the library, never recycled. The Rust heap mark above stays at a couple
  of MiB whatever is read; what documents grow is beside it (pdf.js and its
  worker, the engine's arenas, uncompacted canvases), once per pane, and a
  recycled realm kept it for as long as shelf intent kept it warm. The
  removal runs behind the library's reveal; the next open reveals a fresh
  warm reader booted on intent. A single-pane session is still recycled.
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
  Escape closes the rail through the host's `ShellController` (unless a
  modal or dismissable surface claims that press:
  `app_chrome::floating::dismiss::escape_is_claimed`); the
  descriptor's document, page, format (via the injected `PaneClassifier`)
  and zoom are honoured by the pane; a pane's gloss marks are written by
  the Shell (`ShellApi::save_gloss`, the list crossing as JSON because
  `runtime-contract` may not depend on `ai-core`) and Reload Window asks
  the Shell (`ShellApi::reload`) instead of reloading the frame's own
  document. `tools/check-host-boundary.mjs` fails on any other store write
  or window reload in reader code outside `context.rs`'s `StandaloneApi`.
- **Phase 4 (session-scoped engines): resolved.** The engine session, its
  prefetch switch, lanes, caches, page registry, raster theme, paper state
  machine and search scope are per `PdfSession`; the ownership stamp is the
  pane's generation. See [Phase 4 bridges](#phase-4-bridges-what-phase-5-inherits).
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

## Phase 4 bridges (what Phase 5 inherits)

Phase 4 made every document session-owned. What remains realm-wide — each
safe with one document pane per realm, and listed so split mode replaces it
deliberately rather than discovering it:

- **Element ids.** The engine resolves a page's canvas and host by id
  (`getElementById`), and the Rust page/thumbnail ids are page-numbered, not
  pane-scoped. Two panes in one realm need pane-unique ids first.
- **Root backdrop.** `--pdf-paper` on `<html>` has one publisher: the
  presenting session (latest opened, or `presentSession` — a pane going
  Ready presents). Split mode decides whose paper the shared backdrop shows,
  or gives each pane its own backdrop.
- **Diagnostics totals.** The snapshot's `engine` block sums every session
  (`sessionStats(sid)` has the per-session numbers); `PaneResourceCounts`
  still reports virtualizers and whether a document session is held.
- **Realm-shared by design** (not bridges): id mints (`NEXT_SID`, the pane
  generation mint, reflow session ids), the appearance broadcast and its
  scrub window, the content-keyed retained search index (`RETAINED`), the
  pdf.js module, canvas pool, LUT/pipeline caches and bake worker, and the
  diagnostics gauges. The reasons are in `docs/session-ownership.md`. The
  canvas pool drains and the bake worker terminates once the realm's last
  engine session is retired, so a reader frame kept warm behind the shelf
  holds neither.
- **Document replacement is per pane.** `PaneHandle::replace_document` is
  the one way a pane changes documents (every format, every view mode): the
  replaced session is disposed at the call, and its release (`Retiring`) is
  awaited by THAT pane's open only. A fresh pane has nothing to wait for;
  no open awaits or disposes another pane's document, so split mode can
  open several documents at once without a realm-wide lock.

## Phase 5: the split workspace

- **Layout.** `PaneTree` is pure data: a leaf is a `PaneId`, a split has an
  axis, a clamped ratio (`MIN_RATIO`..`MAX_RATIO`, and a drag never leaves
  a side under `MIN_PANE_PX`) and two children. No empty node and no
  one-child split can be built; `remove` collapses a split into its sibling
  and names the focus successor (a first child's close focuses the
  sibling's first leaf, a second child's its last). Unit tests cover split
  h/v, nesting, close, normalisation, the ratio clamp and a seeded random
  sequence against `check_invariants`.
- **Placement.** `ReaderHost::open_document(launch, target)`: `Active` (the
  Shell's commands: a drop, a warm reader's launch), `Pane(id)` in place
  (the pane keeps its id and replaces its document), or `Split { of, axis,
  side }` — a new pane with its own session, split off `of` on either side,
  taking focus (refused with `TreeError::NoRoom` when a half would be under
  `MIN_PANE_PX`).
  Transactional: the pane is created, placed and handed its box before its
  view mounts, and a refused placement closes it again. Panes ask through
  `PaneEnv.open` with a `Placement` (`Here` / `Beside`), never naming
  another pane. `MAX_PANES` is 4 (`PaneError::WorkspaceFull`, surfaced as a
  toast).
- **Resize.** A divider drag records a ratio and applies the last one per
  animation frame; the new layout reaches panes through the same
  `PaneManager::resize` path a window resize takes, so each pane's own
  viewport and zoom scheduling absorbs it.
- **Focus.** One active id in the manager. A press or keyboard focus inside
  a pane's entry makes it active (capture phase, host-owned); events that
  bubble to the window are claimed by the pane they came from
  (`crate::pane::origin`), and only the active pane presents its paper to
  the shared backdrop. Inactive panes stay shown and live.
- **Close.** `close_pane(id)` removes the leaf, closes that pane only
  (`manager.close(id, successor)`), and re-lays out; every other pane keeps
  its session. The last pane closes with the reader.
- **Diagnostics.** The `host` block carries `layout` (the tree), and each
  pane `viewportArea` and `resources.zoom` next to its lifecycle, format,
  bounds and active flag.
- **Browser smoke.** `tests/browser/lifecycle.mjs` stage 13 opens a PDF, puts
  `public/samples/Split Notes.md` beside it (web-only hook
  `window.__mareaderOpenIn`), checks two ready panes with independent
  sessions, a pointer focus switch, a divider drag, then closes the PDF pane
  and checks the Markdown pane reads on with the PDF's engine session and
  rasters gone.

## Phase 6: document drag and drop

- **One source.** The reader rail's third tab, **Library**
  (`host/library/`), beside Thumbnails and Outline: a compact tree of the
  library's folders and files (`LibraryTree::from_blob` over
  `storage::load_library`, re-read whenever the tab is shown), a name and a
  format badge per row, no covers. Its file rows are the only thing that
  starts a split drag: `DocumentDragSource { document, book_id, path,
  format, label }` (`host/drag.rs`), pressed through window pointer
  listeners (no DOM `draggable`). A drop always creates a NEW pane (the
  same document may show in several); no session moves. A Library-row drag
  never imports anything. Pane drag handles, the keyboard placement, the
  shelf's "Open in Reader" zone and the Shell's shelf → reader carry were
  removed.
- **Open panes.** With more than one pane, the panel's top lists them
  (`[data-open-tabs]`): a click focuses the pane, × closes it; no drag.
- **Row click.** A workspace setting (Settings → Workspace,
  `WorkspaceSettings::library_click`): open in the focused pane (default,
  `Replace`), open as a new split (`Split`), or nothing (`DragOnly`; the
  keyboard still opens beside). A new split goes right of the focused pane,
  or down when only a vertical split fits (`library::beside_axis`). A file
  already open is focused, not opened twice. Opening resolves the row's launch synchronously from the store
  (`storage::resolve_launch`).
- **Session.** `DragSession` is `Idle → Arming → Dragging`: a press arms,
  the shared `DRAG_THRESHOLD_PX` (6 px) starts the drag, and only then is
  the geometry measured — once per drag. The session stores data only (no
  DOM refs); the drag loop is O(visible panes); subscribers are notified
  only when the shown target changes (`try_maybe_update`), which is also
  the only time the live region (`[data-drop-announce]`) speaks. A release
  after a drag swallows the row's click.
- **Geometry and targets.** `DropGeometry` = the workspace slot's client
  rect (read from `#viewer-slot`, nowhere else) + every placed pane's box
  from the layout. Targets: `Split { pane, edge }` for the four edges, and
  `Here { pane }` only on a pane with no document. A split is offered only
  when both halves keep `MIN_PANE_PX` (`tree::split_fits`, the same
  `split_rects` rounding the layout uses) and the workspace is under
  `MAX_PANES`. Scoring is the normalised distance to the edge; draws
  resolve Right > Bottom > Left > Top; the current target is kept until a
  rival beats it by `HYSTERESIS` (0.1) or the pointer leaves its pane.
- **Commit.** A release over a target becomes
  `WorkspaceCommand::OpenInDropTarget`; `ReaderHost::run` re-plans it
  against the workspace as it is now (`commands::plan`: unknown pane,
  occupied `Here`, full workspace and no room are refused with nothing
  changed), resolves the row's launch and places it through
  `open_document` (`Split` / `Pane`), so the manager creates the pane and
  it takes focus. Work that starts in an event handler runs inside the
  session's root owner (`HostSession::enter`): Leptos restores no owner in
  handlers, and `PaneManager::create` refuses to build a pane that would
  belong to no scope.
- **Cancel.** Escape (a capture-phase window listener installed only while
  a drag is live), the window losing focus, the frame leaving the screen,
  a release anywhere but a target, and the ReaderHost's dispose all end
  the session with the tree unchanged.
- **OS import drop.** Files dragged in from the OS are imports, never
  splits, and only while the library is on screen: one Shell listener set
  (`src/services/import_drop.rs`, Tauri's `tauri://drag-enter|leave|drop`)
  filters the paths through `reader_core::format`, and
  `RuntimeManager::import_dropped` hands them to the live library frame as
  `ShellFrame::ImportFiles`; the library imports them onto the shelf it
  shows (none on All), as its Add menu would. The Shell paints a dashed
  "Drop to add to your library" hint over the window while an admissible
  drag hovers the library. Over the reader the listener does nothing.
- **Fixed on the way (Phase 5 surfaces).** A pane's per-pane close sits
  below the title bar when the pane's top meets it: the bar's root is
  hit-testable over the whole top 48 px. A single pane's close pauses the
  pane's owner before cleaning it: its views stay mounted in the host until
  the next render, and a render effect the closing session had notified
  then ran against the purged arena.
- **Diagnostics.** `host.drag` is the session phase (`idle`, `arming`,
  `overTarget`, `overWorkspace`, `dragging`).
- **Browser.** `tests/browser/lifecycle.mjs` stage 14 (Markdown fixture):
  Library rows dropped right and nested bottom, focus on the new pane, the
  open tabs (focus, ×), a row click replacing the focused pane, Escape, a
  release back over the rail, the `Split` and `DragOnly` click settings
  (set through the settings sheet, whose Escape must leave the rail open),
  a full workspace (four panes) offering no target, and a dispose mid-drag. Stage 15: a PDF kept across three
  Markdown split/close cycles (same PDF session, pane and virtualizer
  counts back to the PDF-only baseline), then PDF + Markdown + TXT disposed
  with every pane owner released.

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
