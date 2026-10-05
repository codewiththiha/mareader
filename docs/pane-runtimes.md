# Pane runtimes: one frame per pane

Each reader pane runs in its own iframe, with its own JS realm and WASM
instance. Closing a pane removes its frame, and the realm's memory goes with
it: the WASM linear memory, pdf.js and its worker, canvases and every JS
object. The Shell and unrelated document realms stay alive; replacements
retain the old surface until the new document has actually painted.

```text
Persistent Shell (never reloads; navigation/settings/lifecycle/raster authority)
├─ Library: <iframe library.html> → library.wasm (Library route only)
└─ Reader host: <iframe reader.html> → reader.wasm (disposable on Library return)
   ├─ title bar, sidebar, settings, menus, dividers, layout and chrome mirrors
   ├─ pane A: <iframe pdf.html>    → pdf.wasm    (own pdf.js + worker)
   ├─ pane B: <iframe reflow.html> → reflow.wasm (MD/TXT; no PDF engine)
   └─ pane C: <iframe pdf.html>    → pdf.wasm    (own pdf.js + worker)
```

There are **five artifact types**; reader workspace chrome belongs to its
own disposable host, not the persistent Shell document. Every Library return
unloads that host and all live/incoming/retiring document realms. Shelf
pointer/focus/presses never prewarm Reader or retain an empty reflow realm
behind Library. Entering Reader also removes Library; neither route is
retained/prewarmed behind the other, and both return with fresh instances.

The Reader host builds with `--no-default-features` and no feature selection;
the pane artifacts select `pdf,engine` for `pdf.html`, `reflow,engine` for
`reflow.html`. `pdf-engine` is optional and only the PDF feature enables it.
Shared status/page metadata lives in `reader-core::document`; engine report
data lives in `pdf-core::diagnostics`, not the browser engine. Dependency
and artifact gates keep Reader code out of Shell and PDF execution out of
Library, the Reader host and reflow glue/pages. A PDF pane does not link the
reflow rendering components.

## The seam

The host already speaks to a pane only through `PaneRuntime`
(`crates/reader-runtime/src/host/contract.rs`) and never names a format
type. The split keeps that seam and changes what stands behind it:

- **Host side: `FramePane`** (`crates/reader-runtime/src/frame_pane/`)
  implements `PaneRuntime`. It owns the iframe, the `MessageChannel` and a
  *mirror* of the pane's chrome-facing state.
- **Pane side: the pane frames** (`crates/reader-runtime/src/pane_frame/`,
  bins `pdf` and `reflow`) run the existing `DocumentPane` unchanged, with a `PaneEnv`
  built from the port instead of from the host.

The vocabulary is `reader_runtime::pane_wire` (`HostToPane`, `PaneToHost`),
JSON over the port. Shell API requests use the existing runtime-contract
envelope inside that vocabulary. The initial port offer is accepted only
from the actual parent and matching nonce; disposal is one-shot.

## Chrome: a mirror, not a remote view

The title, view menu, rail and settings modal are rendered by the host with
the same components as before, against a mirror `ReaderContext` the
`FramePane` owns. The state those components read is narrow:

| Pane → host (mirror) | Host → pane (writes) |
| --- | --- |
| document status, error, path, book id, title, author, format, page count, outline, outline pending | view mode, fit mode |
| page, view mode, fit mode, zoom display, search visible | page (outline and thumbnail jumps) |
| launch (name, cover, path) | zoom commands (the menu's steps) |
| | search visible |

A write is forwarded only when the mirror value differs from the last value
the pane reported, so a value arriving from the pane never echoes back.

Settings authority belongs to the persistent Shell. The Reader host mirrors
its canonical snapshot and pushes the whole blob to panes on every change.
A pane settings write is relayed through its host to Shell, which persists
and re-broadcasts it; Reader-host disposal cannot discard settings.

## Thumbnails

The rail lives in the host, the engine in the pane. A host thumbnail cell
asks its pane for page N. The pane renders into a proxy canvas in its own
document through the unchanged engine thumbnail lane (cache, theme bake,
cancellation), turns it into an `ImageBitmap` and transfers it. The host
draws the bitmap and closes it. Unmounting the cell cancels the request and
the pane zeroes the proxy.

The picture the rail holds is the HOST's copy of a raster, so a look change
can only reach it through the host: the pane sends `ThumbsStale` whenever it
re-bakes its look — after the new tokens are painted, and again when a scrub
window ends, because the window itself bakes nothing — and the host
re-renders every settled cell. The frame answers with a bake from its own
raw raster: a cached display is judged against the pipeline as it reads NOW,
never against the generation left behind by the last read, so a cell asking
during a change cannot be served the look before it. Only the cards the rail
can SEE are re-baked for the change; the rest of the cache stays one
generation behind and re-bakes from raw when its cell next asks, which is the
same lazy path a cold card takes.

A row the rail's window leaves behind keeps its canvases for
`BRIDGE_FRAMES` animation frames, up to `BRIDGE_CELLS` cells at a time (the
rail's two columns, so three rows), so a glide that turns around finds the row
it passed still mounted instead of paying a post, a raster and a bitmap
transfer per card.

## Appearance and blend

- Every pane frame paints its own `<html>` from the pushed settings
  (`install_frame_theme`), and its own pane-root look from `PaneAppearance`
  (independent themes), exactly as the in-realm pane did.
- The pane frame wraps its content in `.reader-bg` carrying the workspace
  flags the host decides: `blend`, `independent-themes`, `split-workspace`.
  The host's answer follows the mode IN EFFECT, so a lone pane mirrors no
  `independent-themes` and shows the window theme its colour was promoted
  into as the split collapsed.
- **Shared paper.** A PDF pane's engine publishes `--pdf-paper` and
  `--pdf-paper-baked` on its own `<html>`. The pane frame watches those two
  and reports them. The host keeps the most recently focused PDF pane as the
  publisher (falling back to the next most recent on close), paints its own
  root and pushes the shared values to every pane, which sets them on its
  `.reader-bg`, overriding its own `<html>` for everything inside. Closing
  the last PDF sends `Paper { paper: None }` too, so a surviving text pane cannot keep
  a stale PDF backdrop.
- **Engine hooks.** The appearance menu's re-bake, scrub and menu-open calls
  are forwarded by the host, which reads `data-appearance-scope` in its own
  document and selects the recipient realm. The scope id is not interpreted
  against unrelated iframe ancestors. Independent menu retention targets
  the focused pane; end messages visit all panes so focus changes cannot
  strand a raw-retention/scrub window. Fresh frames inherit these flags.

## Input across frames

- **Focus.** A press or focus inside a pane frame asks the host for focus.
- **Keyboard.** Keys go to the focused document. The pane frame handles its
  own shortcuts and sends the workspace ones (sidebar, settings, Escape for
  the rail) to the host. Keys pressed while the host document has focus are
  forwarded to the active pane.
- **Grab and lift.** Panning stays in the pane. A hold to lift starts in the
  pane, which then streams pointer positions (in host coordinates) to the
  host until release. The still hold is one second; the progress ring takes
  its duration from `HOLD_TO_LIFT_MS`. Listener removal, pointer
  capture release, hold cancellation and fling-loop stop are owner cleanup.
- **Host drags** (dividers, library rows, a lifted pane) raise a transparent
  shield over the panes for the drag, so pointer events stay in the host.

## Rasters across frames

Same-origin frames share one main thread. `public/rasterLane.ts` installs
one two-slot full-page budget in the persistent Shell window. The Reader
host borrows that exact coordinator rather than constructing another one. Its FIFO holds **weak**
wake callbacks and plain owner/lease keys only. Each engine retains its
pending wakes in its own session (at most two per realm), cancels them on
page/session teardown, and checks liveness/generation again after a permit
arrives. The existing per-session/realm caps, fling gate and quiescent
sweeps remain. Normal pane removal reclaims only that nonce's scoped leases. Forced
Reader-host removal reclaims all descendant owners in its generation/nonce
scope, including incoming and retiring realms. `rasterLane` diagnostics report active, queued, owners
and peak; the disposal baseline requires active/queued/owners and all Reader/pane
iframe residency to be zero.

## Lifecycle without flicker

- **Create.** The host creates the iframe hidden (`visibility: hidden`,
  never `display: none`), sends `Boot` (pane id, launch, settings,
  appearance, flags) after the channel handshake, and reveals the frame on
  the pane's first paint.
- **Replace in place.** Every replacement boots a fresh frame, including
  PDF → PDF and Markdown → TXT. No empty realm is prebooted, and neither
  same-format nor unadopted predecessors are reused for a new open. The current
  surface stays until `Painted` plus Ready/actual first paint (or an error);
  no PDF mount/900 ms timer counts as raster paint. The old frame
  is disposed and removed. The pane id, its place and its focus stay, and
  until the swap the pane's chrome shows the open, not the old frame.
- **Boot facts.** `Boot` is drafted when the frame is created and refreshed
  with the host's current settings, focus, layout and sidebar when the
  frame says hello, so nothing said in between is lost.
- **Leave.** Returning to the library first asks every pane to prepare:
  it writes its read point, cancels its page renders and stands its
  thumbnail prefetches down.
- **Close.** The host sends `Dispose`; the pane frame flushes its read
  point, runs `DocumentPane::dispose` and its teardown tail, sends its final
  diagnostics digest and `Disposed`, and the host removes the iframe. A
  1.5 s timeout removes **that nonce only**, never newer retiring frames.
  Startup has owned/cancelled 10 s hello and 30 s paint deadlines; an
  incomplete boot produces a named pane error, not an endless hidden frame.
- **First document.** A pane realm boots with its real launch. An empty
  unhosted development host owns chrome/mirror state only; its first open
  creates exactly one document iframe without prefetching an empty runtime.
- **Final word.** On `Dispose` a pane realm waits briefly for the engine
  work the dispose cancelled to settle, then sends its final digest and
  `Disposed`; the host reads the realm's probe once more before it removes
  the frame.
- **Document epoch.** The host claims the document-session epoch for its
  panes (each open, each close of a held document), so the count does not
  restart when a document kind swap boots a new realm. A fresh Reader host
  starts from zero; Shell's Reader generation/count stays authoritative.
- **Status.** The document status the Shell hears is the host's, derived
  from the mirrors. A pane realm's own status report is not forwarded, and
  only the live frame of a pane may act for the user (open, return to the
  library).

## Legacy removed

- In-document Library/Reader adoption: Shell links neither implementation;
  its route iframes are real disposable realms, with independent document
  children. Nothing outside a PDF pane (or the JS-only cover baker) loads pdf.js.
- The in-realm pane path: the host no longer mounts `DocumentPane`, and a
  pane realm draws no host chrome (the pane contract's `chrome` default is
  empty — only the frame pane supplies a slot).
- In-realm presentation recency for the root paper: the host owns which
  pane's paper colours the root, and a pane realm holds one session. The
  engine bundle keeps `presented` (`public/engine/state.ts`) for the
  sessions that share one engine, not to arbitrate between realms.
- The rail's direct engine binding: thumbnail cells read engine output
  through the pane, never by binding the canvas's own `MountedPdf`
  (`crates/reader-runtime/src/pane/engine.rs`).

## Regression evidence

`tests/browser/pane-runtimes.mjs` runs from the unchanged Deep CI lifecycle
job. It covers same-format/cross-format replacement, rapid supersession,
text realms with no PDF global/imported resources, two real PDF engines
sharing the raster cap, scoped scrub, five-second lift with vacancy fill,
layout/session preservation and final balanced teardown. It saves desktop
(1400 px) and narrow (640 px) screenshots plus a SHA-tagged report under
`dist/verification/`, inside the existing `dist` artifact. CI compilation and
these browser runs, not local build/dependency installations, validate the
change. Native traffic-light appearance still needs a real macOS visual
check; native smoke and chrome-contract checks do not prove shape/blink.

<!-- // only the changed file was rewritten -->
