# Pane runtimes: one frame per pane

Each reader pane runs in its own iframe, with its own JS realm and WASM
instance. Closing a pane removes its frame, and the realm's memory goes with
it: the WASM linear memory, pdf.js and its worker, canvases and every JS
object. Nothing else in the window repaints, so a close or an in-place open
never flickers.

```text
Shell / workspace host (window document, never reloads)
├─ title bar, sidebar, settings, menus, dividers, layout
├─ settings + appearance: one source of truth, pushed to every pane
├─ pane A: <iframe pdf.html>    → pdf.wasm    (own realm, pdf.js, worker)
├─ pane B: <iframe reflow.html> → reflow.wasm (own realm, md/txt + themes)
└─ pane C: <iframe pdf.html>    → pdf.wasm    (own realm, pdf.js, worker)
```

There is no intermediate reader frame: the Shell document itself is the
workspace host. The two pane runtimes are separate binaries, so a reflow
pane never links or loads the PDF engine, and a PDF pane never links the
reflow formats.

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

The vocabulary both sides serialize is `runtime_contract::pane`
(`HostToPane`, `PaneToHost`), JSON over the port, like the Shell protocol.

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

Settings are host-owned. The host pushes the whole blob on every change and
persists it; a pane's own settings write is sent to the host, which applies
and re-broadcasts it.

## Thumbnails

The rail lives in the host, the engine in the pane. A host thumbnail cell
asks its pane for page N. The pane renders into a proxy canvas in its own
document through the unchanged engine thumbnail lane (cache, theme bake,
cancellation), turns it into an `ImageBitmap` and transfers it. The host
draws the bitmap and closes it. Unmounting the cell cancels the request and
the pane zeroes the proxy. An appearance change re-requests the visible
cells.

## Appearance and blend

- Every pane frame paints its own `<html>` from the pushed settings
  (`install_frame_theme`), and its own pane-root look from `PaneAppearance`
  (independent themes), exactly as the in-realm pane did.
- The pane frame wraps its content in `.reader-bg` carrying the workspace
  flags the host decides: `blend`, `independent-themes`, `split-workspace`.
- **Shared paper.** A PDF pane's engine publishes `--pdf-paper` and
  `--pdf-paper-baked` on its own `<html>`. The pane frame watches those two
  and reports them. The host keeps the most recently focused PDF pane as the
  publisher (falling back to the next most recent on close), paints its own
  root and pushes the shared values to every pane, which sets them on its
  `.reader-bg`, overriding its own `<html>` for everything inside.
- **Engine hooks.** The appearance menu's re-bake, scrub and menu-open calls
  are forwarded to the PDF panes (only the scoped pane while a pane-scoped
  slider drags).

## Input across frames

- **Focus.** A press or focus inside a pane frame asks the host for focus.
- **Keyboard.** Keys go to the focused document. The pane frame handles its
  own shortcuts and sends the workspace ones (sidebar, settings, Escape for
  the rail) to the host. Keys pressed while the host document has focus are
  forwarded to the active pane.
- **Grab and lift.** Panning stays in the pane. A hold to lift starts in the
  pane, which then streams pointer positions (in host coordinates) to the
  host until release.
- **Host drags** (dividers, library rows, a lifted pane) raise a transparent
  shield over the panes for the drag, so pointer events stay in the host.

## Rasters across frames

Same-origin frames share one main thread. The realm-wide page-lane cap
(two rasters in flight across every pane) moves to a lane object the host
installs on its window; pane engines acquire and release slots through
`window.parent`. A pane's slots are reclaimed when its frame is removed.

## Lifecycle without flicker

- **Create.** The host creates the iframe hidden (`visibility: hidden`,
  never `display: none`), sends `Boot` (pane id, launch, settings,
  appearance, flags) after the channel handshake, and reveals the frame on
  the pane's first paint.
- **Replace in place.** Opening another document in a pane boots a fresh
  frame behind the current one and swaps them on first paint; the old frame
  is disposed and removed. The pane id, its place and its focus stay.
- **Close.** The host sends `Dispose`; the pane frame flushes its read
  point, runs `DocumentPane::dispose` and its teardown tail, sends its final
  diagnostics digest and `Disposed`, and the host removes the iframe. A
  timeout removes it regardless.
- **Warm pane.** The host keeps one booted, empty pane frame ready, so an
  open from the library is not a frame boot.

## Diagnostics

Each pane frame publishes its snapshot to the host on the digest beat. The
host's digest sums the engine and pane counters over live panes plus the
final snapshots of disposed ones, so `sessionsOpened == sessionsDestroyed`
and the other balances still hold across frames.

## Legacy removed

- The reader frame: the Shell hosts the workspace directly, and nothing
  outside a PDF pane frame loads pdf.js.
- The in-realm pane path: the host no longer mounts `DocumentPane`.
- In-realm presentation recency for the root paper (`presented` in
  `public/engine/state.ts`): a pane realm holds one session, and the host
  owns the recency.
- The rail's direct engine binding (`MountedPdf` in thumbnail cells).

## Stages

1. Contract and pane artifacts: `runtime_contract::pane`, `pdf.html` /
   `pdf.wasm` and `reflow.html` / `reflow.wasm` (each with its own Trunk
   config and feature set), the pane frame boot running `DocumentPane`,
   the build and artifact checks.
2. `FramePane` in the host: iframe, handshake, mirror chrome, commands,
   reveal on paint, dispose; the composition root switches to it.
3. The Shell becomes the workspace host: the workspace mounts in the Shell
   document and the reader frame is retired.
4. Parity: thumbnails, shared paper and engine hooks, keyboard and focus,
   grab and lift, shields, the shared raster lane, the warm pane,
   diagnostics aggregation.
5. Tests and tools follow the frames: the lifecycle suite reaches into pane
   frames, the boundary and artifact checks know the new artifact.
6. Legacy removal and docs.
