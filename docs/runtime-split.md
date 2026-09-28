# The runtime split (Phase 2): how the three runtimes are built, loaded, and disposed

This document is the Phase 2 §1 record: the actual runtime technology, decided
from the existing build (Trunk CSR, one `index.html` target, wasm-bindgen
`--target web` glue, Trunk 0.21.14 in CI), not assumed.

## WASM targets

Three independently loadable WASM artifacts, one per runtime lifetime:

| Runtime | Package (Cargo) | Entry HTML | Artifact |
| --- | --- | --- | --- |
| Shell | `mareader` (workspace root) | `index.html` | `mareader.js` + `mareader_bg.wasm` |
| Reader | `reader-runtime` | `reader.html` | `reader.js` + `reader_bg.wasm` |
| Library | `library-runtime` | `library.html` | `library.js` + `library_bg.wasm` |

Each runtime is a separate `[[bin]]` in its own package, so each artifact
contains only its own module tree: the shell artifact has no reader or
library code in it, and the library artifact has no `ReaderState` (the
compiler cannot even name it — the type lives in `app-state`, but no library
crate imports it; a compile error is the kill switch, not a review rule).

`reader.html` / `library.html` are standalone loadable pages: they boot their
runtime against the real production entry (`run_standalone`), which is the
build-level proof that each artifact stands alone. The packaged app and
`trunk serve` use `index.html` only; the two extra pages are built by their
own Trunk config files (`reader.Trunk.toml`, `library.Trunk.toml`, each with
`filehash = false` so the shell can name them) and merged into `dist/` by
`tools/build-dist.sh`.

## Entry modules

- Shell: `src/main.rs` (`mareader` bin) — mounts the shell: route state, the
  runtime manager and its frames, the persistent overlays, the diagnostics
  surface, the frame channel the runtimes answer on.
- Reader: `crates/reader-runtime/src/bin/reader.rs` — reads its launch
  descriptor and mounts the reader session (see below); `run_standalone`
  boots without a shell.
- Library: `crates/library-runtime/src/bin/library.rs` — same shape.

## Loader mechanism

The shell never imports a runtime artifact. It loads one by pointing an
iframe at the artifact's page (`/library.html`, `/reader.html`) with a boot
descriptor in the URL (`?hosted=1&g=<generation>&n=<nonce>` — §6): the frame
resolves its own module the ordinary way, and the WASM instance is created
in the frame's own realm. The frame IS the boundary — a replacement runtime
gets a fresh browsing context with fresh statics, so the
instance-per-session claim below survives the split.

Module evaluation does NOT start a runtime. The sequence the manager runs, in
order, is:

1. Create the iframe at the artifact page with its descriptor; a frame that
   does not fully claim the descriptor is torn down (§8).
2. The frame's own entry fetches the page, loads the glue, instantiates the
   wasm-bindgen module inside the frame, and pairs with the shell over the
   channel (`crates/frame-transport`).
3. The entry starts the session in that realm — the launch descriptor is
   answered over the channel (`resolve_launch`) — and reports the boot
   stages until `Ready` paints.

Every step can fail on its own, and each failure is named on the error card
(`module load`, `init`, `start`; src/app/boot.rs plus the frame's protocol
`Failed` event): a missing artifact page or glue fails during load, a wasm
that will not instantiate fails at init, a missing export or a throwing
start fails at start.

A frame that dies — replaced session, crashed boot, a message stamped with
another generation — never passes its traffic on: every message carries the
generation that issued it, and stale traffic is counted, never applied
(§35). Compiled code still caches in the browser's cache, but no module
instance is SHARED: each frame builds its own.

### Slot states: active, warm, retiring

Loading a runtime is the expensive part of a route switch, so the loader
rarely runs on the click. Each frame carries a `data-mareader-slot`
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
assets), which is what Phase 0 measured as application scope.

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

Standalone pages install a storage-backed shell substitute so the same
entry code runs unhosted; the trait keeps exactly these two implementations
(plus the recorder the host tests use).

## Artifact sizes

Released from the build contract's CI log (Deep CI #186 on this branch's tip,
2026-09-25 — `tools/check-runtime-artifacts.mjs` prints every artifact's size):

| artifact | bytes | what it says |
| --- | --- | --- |
| `library.js` + `library_bg.wasm` | 62,013 / 1,854,894 | the dependency split's library artifact: it has no `pdf-engine` execution half — every page render it ever asks for crosses the boundary |
| `reader.js` + `reader_bg.wasm` | 83,702 / 2,213,394 | the reader artifact: `pdf-engine`, `pdf-core`, `reflow-core`, `virtual-list`, `md-core`, `txt-core` are all compiled in, as the dependency gate asserts |

These run lower than any same-page baseline by carving the two runtimes'
distinct dependency closures apart; the gate
(`tools/check-dependency-gate.mjs`, `cargo tree` over the wasm32 target
graph) is what promises those closures never reconverge again.

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
