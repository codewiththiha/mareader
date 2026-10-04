# Runtime-path cleanup audit

## Scope and method

The starting revision was `e4b4e4bead5958d0c1189b040d75b9683b43a365` on `pane-frame-runtimes`. The authored-code inventory contained 541 Rust source files plus Shell/native code, TypeScript/JavaScript engine and build tooling, browser tests, HTML and styles. Vendored pdf.js/cmaps and generated bundles are not hand-maintained cleanup targets.

The repository-wide pass checked module declarations, branch-changed paths and symbol/caller references. The detailed review followed the paths changed by the runtime experiments: Shell boot/frame gates; shared command vocabulary and frame transport; Reader/Library contexts and entries; workspace/pane factories, document boot/replacement/mirrors, raster scope ownership; shared titlebar/native subscription plumbing; and canonical build, staging, dev invalidation and their contract tests. A name containing "legacy"/"fallback" or having no cross-file caller was only a candidate, never sufficient evidence for deletion. Integration tests are discovered by Cargo, and functions used inside their own module are not orphans.

## Removed or consolidated

| Old path/state | Finding | Current owner/path |
| --- | --- | --- |
| `ShellApi::resolve_launch` synchronous query | A port implementation issued an async request and immediately returned `None`, implying a miss and leaving a ticket no caller awaited. | Command-only `ShellApi`; hosted Reader explicitly awaits `PortShellApi::resolve_launch`. The supported standalone entry reads its local store directly. |
| Reader's separate pending-open map plus transport ticket map | Two registries retained the same request/continuation. Production discarded the ticket. | One transport-owned, wakeable launch future. Answers remove their registry entry before waking; dropped futures and API disposal cancel entries and release wakers. |
| Empty reflow realm, bootstrap/warm promotion and in-realm `HostToPane::Open` | Remaining execution alternatives from earlier prewarming/in-place document reuse. | A documentless development host owns only chrome/mirror state. A real first open creates its first document frame; every subsequent open creates a fresh frame. Boot requires a launch. |
| `Session.launch` snapshot and `LibraryContext.id` | No consumers; the launch clone retained duplicate metadata/cover data beside the pane's authoritative launch signal. | Pane launch signal and frame/session identity already used by the actual owners. |
| Root contact/ready/paint/dispose notifications and multi-waiter boot lists | Gates already owned readiness/disposal. Notification handlers were no-ops or post-warm promotion leftovers. | One serialized navigation owns Ready/Painted gates; those still wake on cancellation and retain strict timeouts. Runtime stages/errors/boundary/stale telemetry remains. |
| Non-scoped raster owner fallback | A pane could use only its nonce if the new Reader scope was absent, bypassing whole-host scoped retirement. | Real document panes require the Reader's coordinator **and scope**; retirement uses the scoped owner. The standalone smoke/cover realm's existing local gate remains supported. |
| Repeated artifact lists, arbitrary HTML selection, missing dev route inputs | Builder/staging/restoration repeated layout definitions; the builder could guess an unrelated HTML file. The watcher omitted Library/Reader entry HTML and Trunk config changes. | `tools/runtime-artifacts.mjs` owns the four runtime layouts for all consumers. Only the target page or Trunk's normalized `index.html` is accepted. Missing required artifacts and IO failures are fatal. All eight runtime entry/config inputs participate in dev invalidation. |

Chrome mirror projection was separated into `frame_pane/mirror.rs`; iframe creation, nonce/port ownership, handoff and retirement stay in the lifetime module. This is a responsibility split, not a second pane implementation.

## Unwired behavior repaired

The old `window_bridge.rs` file was not declared by any module and was never called. It also referenced an obsolete services path. Meanwhile the current caption controls received a maximized signal with no external native-state updater.

The old orphan is gone. The live `AppTitleBar` installs its private `window_state` module, scoped to the current route, only on frameless native platforms. It queries real native maximize state, coalesces a resize storm into one in-flight plus one trailing probe, and checks signal/owner liveness after each await. macOS traffic-light behavior and styling are unchanged.

The shared native subscription now keeps a pending-registration callback alive **but inert** after owner disposal until its registration can be unlistened. Unlisten precedes closure release. This avoids freeing a WASM callback still held by the parent native registry. It does not retain the disposed route's reactive owner.

`storage::migrate_gloss_keys` lost its caller in the same split (it ran from the old bootstrap) while its doc comments kept pointing at it. It runs again from `LibraryContext::new`, before any pane can read a row's marks, and remains gated by its own durable flag. This is a data migration: leaving it unwired would have dropped existing readers' page-anchored notes.

## Second pass: dead helpers, duplicate pane setup, unused dependencies

A second repository-wide pass over the same revision looked for symbols whose only remaining references were their own doc comments, duplicate setup between the two pane implementations, and manifest entries no source in the crate reaches. Every deletion below was verified by a whole-tree token census (tests included) before it was made.

| Old path/state | Finding | Current owner/path |
| --- | --- | --- |
| `library_core::tracking::TrackingTree::track_at` | The per-rung decision read; no caller — the menu reads the effective answer. | `TrackingTree::resolve`/`tracked`; the tracking tests now assert whole-tree equality with `TrackingTree::tracking_root()`, so the removed reader cannot come back unnoticed. |
| `library_core::blob::LibraryBlob::awaiting_check` | A one-line public wrapper over `book_rows(&blob.books).any(\|b\| b.fp_pending)` with no caller. | The expression itself, at the rescan gate that reads it. |
| `library_core::conflict::Placement::is_destructive` | No caller: the ask sheet labels `Replace` in its own words and `ChoiceSpec` carries no danger flag. | `Placement::Replace`, labelled where the choice is built. |
| `Settings::active_preset` with `apply_preset`/`find_preset` and the `sanitize` dangling-selection clear | Stored state nothing read: every surface derives the active preset by comparing the live look, and a stored id could claim a selection the reader is not looking at. | Live-look comparison in the appearance menu; saving a preset no longer writes a field. |
| `app_ui::components::app_overlays::drag_overlay::DragOverlay` | No caller since the runtime split; the Shell paints its own `data-import-drop` hint from `install_import_drop`'s hover signal. | `src/app/mod.rs`'s hint. The `.drag-overlay`/`.drag-dropzone*` rules and their keyframes were the component's only consumer and are gone with it; the `--z-drag-overlay` token and the `DRAG_OVERLAY` layer class stay (the drag layer and the first-paint cover use them). |
| `app_chrome::hooks::frame_active::frame_is_active` | Non-reactive twin of the slot observer; no caller. | `use_frame_active`, the signal the traffic lights read. |
| `reader_runtime::state::viewer::ViewerSignals::reset_position` | Reset-on-close for a document that lived in the shared realm; every document now runs in its own realm, whose signals are fresh by construction. | The per-document realm's own `ViewerSignals`. |
| `public/reader/raster-protocol.ts` `RASTER_LANE_KEY` | Exported constant no module imported; the pane side reaches the lane through `window.parent`. | `public/reader/raster-coordinator.ts` and `public/engine/raster-lane.ts` keep the seam. |
| Duplicate pane setup: `frame_pane::FramePane::create` vs `pane::document::DocumentPane::create` | Two copies of the `ReaderContext` + `PaneSurface` build, plus two private `neutral_status` and `empty_launch`. | `crates/reader-runtime/src/pane/base.rs`: `contexts()` and `empty_launch()`, called by both panes; `bare_launch(path)` spreads `empty_launch()`. |
| An empty `BootStage::Disposed` arm in `src/app/frame.rs` | A branch whose body only explained why it was empty. | The comment, on the `Status` arm it belongs to. |
| Manifest entries nothing reaches: `app-ui`→`library-core`, `serde_json`; `reader-runtime`→`ui-geom`; `storage`→`leptos`; `reflow-core`→`serde` (and its dev `serde_json`) | Unused dependencies keep a graph edge alive that the dependency gate reasons about. | The crates' own manifests. |

## Third pass: visibility, and the end of the dead-symbol census

A whole-tree census ran again over the current tip (575 Rust files), this time asking a narrower question
than the second pass: for every `pub`/`pub(crate)` item, which REFERENCES name it — with comments and
string literals stripped first, so a name that survives only in a doc sentence, a NOTES entry or a vendor
bundle does not read as a caller. Two facts came out of it.

The dead symbols really are gone. The seven `pub` fns the second pass removed have no remaining mention
anywhere (`is_destructive`, `track_at`, `awaiting_check`, `apply_preset` are absent from the tree
entirely), no type in the workspace has zero code references, and the field census' only zero-use fields
remain the serde-wire shapes and the `ObserverBinding` destructure it already justified. Nothing to delete.

What was left is surface: 125 items whose only outside-file evidence was prose. 55 of them — functions,
consts and statics — were provably safe to narrow, and are now private: the item is a fn/const/static, it
has a real non-test use in its own file (so the `dead_code` lint stays quiet), no other file names it, no
same-file `pub use` re-exports it, it carries no `#[wasm_bindgen]`/`#[no_mangle]` export attribute, no
source gate or shipped JS/TS names it, and neither its declaration nor any of its uses sits inside a
`#[cfg(...)]` region.

| Old path/state | Finding | Current owner/path |
| --- | --- | --- |
| 55 `pub`/`pub(crate)` fns, consts and statics across 30 files | Named by nothing outside their own file; the `pub` advertised an interface no sibling module can reach. | The same items, one visibility word lighter. Examples: `virtualizer.rs`'s `publish_range`/`arm_flush`/`arm_now_flush`/`flush_banked_scroll` (the adapter's own plumbing), `host/mod.rs`'s lift/drag handlers, `host/tree.rs`'s ratio constants, `app-ui`'s `Loader`/`ApplyToAll` components (used by their own module). |
| `app-ui::…::use_custom_event::use_typed_event` | No caller anywhere: the live hook is `use_typed_event_from` (gloss wiring), which is this one plus the dispatch element. | `use_typed_event_from`. |
| `app-ui::…::use_custom_event::use_raw_event` | Same: `use_raw_event_from` is the live sibling (link navigation, selection, selection tracking). | `use_raw_event_from`; the module doc now describes what is left. |
| `use_custom_event`'s `pub use crate::events::dispatch_typed_event` | A second path to the dispatcher; every caller already reaches `app_ui::events::` directly. | `crate::events` (services use it there). |

Deliberately NOT narrowed, each for a reason a name census cannot see: the 45 candidate TYPES (a type's
name is usually absent at its use sites — the value comes back from a fn by inference — so narrowing one
below the visibility of a signature that names it trips `private_interfaces`, which `-D warnings` makes
fatal); items whose only callers are test-only or cfg-gated (narrowing makes the other build warn
`dead_code`); and the names the source gates read textually (`canvas_id_for_mode`, `take_session`, the
engine-smoke `disable`). The reader host's lift/drag/grab path was re-read end to end while checking the
census — it is live (the pane frame installs the gesture, the sink reaches `ReaderHost::begin_lift`, the
view draws the lifted card) and holds no remnant; nothing there was removed.

## Preserved deliberately

- Five artifact types and independent PDF/reflow document realms, with no Reader/Library warm slots or route reuse.
- Authenticated channel adoption, generation/nonce/role checks, paint-preserving handoffs, bounded failed boot/forced disposal and observable terminal counters.
- Workerless/unsupported-platform handling used by current supported deployments, ordinary loading geometry estimates and visible error states. These are not fallbacks to a retired reader implementation.
- Persistent library/settings/bookmark/gloss migrations, including older stored shapes/keys. Deleting them would lose existing users' data or reading position, violating "same functioning".
- Standalone development entries and off-WASM host-test shapes; a claimed hosted boot still never falls back to standalone.
- Existing real-realm browser lifecycle assertions and their test-only query composition helper. That helper is not shipped code or a production alternate runtime. New cleanup checks deliberately access native documents directly.
- Explicit user-requested window reload. Normal routes/pane close never reload Shell; the optional recovery action is not routine lifetime management.

## Validation coverage

Existing CI/workflows and fatal-warning policies are not weakened. Added checks cover:

- launch futures: answered miss versus pending, stored descriptor preservation, paired request IDs, exact wakes, drop cancellation, API disposal and ignored late/double answers;
- actual dialog path opening through the hosted async resolution, followed by disposal while its reply is held;
- no empty document iframe/artifact fetch in an unhosted documentless host, then exactly one realm on its first actual open;
- compiled Library titlebar against a deterministic native window/event boundary: initial/external maximize state, coalesced resize and disposal before registration acknowledgment;
- build copy fixtures: canonical page priority, supported normalized name, rejection of unrelated HTML, initial optional staging and no overwrite during restore;
- a source gate against the retired synchronous query, in-realm open/reuse and orphaned window bridge.

Compilation, engine smoke, native boot, actual browser regressions and memory replay run in Actions for the pushed revision. Local checks are source/syntax/contract checks only; no compiler, Trunk/bundler build, dependency or browser installation is needed locally. The pre-existing worktree-only executable-bit difference on `tools/build-dist.sh` is not a cleanup target.

This audit is not a claim that every large file can be reduced further, that all platform error handling should be removed, or that WebKit process-memory retention is solved. Current memory ownership and process-RAM limits remain in [memory/audit.md](memory/audit.md).
