# Reader architecture

How the reader is built today: the runtimes, who owns what, the invariants the
tests enforce, and the known limitations. Read it before changing runtime,
pane, engine or memory behaviour. Why the runtime split took its current
shape is in `docs/frame-lifecycle-alternatives.md`.

## Overview

- **Persistent Shell plus disposable route and document realms.** Shell
  owns navigation and settings without a window reload. Library and Reader
  workspace chrome each have their own iframe/WASM artifact; every Reader
  host is removed on Library return. Each document pane independently runs
  `pdf` or `reflow`. Artifact boundaries are enforced; see
  `docs/pane-runtimes.md`.
- **Explicit lifecycle.** `ReaderRuntime` is a state machine with
  generations, a resource registry and observable disposal
  (`crates/reader-runtime/src/runtime.rs`). Counters live in
  `crates/reader-runtime/src/diagnostics.rs`; the browser lifecycle suite
  (`tests/browser/lifecycle.mjs`) and `docs/memory-baseline.md` hold the
  measurements.
- **Host and panes.** `ReaderHost` and `PaneManager` own the workspace
  (`crates/reader-runtime/src/host/`); a document pane owns one session
  (`crates/reader-runtime/src/pane/`). The map is in
  `docs/lifecycle-ownership.md`.
- **Session-scoped engines.** Each pane owns one `FormatSession` per open
  document: `PdfSession` (`crates/pdf-engine/src/session/`, one engine
  session per sid in `public/engine/state.ts`), `MdSession` and `TxtSession`
  (`crates/reader-runtime/src/pane/session.rs`). The inventory is in
  `docs/session-ownership.md`, enforced by `tools/check-session-ownership.mjs`.
- **Split workspace.** `/reader` runs a `PaneTree` with up to four live
  panes, host-owned dividers, focus outline and per-pane close; see
  [Split workspace](#split-workspace).
- **Document drag and drop.** A file row in the reader's Library panel is
  the split-drag source; see [Document drag and drop](#document-drag-and-drop).
- **Appearance.** Split-mode blend with a shared MRU paper, and
  independent per-pane colour and texture; see
  [Workspace appearance and blend](#workspace-appearance-and-blend),
  [Split pane decoration](#split-pane-decoration) and
  [Independent pane looks](#independent-pane-looks).
- **Fit, moves and grab.** Fit follows the page on screen, panes move and
  lift, and empty space grab-pans; see [Fit, pane moves and grab](#fit-pane-moves-and-grab).

## Runtime structure

- Shell (`src/`) owns routing, the runtime manager, diagnostics, persistence.
- Library and Reader host are separate disposable iframe/WASM artifacts
  (`src/app/frame.rs`), never view slots linked into the persistent Shell.
  Reader workspace chrome, layout and mirrors live in the Reader host;
  `FramePane` owns the independent document iframes. Library return removes
  the Reader host and every document realm, with no Reader retention,
  prewarming or recycling behind Library. Entering Reader likewise removes
  Library; both routes remount fresh and only Shell persists.
- Frame roots must carry `h-full w-full`: a mount with `height: auto` gives
  the virtualizer an indefinite viewport and every page mounts at once
  (the "peak N page hosts" browser failure).
- The PDF engine is session-scoped: every document call names a session
  (`sid`), each pane's `PdfSession` owns one, and reader code reaches it only
  through `pane.pdf()` / `MountedPdf` (`crates/reader-runtime/src/pane/engine.rs`).
  Production PDF panes do not share an engine realm. Registration still
  pins each canvas/host to its session's elements (`data-engine-sid`), and
  the two-session engine smoke keeps that ownership invariant covered.
- A full-page raster is MAIN-THREAD work — pdf.js draws into the canvas
  synchronously; only parsing/decoding runs in pdf.js's worker — so the page
  lane has a host-wide two-slot cap (`public/rasterLane.ts`) shared across
  iframe realms, plus the existing `REALM_PAGE_LIMIT = 2` per realm. Four panes
  re-theming or rasterising together pace as one progressive sweep instead
  of stacking eight concurrent stalls; a reading pane alone keeps its two
  slots. The per-session queues and their teardown drain are unchanged, and
  the theme re-render paths (`rerenderLivePages`, `preparePagesForScrub`,
  the scrub settle) ride the lane instead of bypassing it. The lane-pump
  registry holds sessions by `WeakRef` and `destroySession` drops the entry
  before any other teardown step, so the registry can never pin a session
  (its surfaces, thumb cache, pdf proxy) past its close. Queued render jobs
  re-check `disposed` at the rAF edge AND at the front of the lane, so a
  session that retired while work waited starts nothing. The additional
  host-permit await re-checks page/session/generation liveness too. Its
  wake is session-owned and weakly held by the host; frame removal reclaims
  only that frame's leases; host removal reclaims its whole descendant scope.
  The bake's pixel
  readback runs in the bake worker (a transferred `ImageBitmap` read through
  the worker's own canvas), so a theme change pays no main-thread
  `getImageData`; the inline kernel stays the no-worker fallback, and a
  dead worker degrades one frame, not the page. A raster small enough that the
  crossing costs more than the filter (`INLINE_BAKE_MAX_PIXELS`) takes the
  inline kernel on purpose — a thumbnail is that small, and a rail-wide
  re-bake is dozens of them; both paths run one kernel, so which one ran never
  shows in the pixels. The bake lands ON the visible canvas (its paper fill and
  composite are one synchronous turn on the destination), so a themed page
  costs one read, one fill and one draw instead of a third full-page surface
  plus a blit. `unregisterPage` no longer
  sweeps the document: a window move unmounts pages constantly, and the
  sweep belongs to quiescence (the render cadence `CLEANUP_EVERY`, the idle
  timer, the reader's scroll-idle sweep).
- PDF pages never show a placeholder: an unrendered page is blank until
  its full-resolution render lands, and a bitmap left at a stale scale asks
  for its crisp render at once. The strip's fling gate keeps pages a fling
  sweeps past from rasterising; its speed-aware visibility
  (`in_view_signal` in `formats/pdf/strip.rs`) lets a page inside, or
  within 320 px of, the viewport render immediately at reading speed, and
  after a 60 ms dwell (`IN_VIEW_DWELL_MS`) mid-fling. A timer re-checks at
  the dwell deadline, so a page never waits for another scroll event. See
  `docs/memory/fling-gate.md`.
- In the disposable Reader host: `start_session` (composition root) → runtime →
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
- The Shell loads **no Reader or Library implementation**. Its two-slot
  raster coordinator holds weak wakes/plain leases; Reader hosts borrow the
  same object. Dependency/artifact gates keep runtime code out of Shell and
  `PDFReader` imports/scripts out of Library, Reader host and reflow.
  Shelf cover bakes remain isolated in the Shell's short-lived JS-only page.
- `*_bg.wasm` is wasm-bindgen's file naming, not an extra instance: exactly
  five artifact types ship — Shell, Library, Reader host, PDF and reflow.
- Memory docs live in `docs/memory/` (index `docs/memory/README.md`):
  `rules.md` is the binding rule set for new code (frame-scoped release,
  dwell before expensive work, bounded caches with drains, zero-and-remove
  for canvases, weak module references, observable teardown); `audit.md`
  is the subsystem audit — every allocation owner checked
  against the rules, one fix applied (entry snapshots are now zeroed on
  session teardown, not merely dereferenced); `fling-gate.md` is the
  churn record. New memory-sensitive code reads `rules.md` first; a new
  audit updates `audit.md`.

## Fresh route realms and document handoff

The Shell manager owns actual route iframes in `Active`, `Incoming` and
`Retiring` slots. Every route departure disposes/removes that realm: Library
is absent while reading, Reader and all document realms are absent at the
settled Library baseline. Neither runtime prewarms, rearms or recycles.
A return creates a fresh iframe/WASM instance from durable data, not a
window reload. Pointer/focus on Library never allocates Reader.

Handoffs retain outgoing pixels until incoming Ready/Painted. Newest
navigation cancels a pending incoming host and wakes its gates; late boots
of either kind cannot reveal a cancelled route. Library defers incoming
startup writes until reveal, and its cover-bake queue/task/page is cancelled
when it retires. See `docs/runtime-split.md`.

Every **document replacement** after adoption uses a fresh iframe, even
within one format. The pane id/layout/focus stay stable; the old document
pixels remain until the new frame has mounted and actually painted the
document. PDF paint is a successful full-resolution current-canvas
completion in every mode, never a mount or fallback timer. Text preserves
its anchored synchronous-DOM paint path. Rapid requests supersede the
incoming nonce; late hello, metadata and paper cannot take over the latest
open. The chrome reports Opening/new launch rather than old metadata.

An outgoing document flushes its read point and disposes its owned work.
Its 1.5 s retirement fallback removes only that nonce, not an entire batch
of frames. Owned 10 s hello/30 s paint deadlines produce named errors and
are cancelled on adoption, paint or retirement. Removing the iframe also
reclaims its host raster leases. A closed pane does not dispose siblings,
and returning to Library never reloads the Shell.

A persistent host must not cache one final JSON report per closed frame.
Final reports fold into one bounded plain-data aggregate; failed or
unverifiable terminal evidence is never replaced by a later success.
See `docs/pane-runtimes.md` for the protocol, paper handoff and regression
coverage, and `docs/memory/audit.md` for measurement limits.

## Execution-path cleanup

`ShellApi` contains commands/writes, not a synchronous port query. Hosted
Reader path opens await a transport-owned launch future; response, drop and
disposal remove the request before waking its continuation. A separate
pending-open registry is unnecessary. Standalone development reads its store
locally and a claimed hosted marker never falls back to it.

Document realms always boot with a real launch. A documentless development
host owns only its mirror/chrome until the first open; every later open gets
a fresh realm. There is no in-realm `Open` wire message and no empty warm
realm to promote: a document realm exists because a document is open. The
scoped native window-state updater is installed
by each live `AppTitleBar`, with coalesced probes and late-registration-safe
unlisten.

The rule for pruning a path here: a name that reads as "legacy" or "fallback",
and a symbol with no cross-file caller, are CANDIDATES, not findings. Cargo
discovers integration tests that no `src/` file calls, a helper used only
inside its own module is not an orphan, and a doc comment that still points at
a removed caller is a clue to read, not proof. The judgement is made after
reading the module and every test that reaches it — and the `docs/**` passages
the removal invalidates are rewritten in the same change, so the map never
describes a tree that no longer exists.

## Invariants (enforced by tests — never weaken them)

- Shell diagnostic counters are authoritative; runtime digests merge in only
  keys the shell does not already own (`src/diagnostics.rs`, unit-tested).
- Per kind, `created - completed == (active is X)`. The
  equality is checked after retirement settles. At Library baseline
  no Reader is active/warm/retiring: Reader created/completed are equal and
  Reader/pane iframe residency plus raster active/queued/owners are zero.
- Leaving the reader cancels in-flight page renders synchronously with the
  click (the active pane's `PaneCommand::PrepareLeave` →
  `cancel_page_renders`) before the navigate
  command crosses the frame channel; the session destroy during disposal
  remains the single teardown path.
- Browser peaks: page hosts ≤ render window + zombie cap, active renders ≤
  page-lane slots, counters drain to zero at baseline.
- At most one workspace view slot is active. Every placed document pane is
  visible; at most one document iframe is visible per pane. A
  hidden slot is `visibility: hidden` — **never `display: none`**, which
  starves the iframe of `requestAnimationFrame` and would therefore never
  produce `Painted`.
- Two frames may differ, two VISIBLE frames may not: hiding the outgoing
  frame is part of the reveal, not a follow-up task.
- Nothing is disposed while it is on screen (`run_retire` re-asserts this for
  any retirement that did not come from a reveal).
- Per kind, `created − completed == (active is X)`: the
  retired session's disposal still runs to completion, just not in front of
  the handoff.
- A Library return creates a new generation/JS realm, never the old Library
  frame. Reading has zero Library frames and no Library-owned bake page.
  Cancelled Library boots cannot replace a still-visible Reader or later
  reappear. Shell identity and durable settings/read points survive.
- Disposal epoch belongs to the host's document-session count: every open
  and held-document close claims it, and changing a document realm does
  not restart it. The reported runtime generation is Shell-owned — the
  reader-session count, advancing once per fresh Reader-host session
  across frames; Reader hosts never rearm.
- The theme pipeline's `gen` (public/engine/theme/pipeline.ts) moves only
  when a bake INPUT moves (filter | blend | `--color-paper`), never on a
  root-style write alone: the engine's own `--pdf-paper*` publications
  during a zoom used to invalidate every in-flight bake and loop re-renders
  (the zoom flicker).
- The film-grain `.noise-overlay` lives once in each route document
  (created by `install_frame_theme`), next to the body classes that drive it.
  Reader-host grain covers its chrome and panes; document realms paint their
  own tokens/motion without a second grain layer. Shell installs no overlay.

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

## CI

GitHub Actions is the build: `CI` (format, clippy, wasm check, dependency
gate, `cargo test`, web contracts, macOS shell) runs on every push that
touches code; `Deep CI` (browser lifecycle baseline, Tauri boot smoke, split
memory replay) runs on pushes touching app or engine paths, and `[skip deep]`
in the last commit's subject drops all three of its jobs when the change cannot
move a byte, a wake or a release — the lists that decide live in `AGENTS.md`.
Neither workflow reads `docs/**`, so a docs-only push runs nothing.
Details: `docs/ci-architecture.md`.

## Measured, not assumed

The browser lifecycle suite verifies actual Library/Reader-host iframes,
independent document realms, fresh Reader generations across four returns,
zero Reader residency under repeated shelf pointer/focus/presses, cancelled
incoming Reader boots and a preserved Shell marker. The host sampler retains
its no-blank/no-two-visible-routes checks; worker/render/prefetch/canvas and
virtualizer teardown assertions are unchanged. Mixed workspace proofs also
require the Reader-host iframe itself to be gone, not merely empty.

`tools/measure-split-return.mjs` samples renderer PSS plus route/document
residency in Chromium and WebKit, with idle and continuous shelf intent.
For the current policy, every +2 through +70 second sample must have zero
Reader/pane frames and drained root raster leases. Historical cached builds
remain comparison data. No zero-growth or immediate RSS-reclamation claim
follows from realm teardown alone; use exact-revision replay measurements.

## Pane ownership boundaries

The host owns the workspace and each pane owns one document session. These
boundaries are already pane-scoped and must not regress:

- Every pane-owned DOM lookup goes
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
  `runtime-contract` may not depend on `ai-core`). The reader never writes
  the origin's store or reloads a window itself, and
  `tools/check-host-boundary.mjs` fails on any such line outside
  `context.rs`'s `StandaloneApi`.

## Realm-wide state

The persistent host owns authoritative settings, pane looks, layout/focus,
and shared PDF-paper recency. Each PDF iframe has one engine session,
its pdf.js worker, bounded pools/caches and paper/theme observers. Text
frames have their own parsing/layout/theme state and no PDF engine. Page
ids may repeat across frames; registration still pins owned elements.

Shared blend repeats the latest focused PDF paper across panes/gutters;
text focus retains it, PDF close falls back to the next publisher, and
closing the last PDF explicitly clears it from surviving text frames.
Independent blend keeps each pane's paper local. Scoped scrub is routed
by the **host's** `data-appearance-scope` into the selected realm, not
matched against iframe ancestors. Menu retention is pane-local in
independent mode; end messages settle every realm.

Host raster scheduling has two slots, weak wakes and at most two requests
per document realm. The local queues retain/settle their own jobs and
cancel permit waits before teardown. Diagnostics combine live/disposal
frames with one bounded final aggregate and the host lane's gauges. No
unverifiable frame report may be treated as drained. Per-pane session
ownership, liveness stamps and quiescent sweeps remain enforced by
`tools/check-session-ownership.mjs` and the engine/browser suites.

## Split workspace

- **Layout.** `PaneTree` is pure data: a leaf is a `PaneId`, a split has an
  axis, a clamped ratio (`MIN_RATIO`..`MAX_RATIO`, and a drag never leaves
  a side under `MIN_PANE_PX`) and two children. No empty node and no
  one-child split can be built; `remove` collapses a split into its sibling
  and names the focus successor (a first child's close focuses the
  sibling's first leaf, a second child's its last). Unit tests cover split
  h/v, nesting, close, normalisation, the ratio clamp and a seeded random
  sequence against `check_invariants`.
- **Placement.** `ReaderHost::open_document(launch, target)`: `Active` (the
  Shell's commands: a drop or an in-session launch), `Pane(id)` in place
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
  (`crate::pane::origin`), and a focused PDF pane presents its paper to the
  shared backdrop — a reflowable pane never presents, so the last focused
  PDF's colour holds across MD/TXT focus (`PaneRuntime::focus` →
  `PdfPane::present` is session-only). Inactive panes stay shown and live.
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

## Document drag and drop

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
- **Title-bar overlap.** A pane's per-pane close sits
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

## Workspace appearance and blend

- **Confirmed DOM contract:** a split does NOT use one physical paper surface.
  Every pane has its own `[data-pane-root]` box and each PDF page has its own
  `.pdf-page` host. Ordinary blend mode intentionally paints the latest
  focused PDF's same `--pdf-paper-baked` value onto each pane root and the
  outer `.reader-bg`; the common colour creates the visual impression of one
  shared backdrop. Reflow panes therefore stand on the PDF's paper in ordinary
  blend mode. The MRU publisher is the shared fallback; focus on MD/TXT holds
  the last PDF colour instead of clearing it.
- **Independent mode:** `PaneThemes` owns temporary looks by `PaneId`. The
  modes need the split they serve (see
  [Independent pane looks](#independent-pane-looks)): a lone pane stands them
  down and takes the look its own colour and texture were promoted into, so
  the shared chrome and the outer reader backdrop keep the look the reader is
  looking at; noise remains global. The pane painter writes
  base/tint/filter/reflow/texture tokens onto each pane root. Each PDF session
  pins its `EngineSession.themeRoot` to the root containing its registered
  page and owns `themePipeline` (actual-input fingerprint + generation), so
  page rasters, thumbnails and detected-paper composites use that pane's
  filter/blend/paper, never whichever pane last painted `<html>`. The rail's
  cells hold the host's copy of a pane's bake, so the pane says when that
  bake moved (`ThumbsStale`) and the host renders its cells again. The MRU
  publisher still updates the ordinary shared paper; independent CSS selects
  `--pane-pdf-paper-baked` only inside the corresponding PDF pane.
- **Blend and native chrome:** ordinary blend repeats one colour across the
  pane roots. Independent blend keeps each PDF's baked paper local, each
  reflowable pane's own `--tx-paper`, and the global paper on gaps outside
  panes. Pane-entry clipping + isolation prevent a PDF's CSS blend/texture
  from sampling or painting through an adjacent pane; the titlebar glass,
  native macOS traffic-light region and window-level backdrop remain on the
  remembered global theme because no pane painter mutates document-root
  tokens.
- **Edge/lifetime handling:** active-look reads subscribe to the pane-theme
  version (the menu selection and dials follow focus/edit changes); engine
  generation advances only on actual per-session filter/blend/paper changes;
  renderer landing checks that generation; theme-root observers are per
  session, weakly keyed, and disconnected at the start of teardown. Root
  replacement retargets the observer. A texture/grain/ink-only edit does not
  rebake PDF pixels.
- **Proof:** `tools/engine-smoke/sessions.ts` renders two live PDFs through
  distinct pane filters, then changes/rebakes only one and checks distinct
  local papers while the MRU shared paper follows focus. Browser lifecycle
  Stage 13 uses real appearance controls on PDF | MD, asserts Dark MD + Dim
  then Light PDF, and injects divergent paper colours while blend is on to
  prove the PDF colour cannot recolour MD or shared chrome/gutters.

## Split pane decoration

- **Focus paint:** `.reader-bg.split-workspace [data-pane-id]` creates an
  isolated pane stacking context; the active entry receives the higher
  sibling z-index and its focus outline paints above that pane's content.
  The outline uses a configurable inset stroke rather than a Tailwind ring
  whose stacking could be obscured by a later pane.
- **Settings:** split-only pane controls appear in the Reader Settings →
  Theme tab, only while the host has two or more placed panes. The title-bar
  Appearance popover has none of these controls; its independent-theme
  toggle sits after all appearance dials at the bottom. Outline width is
  0–8 px (default 2); colour is Auto (follows the global accent), named
  swatches styled like the highlighter palette, or custom RGB; spacing is
  0–24 px (default 0); pane-box shadow defaults off; square corners are the
  default. Values persist in `WorkspaceSettings` and sanitize on load.
- **Geometry and ownership:** spacing creates the same full margin at all
  four workspace edges and the same total gap between adjacent pane boxes
  (each side of a shared divider contributes half). Pane viewport bounds
  receive those insets, so PDF and reflow dimensions match the visible boxes.
  Shadow/radius/clip live on the host's outer `[data-pane-id]` box, not on
  PDF page hosts. Grain and document appearances are unaffected.
- **Regression proof:** browser lifecycle Stage 13 checks that the title-bar
  Appearance menu has no pane decoration controls and its independent-theme
  toggle follows the film-grain section; split-only controls are in Settings
  → Theme. It exercises the palette-style colour swatches, 5 px outline,
  10 px rounded box, outer pane shadow, 12 px inner gap and 12 px margins on
  every workspace edge, then confirms geometry restores when spacing resets.
  Host unit tests cover outer and shared-edge insets plus narrow bounds.

## Fit, pane moves and grab

- **Fit uses the page on screen.** The open still seeds every page with page
  1's box (a serial size probe over a long book looked like a hang); page
  hosts now report the scale-1 size each render produced
  (`PageMetrics::rendered`, written untracked so it never rebuilds a layout),
  `FitDims::of` prefers it, and `zoom::target::page_rendered` re-resolves an
  active fit when the page under the reader turns out to differ (resume page,
  page flips, jumps). A tolerance absorbs the engine's whole-pixel rounding so
  no refit/re-render loop can start.
- **Open at Fit Width (every format).** Markdown and text in the stream
  seed the startup fit too (`startup_scale` does not exempt the stream).
  The startup default is Fit Width, with a one-shot gate
  (`Settings::startup_fit_width`) moving installs that persisted the old
  default.
- **Move items.** In a split the view menu shows Move Left/Up/Down/Right
  (`PaneTree::move_pane`); with one pane it keeps Split Right/Down. A move
  crosses the nearest split along its axis; the other side is matched level
  by level, so a grid swaps one cell, a pair passes a tall pane as a column,
  and a whole-side swap flips the ratio so each side keeps its size.
- **Grab and lift** (`host/grab.rs`, `host/lift.rs`). Empty space is
  anywhere with nothing to select or press under the pointer: the gutters
  and the white of a page or block around and between its lines (a glyph
  hit test on the target's own text runs, `text_at`); links, images, marks
  and controls are excluded. It shows the hand and drags its nearest
  scroller in both axes with a decaying fling; the press keeps its default
  so focus still follows a click. In a split a still hold of `HOLD_TO_LIFT_MS` (1 s, ring
  duration published from the same constant) lifts the pane. While held the
  workspace is laid out without it (`lay_out` in `host/mod.rs`), so its
  neighbours fill its place; a release docks it beside the target's nearest
  edge (`PaneTree::dock`; a pane too small to halve is swapped instead). Layout-only: sessions are untouched. Owner cleanup removes listeners,
  releases pointer capture, clears the hold timer and stops the weak-state
  fling loop. Animation trampolines do not form a self-retaining Rc cycle.
- **macOS traffic lights.** tao re-applies `trafficLightPosition` from every
  `drawRect:`; the native layout used a different container height (and
  collapsed it on hide), so resizes ping-ponged it (blink) and autoresizing
  squeezed the buttons (ovals). It now agrees with tao on container and x,
  centres via the buttons' `origin.y`, pins their natural size, and applies in
  the event's own main-thread turn. `tools/check-chrome-contracts.ts` mirrors
  the y inset.
- **A frame's Tauri surface.** The bar and the caption are mounted by a ROUTE,
  so they live in a frame document, and a frame's access to Tauri is not
  uniform: macOS and Linux get no initialization script in sub-frames
  (CVE-2024-35222) and are served by the facade `public/tauri-relay.js`
  publishes from the parent's API, while Windows is documented the other way
  round — wry adds scripts to subframes there, so a frame holds its own real
  `__TAURI__`. Two things do not follow from having that object: an `emit` is
  delivered by scripting the MAIN frame, so a `listen` registered on a frame's
  own `event` namespace is unreachable (the caption's `tauri://resize` probe,
  the shelf's focus and cover-progress listeners and the AI stream all listen
  from frames), and the injected drag-region script acts only on an element
  that carries `data-tauri-drag-region` itself. The relay therefore re-points a
  frame's `event` namespace at the host frame and installs the drag listener
  with the same semantics as Tauri's — including `"deep"`, which is what makes
  a CONTAINER a region: `#toolbar-row`, its band and the sidebar's header claim
  their subtrees and let nothing else, so the bar drags while its buttons stay
  buttons and the search pill opts out with `"false"`.
- **The boot screen.** `#shell-boot` (index.html) is the page's own placeholder
  and `src/app/boot.rs` paints the runtime host's cover after it; both wear the
  app's loading mark and a line of copy that gets more specific as the wait
  grows (`public/shellBoot.js`, whose last stage is the failure report). A
  healthy boot used to be a blank themed sheet — clean, and unreadable as
  anything but a hang on a webview that takes seconds to paint its first frame,
  which is what a cold Windows launch does. `public/bootPaint.js` hands the
  remembered paper to the native window too, because no stylesheet can reach
  that layer and its default is white.

## Independent pane looks

Two preferences, one stored `Appearance` per pane, and a family of that look
is per pane only while the preference that owns it is in effect: colour is the
theme switch's, texture is the texture switch's, and independent themes carry
texture with colour because a look they own is a whole look.

- Turning independent themes on keeps the focused pane's look and gives
  every other pane its own tint hue; a pane born while on gets one too.
  `theme::distinct_hue` picks at random inside the middle half of the
  widest gap between the hues showing (unit-tested), so no two match;
  an untinted start gets `PANE_TINT_STRENGTH`.
- Turning independent textures on does the same for the texture family: the
  focused pane keeps its pattern and dials, every other pane gets a mode of
  its own (`theme::distinct_texture`, random among the modes not already
  showing), and a routed texture edit — the mode or either dial — lands on the
  focused pane alone. A pane's `texture-*` class comes from the look the host
  routed to IT (`pane_frame::realm` derives the `TextureSignal` from
  `viewer.look`, not from settings), which is what lets a text or Markdown
  pane be textured apart from the PDF beside it.
- `workspace.shared_base_mode` (default on; Settings → Workspace, "Light,
  Dark and Dim change every pane"): only colour is per pane. Panes show
  the global base (`PaneThemes::shown`) and a routed base switch writes the
  global base. Off lets each pane own its base too, which the lifecycle
  pane-theme stage exercises by turning the setting off. The row is disabled
  while the colour mode is off: sharing a base only means something to a pane
  that owns a colour.
- Both switches are in the appearance menu (each row appears with the split it
  serves) and in Settings → Workspace, so the preference is readable where a
  reader looks for the word — the modal carries the longer explanation.
- The mode needs the split it serves, so it stands down with ONE pane
  placed: the surviving pane's own colour is promoted to the global theme as
  the last split collapses (`PaneThemes::promote`, read while the split is
  still live), which is why the single pane left and the shared chrome agree
  by construction. The stored toggle (`independent_themes`) stays on, so the
  next split brings the mode back by itself with the survivor's colour
  untouched and the new pane seeded beside it. Turning the toggle off by hand
  drops this family's overrides for good; a pane's texture half stays in the
  map while `independent_textures` owns it, because a switch never clears the
  family it does not route — and switching the colour mode on re-seeds hues,
  not patterns, for the same reason.
- Re-raster is per pane: `refreshTheme` (public/pdfEngine.ts) refreshes
  only sessions whose own pipeline generation moved, and a routed slider
  drag marks `data-appearance-scope` on the document so the engine scopes
  the raw-raster scrub (and its CSS class, on the pane root) to that pane's
  sessions. The engine smoke asserts an untouched session renders nothing.

## Known limitations

- The Shell accepts boundary traffic only from the live frame — plus a
  RETIRING frame's terminal words (read point, settings, cover, gloss,
  digest, status), whose last digest is the evidence the disposal baseline
  reads. A `BakeCover` ask is gated on an incoming or active Library,
  because a cold shelf asks from inside its own mount.
- Pane teardown is observable: every digest carries `host` (lifecycle,
  activePane, per-pane lifecycle/bounds/resources, panesCreated) and the
  browser suite asserts one ready, focused pane with a fresh id per open
  and an empty, disposed workspace after close (`assertHostWorkspace`,
  `assertHostDisposed` in `tests/browser/lifecycle.mjs`).
- Native smoke and geometry contracts do not visually prove circular macOS
  traffic lights or absence of resize blinking. A real macOS visual check
  remains separate from the desktop/narrow browser evidence.
- Neither lane MOVES a window: `tools/tauri-smoke.mjs` runs under a window
  manager that does not honour `start_dragging`, and a browser has no window to
  move. The `stage0-window-chrome` stage of the browser suite therefore pins the
  COMMANDS the chrome issues — which press becomes `plugin:window|start_dragging`
  and which does not, what a double-click becomes, where a frame's listeners are
  registered, and that the caption's glyph follows the answer the window gives.
  Whether the window obeys is the desktop's business.
- The drag overlay is the SHELL's, not a component: `install_import_drop`
  (`src/services/import_drop.rs`) listens for the native drag events and
  returns the hover signal the Shell paints its `data-import-drop` hint from
  (`src/app/mod.rs`). The app-ui `DragOverlay` that lost its caller in the
  runtime split was deleted with its stylesheet block. The frameless
  caption's maximize/restore glyph is live again through the titlebar's own
  `window_state` module (`app_title_bar.rs`), which replaced the orphaned
  `window_bridge.rs`. That module answers a probe and never guesses: an
  unavailable answer leaves the last state on screen, because writing
  "not maximized" for "the window did not say" is how a glyph comes to look
  deliberately wrong. `install()` deliberately does not gate on the surface
  being present — in a frame it is published by a script, so it can arrive
  after the bar mounted, and the next resize finds it.

<!-- // only the changed file was rewritten -->
