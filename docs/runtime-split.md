# The runtime split: how the runtimes are built, loaded, and disposed

This document records the runtime technology as built (Trunk CSR,
wasm-bindgen `--target web` glue, Trunk 0.21.14 in CI). The per-pane frames
are described in [pane-runtimes.md](pane-runtimes.md).

## WASM targets

Three independently loadable WASM artifacts:

| Runtime | Package (Cargo) | Entry HTML | Artifact |
| --- | --- | --- | --- |
| Shell | `mareader` (workspace root) | `index.html` | `mareader.js` + `mareader_bg.wasm` |
| PDF pane | `reader-runtime` (`pdf` features) | `pdf.html` | `pdf.js` + `pdf_bg.wasm` |
| Text pane | `reader-runtime` (`reflow` features) | `reflow.html` | `reflow.js` + `reflow_bg.wasm` |

The Shell links the library (`library-runtime`) and the workspace host
(`reader-runtime`'s host half) and mounts both in its own document; it never
reloads. Every open document runs in a pane frame of its own, so closing a
pane drops its realm — the only way a WASM heap, which never shrinks, is
given back. The pane pages are built by their own Trunk configs (each with
`filehash = false` so the host can name them) and merged into `dist/` by
`tools/build-dist.sh`; `tools/check-runtime-artifacts.mjs` pins the set.

## Entry modules

- Shell: `src/main.rs` — mounts the Shell: title bar, sidebar, settings,
  menus, the runtime manager and its slots, the diagnostics surface.
- Library and workspace host: `library_runtime::frame::adopt_in_document`
  and `reader_runtime::frame::adopt_in_document` start a session in a slot
  of the Shell's document, paired with the manager over a `MessageChannel`.
- Panes: `reader_runtime::pane_frame` boots in `pdf.html` / `reflow.html`,
  says hello to the host with its nonce, and is handed its port and boot
  descriptor (`crate::pane_wire`).

## Loader mechanism

The manager creates a slot (`div.runtime-frame` in `#runtime-host`) and
starts the runtime's session in it over a fresh channel. Every message
carries the slot's generation; traffic stamped with another is counted,
never applied (§35).

Every step can fail on its own, and each failure is named on the error card
(src/app/boot.rs plus the protocol's `Failed` event). A pane frame whose
artifact does not load answers with silence: after 10 s the pane draws a
named error over itself and the console logs `[mareader] pane boot failed`
with the artifact; the Shell and the other panes stay up. A Shell artifact
that does not load leaves the page placeholder, which `public/shellBoot.js`
turns into a "did not start" state after 20 s.

### Slot states: active, warm, retiring

Starting a runtime is the expensive part of a route switch, so the loader
rarely runs on the click. Each slot carries a `data-mareader-slot`
attribute — `active`, `warm`, `retiring` — and the manager
(`src/app/manager.rs`) keeps at most one of each behind the screen:

| Slot | Visible | Meaning |
| --- | --- | --- |
| `active` | yes | the runtime the user is looking at; `z-index: 1` |
| `warm` | no | booted through `Ready`, waiting to be revealed |
| `retiring` | no | displaced, still disposing — off the critical path |

The two runtimes are warmed on different words. The shelf is warmed 700ms
after the reader settles on screen (`WARM_DELAY_MS`): it is light, and a
reading session always ends on it. The reader is warmed only on the shelf's
**intent signal** — `RuntimeFrame::ExpectReader`, which the shelf sends
(throttled to one a second) when the pointer is over or moving across the
grid, or a card is pressed or focused — never on the shelf's paint. A warm
boot stops at `Ready`: it never opens a document, because the PDF machinery
is exactly the cost that must not be paid for a book nobody asked for — the
existing `ShellFrame::Launch` opens the document on the real navigation, and
a revealed shelf gets a `Refresh` instead, since the row the reader left has
moved since it seeded.

A warm reader does not stay for free. Every intent signal restarts a
`WARM_READER_IDLE_MS` (60 s) clock, and when it runs out with the shelf
still on screen the reader is **evicted**: disposed through the same §12
exchange as any retirement and its frame removed
(`RuntimeManager::evict_idle_warm_reader`). The reader is the heavy runtime
— its frame keeps a wasm heap that never shrinks, pdf.js, the engine's
worker — so a reader kept "just in case" is exactly the memory the library
route is supposed to give back; with the eviction, the library route at rest
holds the library alone (`readerFramesResident 0` in the diagnostics probe).
The next intent boots a fresh reader; a click that beats it pays a boot with
the shelf still on screen, never a covered cold start. The browser suite
shortens the window through `?warmIdleMs=` on the boot URL.

A navigation whose warm frame is ready is a **promotion**: the same element
flips its slot to `active`. Same document, same realm, same WASM instance —
the boot is already spent. The outgoing frame becomes `retiring` in the same
synchronous block as the reveal, so two frames are never visible at once,
and its disposal runs behind the handoff.

Two constraints follow from the browser, not from the design:

- A hidden slot is `visibility: hidden`, never `display: none`. Browsers stop
  calling `requestAnimationFrame` in a `display: none` iframe, and `Painted`
  is rAF-driven — a warm frame hidden that way would never finish booting.
- The frame must be in the registry BEFORE its boot verdict is awaited. A
  promotion arriving mid-boot resolves from the same verdict, so if the frame
  is not findable yet the promotion concludes there is nothing to reveal.

A warm frame that cannot be revealed (its verdict is an error or the ready
timeout) is torn down and the transition falls back to a cold start: the warm
slot is an optimisation and must never cost the user the runtime.

## The boot contract

One build produces the frontend the app runs. `tools/build-dist.sh` runs the
shell page's Trunk build and the two runtime builds, merges them into `dist/`
and then VERIFIES the result (`tools/check-runtime-artifacts.mjs`: every
runtime artifact present and non-empty, and the shell page carrying its boot
placeholder). Tauri's `beforeBuildCommand` calls that script through
`npm run build:dist`; CI calls the same script; `tools/check-tauri-contract.mjs`
fails the build if either side starts building the frontend by another path.

`trunk build` alone is NOT the app's build: it emits the shell page, leaving
the runtime pages the frames load to 404. The two runtime builds each emit
their own
page, whose name the merge takes as it finds it (a custom `dist` dir makes
Trunk normalize it to `index.html`); a build that produced no page at all fails
there instead of shipping a `dist/` without one. — which is precisely how the packaged app
shipped a native window with an empty runtime host and nothing in the
terminal.

Development has the same guarantee in operational form: `npm run dev:frontend`
(`tools/dev.mjs`, Tauri's `beforeDevCommand`) builds all three artifacts,
starts `trunk serve`, probes the DEV SERVER for `index.html` + both runtime
artifacts + both wasm modules, and only then reports the boot as safe.

The runtime host is never empty (`src/app/boot.rs`):

| state | host holds | how it ends |
| --- | --- | --- |
| loading | the shell's own loading card | the runtime's own DOM arrives |
| active | the live runtime's DOM (`data-mareader-active`) | a transition starts |
| error | runtime + stage + cause, with a reload button | a reload boots again |

"The runtime is active" and "the runtime has painted" are different moments, and
the host is covered across the gap between them. A runtime mounts with
`mount_to`, which CLEARS the container, and its first render can be a suspense
anchor with no elements at all — so the shell re-covers the host whenever it
has nothing painted in it, and takes the card away when it does. That check is
a `MutationObserver` (`src/app/boot.rs`), not a timer: its callback runs in the
mutated task's own microtask checkpoint, so no other task can observe the host
bare, which a polling interval cannot promise.

Before the shell itself exists the page shows the placeholder that
`index.html` ships (`#shell-boot`, "Loading MAReader…"). The SHELL removes it,
in the same step that it uncovers the host — one owner for that moment, because
two of them is exactly how the window ended up uncovered between them.
`public/shellBoot.js` covers the case where the shell wasm never starts at all.
A failed boot paints the error state and names the artifact and stage in the
console — there is no fallback to a monolithic page, because that page no
longer exists.

## Mount targets

The shell owns one mount target:

```html
<body>
  … shell-owned overlays (noise, drag feedback) …
  <div id="runtime-host"></div>   <!-- exactly one active runtime mounts here -->
</body>
```

The manager replaces the host's content deliberately: the outgoing frame is
torn down before the incoming one mounts, and the host holds exactly one
runtime's iframe at a time. A runtime never reaches outside its mount root;
`document.body`-level chrome belongs to the shell.

## Instance creation

`RuntimeManager::start_reader(state, LaunchDocument)` /
`start_library(state)`:

1. Dispose the active runtime first (below) and await its completion.
2. Create the frame: an iframe at the artifact page carrying the boot
   descriptor (`?hosted=1&g=<generation>&n=<nonce>`), so a stale frame can
   never pass as the session that replaced it (§6/§35).
3. The frame's entry instantiates the artifact in its own realm and pairs
   with the shell over the channel; the session mounts inside the frame
   (`reader_runtime::start_session`, the composition root) — a fresh
   reactive ownership root and `ReaderRuntime` (begin_mount → mark_ready),
   the reader host (`crates/reader-runtime/src/host/`) with its pane
   manager, and the host's first pane (`crates/reader-runtime/src/pane/`),
   which builds its OWN `ReaderState`, effects, listeners, virtualizers and
   engine session under its own reactive owner. A warm session gets its
   pane too, waiting for the launch its promotion hands over.
4. The runtime reports `Ready`; the manager records the slot as
   `Slot::Reader { generation }` with the frame's generation. The launch
   data crossing the boundary is a serialized `LaunchDocument` (`book_id`,
   `path`, `resume_page`, `saved_fraction`, `blend_override`) — stable,
   minimal, serializable (§13/§14) — answered over the channel; the reader
   obtains everything else through its own services.

## Instance disposal

Disposal is a frame round-trip (`dispose_active`, `src/app/manager.rs`):

1. The manager issues the dispose over the frame's channel; the session
   tears down INSIDE the frame. While the session is still alive the host
   disposes every pane (`PaneManager::dispose_all`): each pane flushes its
   read point, claims its document session, closes the paper session,
   takes its virtualizers out of its registry and cleans up its reactive
   owner — listeners, observers, timers, effects — explicitly. The unmount
   follows, and its cleanup runs `ReaderRuntime::dispose`, which awaits the
   panes' tails (engine destroy awaited → sweeps → virtualizer disposal →
   each pane `Disposed`) and only then marks the runtime `Disposed`. The
   unmount is sufficient on its own: nothing inside the session holds its
   reactive owner, so dropping the unmount handle releases the whole tree
   and its cleanup still runs the host's disposal (a no-op when it already
   ran).
2. The frame answers `DisposeComplete`; only then does the manager remove
   the iframe (§12: phase 1 acknowledged, phase 2 removal — a strict
   timeout forces the removal either way). A session that holds no
   document — a warm reader being evicted or replaced — answers the same
   way at the end of its tail; it merely has no engine document to destroy.
3. The slot returns to idle; the next start builds a NEW frame, so no
   static, listener, or heap survives on the shell side either.

Nothing of the runtime instance outlives its frame — the realm is disposed
with it. What does survive is origin-level (localStorage entries, served
assets), which is what the baseline measured as application scope.

The manager never starts a new runtime until the previous runtime's dispose
completed (§5). What must not survive is live state, and it does not (the
browser suite's `at_baseline` gates it).

## Cross-runtime communication

One narrow channel, JSON-serialized both ways (§14): a MessagePort per frame
boot, every message stamped with the frame's generation (§35), so traffic
from a replaced frame is counted as stale and never applied.

- runtime → shell: the `ShellApi` trait (`crates/runtime-contract/src/
  boundary.rs`) — typed calls (`open_document`, `navigate_library`,
  `read_point`, the save/publish family, `resolve_launch`) — implemented
  for a hosted frame by the transport's port handle (`PortShellApi` in
  `crates/frame-transport/src/lib.rs`).
- shell → runtime: the `ShellFrame` protocol vocabulary
  (`crates/runtime-contract/src/protocol.rs`) for commands such as a launch,
  a refresh or a baked cover's answer, and the frame's own boot events
  answering back (`FrameEvent`: contact, stage, painted, failed-with-stage,
  `DisposeComplete`, the shelf's `ExpectReader` intent).

Shelf covers are the one piece of PDF work the library needs, and it is done
by neither runtime: the shelf's `BakeCover` ask goes to the Shell, which
mounts its own hidden bake page (`src/app/bake.rs` → `public/bake.html`,
script `public/coverBake.ts`: pdf.js and the engine's cover render, no wasm,
no runtime), drives it over `window.postMessage`, answers the shelf with
`ShellFrame::CoverBaked`, and removes the page a few seconds after the queue
drains. The Shell page itself still loads no engine, and no reader is ever
booted for a cover. A cold shelf asks from inside its own mount, before its
Ready verdict admits it to the Shell's frame registry: the baker keys the
ask on the frame generation, boots the page at once, starts the bake when
the frame is admitted, and prunes the ask if the frame is torn down first.

No drag crosses runtimes. A split is dragged inside the reader, from its
own Library panel. A file dragged in from the OS is an import, handled by
the Shell only while the library is on screen: the Shell's
`tauri://drag-drop` listener (`src/services/import_drop.rs`) filters the
paths to the formats the app opens and sends the live library frame
`ShellFrame::ImportFiles { paths }`, which imports them onto the shelf it
shows. The reader ignores that frame.

Unhosted sessions (the unit-test lane) use a storage-backed substitute; the
trait keeps exactly these implementations plus the recorder the host tests
use.

## Artifact sizes

`tools/check-runtime-artifacts.mjs` prints every artifact's size in the CI
build log. Each pane artifact is built with its own feature set, and the
dependency gate (`tools/check-dependency-gate.mjs`, `cargo tree` over the
wasm32 target graph) keeps the library's closure free of reader code.

## Asset loading

`styles.css`, `public/vendor` (pdf.js), `pdfEngine.js`, `readerEngine.js`,
`bake.worker.js` are referenced by all three HTML entries, so each build
emits them; the merged `dist/` holds one copy (`bake.html` and
`coverBake.js` ship with the shell page alone). pdf.js itself is not a
script tag anywhere but the bake page: the reader's engine imports it on the
first PDF open (`ensurePdfjs` in `public/engine/loader.ts`), so a reader
session that never opens a PDF never fetches or holds it. All three runtimes are
served from the same origin but live in separate frame realms — own
document, own window, own WASM instance. What stays shared is
origin-level: `localStorage` (one browser store; each runtime touches only
its own keys), the served assets, and the platform APIs each frame's
document can reach — while DOM, reactive state, WASM linear memory and
statics are per-frame. That split — origin-shared, instance-isolated — is
the boundary the rest of this migration keeps honest.
