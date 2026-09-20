# Workspace runtime ownership

The Tauri frontend is `index.html` → `host/host.ts` → `bootWorkspace()` plus
`apps/workspace-wasm`. It no longer mounts the library or a routed reader in
its own realm. Trunk is an artifact builder, invoked by `build-runtimes.mjs`;
the same builder is used by Tauri dev, CI and production. It builds five apps
in **separate Cargo invocations**, preserving feature boundaries.

## Appearance and UI

The workspace mounts the existing `AppTitleBar`, `ShellController`, `PushRail`,
`OverlayRail`, `SidebarShell`, `SidebarHeader`, `BookInfo`, `ReaderMenu`,
`AppearanceMenu`, and `SettingsModal`. Their styles, traffic-light placement,
pinning, settings sections and appearance-token computation remain shared
with the original UI. The new compact library tree lives in that sidebar;
there is no second reader toolbar or rail inside a frame.

A reader mounts `ReaderSurface`: the existing virtualizers, first-paint gate,
viewer, search, AI/gloss, page indicator and bottom controls. The library page
is unchanged visually and runs in its own disposable frame. The persistent
workspace keeps a value projection of library rows/shelves, not its cover UI,
PDF parser or document content. Persisted folder-generated shelves already
encode the folder hierarchy, so they use the same recursive projection.

Reader snapshots travel as values over MessagePort. Focus originates from a
pointerdown inside the frame. The shared chrome mirrors the focused format,
settings, outline and navigation state, then sends control changes back to
that pane. A pending appearance slider is flushed before changing focus.

Every pane has a base settings profile. Blend is a workspace session whose
shared appearance does not change with focus. Effective settings are sent to
each frame without saving over its base. PDF baked paper is sampled in its
own realm and forwarded as a color, not a canvas or document handle; reflow
pages receive the exact shared paper token. Leaving Blend restores base
appearance. Readers and workspace theme effects never persist an effective
Blend profile to the global settings store.

## Split placement

`host/layout.ts` is the production immutable binary tree and geometry owner.
Both preview and commit call `place`; hover never creates a frame. No unused
Rust model is presented as proof of production wiring. `workspace-wasm` owns
chrome, while the host owns layout DOM and runtime lifetime.

Only sidebar book leaves create an internal session, with the MIME
`application/x-mareader-library-item`, version, library-sidebar source,
cryptographic one-use token and book ID. External files and stale payloads
cannot become split requests. Native OS file opens remain a separate path.
Tauri keeps native file-drop enabled; because WebView2 can intercept HTML DnD,
internal desktop gestures use pointer capture and feed the same MIME/session
validator and overlay. Browser gestures use native HTML DnD.

The overlay is above reader frames. It projects every final rectangle at
left/right/top/bottom/center, with 50/50 new splits. Four leaves is the hard
limit; center replacement remains available. Closing removes the leaf and
collapses its parent without reloading the remaining frames.

## Lifetime and debugging

`PaneRuntimeManager.panes` is the sole reader owner. Close waits for the
reader's DISPOSED acknowledgment after progress flush and engine destruction,
then the port, subscriptions and iframe are released before the model
collapses. A five-second timeout is an explicit error-reported fallback for a
hung/crashed frame, not a claim that destruction was acknowledged. Startup
failure/cancellation also removes the frame and its map entry.

The library frame is disposed before the first reader opens. Closing the last
reader creates a **new** library frame. Its page is the sole library writer
while mounted; reader progress is persisted by the workspace while it is not.
Neither route reuses the other's WebAssembly instance. Browser allocators may
retain RSS after a realm is released; immediate process RSS reduction is not
promised.

Inspect:

```js
__MAREADER_DEBUG__.workspace
__MAREADER_DEBUG__.panes.size
[...__MAREADER_DEBUG__.panes.keys()]
__MAREADER_DEBUG__.activePane
__MAREADER_DEBUG__.tree
__MAREADER_DEBUG__.library
```

`tools/verify-legacy-runtime.mjs` prevents the old boot path returning.
`tests/runtime/browser.mjs` runs on GitHub against all five built artifacts,
including real PDF workers, mixed-format splits, complete hover geometry,
frame counts, replacement, real iframe focus and close/collapse. Its only mock
is the native filesystem/window transport. Native macOS/WebView2 appearance
and OS-drop behavior still require desktop acceptance; Chromium is not proof
of identical platform compositor behavior.
