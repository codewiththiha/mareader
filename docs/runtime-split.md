# Runtime artifacts and route lifetimes

The app ships five independently loadable WASM artifact types. Served files
and compiled-module caches may be shared; each iframe instantiates its own
WASM linear memory and JS realm. Removing a view inside a persistent WASM
instance is not equivalent to releasing that instance.

| Runtime | Package / bin | Entry page | Glue + WASM |
| --- | --- | --- | --- |
| Persistent Shell | `mareader` | `index.html` | `mareader.js`, `mareader_bg.wasm` |
| Library | `library-runtime` / `library` | `library.html` | `library.js`, `library_bg.wasm` |
| Disposable Reader host | `reader-runtime` / `reader`, no default features | `reader.html` | `reader.js`, `reader_bg.wasm` |
| PDF document pane | `reader-runtime` / `pdf`, `pdf,engine` | `pdf.html` | `pdf.js`, `pdf_bg.wasm` |
| Markdown/TXT pane | `reader-runtime` / `reflow`, `reflow,engine` | `reflow.html` | `reflow.js`, `reflow_bg.wasm` |

```text
Shell: navigation, persistence/settings authority, lifecycle, raster budget
├─ Library iframe while the Library route is active
└─ Reader-host iframe while reading
   ├─ workspace chrome, sidebar, menus, settings, layout and focus
   ├─ PDF pane iframe → its own WASM, pdf.js and document worker
   ├─ reflow pane iframe → its own WASM, no PDF engine
   └─ other independently owned document panes (up to four)
```

The Root Cargo graph links neither runtime crate. Library and Reader chrome
are not mounted into the Shell's document. The Shell never reloads for
normal route changes, document replacements or pane closes.

## Route policy

**Reader is allocated only for an actual document open.** Pointer movement,
focus and presses on the shelf do not prewarm it. Every Library return
retires the Reader host, regardless of heap size or pane count. There is
no Reader recycling, empty Reader retention, intent hint or idle-eviction
window. The next open gets a fresh Reader-host realm and independent pane
realms, even when its served artifact files are already cached.

**Library is equally disposable.** Once Reader has painted, the outgoing
Library session unmounts and its iframe/WASM is removed. Returning always
creates a fresh Library frame/generation, seeded from durable settings,
books, covers and reading progress. Neither route has a warm slot, recycle
delay or `Rearm` command. Only the small Shell persists between routes.

**Route documents are DOM boundaries as well as memory boundaries.** Both
route views once carried a `toolbar-row` id, and a lookup by id answers in
document order: `app_chrome::floating::position::place_at_anchor` and the
titlebar's measurements call `document.get_element_by_id`, so a hidden
Library toolbar could satisfy a Reader menu's containment check and place its
popover wrong. A chrome id is unique inside the document that renders it, and
an anchor is never resolved across documents.

Library-owned cover work is cancelled at retirement: queued requests are
pruned, the in-flight bake/loading task/render is aborted, its offscreen
canvas is zeroed, and the JS-only bake page/listener/timers are removed.
A late answer cannot mutate the replacement Library. Native import jobs
may finish independently, but the old Library retains no listener/task UI.

## Pixel-preserving handoff

The Shell owns `#runtime-host`. Its actual iframe elements carry
`data-mareader-slot`:

| Slot | Visible | Lifetime |
| --- | --- | --- |
| `active` | yes | exactly one route on screen |
| `incoming` | no | a requested cold boot, awaiting Ready **and** Painted |
| `retiring` | no | displaced route, awaiting graceful disposal |

A fresh route boot leaves the outgoing frame on screen; it does not remove the
shelf and put a blank incoming iframe in its place. The incoming realm is
laid out under `visibility: hidden`, never `display: none`. `Ready` is a
mount verdict; `Painted` is the runtime's two-rAF paint opportunity. A
missing paint is a named failure, not a synthetic successful reveal. The
new active stamp and outgoing hiding happen in one synchronous handoff.
Per-document handoffs additionally await actual document paint as described
in [pane-runtimes.md](pane-runtimes.md).

The route bounds are 20 seconds for contact/Ready, 2.5 seconds from Ready to
Painted, and 8 seconds for graceful route disposal. Failures name runtime,
stage and cause. Initial boot has the Shell loading card and page placeholder;
subsequent cold handoffs keep outgoing pixels instead of covering them.
Newest navigation wins. Cancelling an incoming boot removes its iframe and
wakes every pending gate; a late response cannot reveal either cancelled
route behind its successor. Hidden incoming Library defers startup writes
until `Refresh` after visible paint; hidden means incoming, not prewarmed.

Incoming status/digest reports are held as one latest plain-data value in
the driver and replayed after active/launch authority is published. This
prevents a fast first Ready report from being lost before the paint gate.

## Authenticated transport

The frame URL contains `?hosted=1&g=<generation>&n=<nonce>`. The Shell
re-offers a `MessageChannel` until contact. The artifact accepts an offer
only from its actual same-origin parent with matching nonce/generation.
An invalid claimed hosted marker never falls back to a standalone app.
Every subsequent envelope carries its generation; stale messages cannot
mutate the active route. Library/Reader entry bins use this hosted bootstrap;
standalone entry pages are available for development. A documentless
standalone Reader host does not preboot a PDF/reflow iframe; its first real
open creates the document realm.

Persistence and settings authority stay in the Shell. Runtime commands and
writes use `ShellApi` (`runtime-contract`) over `PortShellApi`
(`frame-transport`). Pane requests are relayed through their Reader host;
the existing pane protocol, independent mirrors and per-pane liveness
checks remain unchanged. The same-origin Tauri relay runs before each
artifact and delegates native calls through the host to the main window.

## Whole-Reader disposal

1. Before navigation, every pane prepares to leave: flush its read point
   and cancel page work while its state is alive.
2. After Library is painted/revealed, the Shell asks the outgoing Reader
   host to `Dispose`. Its `PaneManager::dispose_all` drains every live,
   incoming and retiring pane realm. Each document explicitly cancels
   queues/search/prefetch/virtualizers, zeros canvas backing stores,
   terminates workers and releases listeners, observers and timers.
3. The Reader host waits for pane tails before unmounting its reactive root
   and emitting its terminal digest and `DisposeComplete`.
4. The Shell closes ports/listeners/timers, removes the **Reader-host
   iframe**, and reclaims that host's whole raster scope. Its WASM memory,
   module instances and descendant browsing contexts are outside every live
   application realm. A strict timeout performs forced removal if
   the graceful acknowledgment never arrives.

There is one window-wide two-slot raster coordinator in the Shell
(`public/rasterLane.ts`). `readerHost.ts` borrows that same object; it does
not allocate a second hosted budget. Owner keys are
`reader:<generation>:<nonce>/<pane nonce>`. Normal pane removal retires only
that pane's keys. Reader-host removal also calls `retireScope` to reclaim
all live/incoming/retiring descendant leases if normal cleanup was cut
short. Queued wakes are weak and jobs re-check liveness after the permit.

The Shell watchdog only handles failed startup; it never fetches Reader or
pane artifacts speculatively. Actual opens pay the route's artifact cost.

## Build and dependency gates

`npm run build:dist` (`tools/build-dist.sh`) runs the Shell and four route/
pane Trunk targets, merges all pages/glue/WASM into `dist/`, then asserts the
artifact contract. Tauri, CI and the dev orchestrator use that same builder.
The shared layout in `tools/runtime-artifacts.mjs` drives canonical builds,
Trunk staging and dev restoration. All four runtime HTML/Trunk-config inputs
are watched. Only the runtime's named page or Trunk's normalized `index.html`
is accepted; arbitrary HTML and malformed dev URLs are not fallback paths.
A bare `trunk build`/`trunk serve` is not a complete app build.
`tools/stage-merged.mjs` and the dev freshness/HTTP probes preserve the full
five-target set across a Shell rebuild.

The dependency gate rejects both runtime crates and document implementation
dependencies in the Shell graph, all Reader dependencies in Library, and
`pdf-engine` in the feature-neutral host and reflow graphs. The artifact
gate rejects `PDFReader` imports in Shell, Library, Reader host and reflow
glue, plus PDF scripts in their pages. Only `pdf.html` loads the PDF engine.
Library cover baking remains in the Shell's short-lived JS-only bake page;
cover work never instantiates Reader WASM and stops when Library retires.

## Observable release, not a RAM promise

At the settled Library baseline, `readerFramesResident == 0`,
`paneFramesResident == 0`, `readerSessionsCreated == readerDisposesCompleted`,
`libraryFramesResident == 1`, and root raster active/queued/owners are zero.
While reading, `libraryFramesResident == 0`, Library created/disposed counts
match, and no Library-owned bake page remains. After either handoff settles,
each route's created minus disposed equals its active count, never a hidden
counterpart.
`atBaseline` fails closed if any realm, lease or terminal evidence remains.
Browser tests cover repeated intent, rapid open/return, fresh host identities,
cancelled incoming boots of both kinds, Library-bake cancellation, fresh
Library JS/WASM identity and mixed per-pane teardown without reloading Shell.
Chromium/WebKit replay asserts zero Reader residency from the +2 second
sample through +70 seconds, with and without pointer intent.

Browser caches, allocators and graphics resources may retain memory after
live instances are gone. This ownership change is not a guarantee of zero
RSS or immediate operating-system reclamation; memory savings require
same-workload measurements. Historical comparisons remain historical, not
claims about this policy.

## Launch queries and native chrome lifetime

The command-only `ShellApi` cannot pretend to answer synchronously over a
port. Hosted path opens await a single transport-owned launch future. Normal
answers, API disposal and abandoned futures remove/wake their requests;
Reader also checks its session/pane liveness after the await. There is no
parallel continuation map or orphaned query issued by a synchronous miss.

Each current titlebar installs native maximize-state synchronization in its
own owner. Native subscriptions become inert on retirement, unlisten before
freeing callbacks, and retain pending registration callbacks until their
native handle can be released. This replaces an undeclared, unwired bridge.
