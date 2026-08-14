# PDF Reader

A desktop PDF reader built for long-form reading. Native Tauri v2 shell, Rust/WebAssembly
interface written in Leptos, and Mozilla's pdf.js vendored locally as the rendering engine.

The design goal is eye comfort over hours of reading: instead of a fixed set of themes, the
appearance system exposes a continuous colour space (base mode plus a computed tint) layered with
optional paper textures and film grain, all persisted between sessions.

---

## Table of contents

- [Highlights](#highlights)
- [Features](#features)
  - [Document viewing](#document-viewing)
  - [Zoom and fit](#zoom-and-fit)
  - [Navigation](#navigation)
  - [Search](#search)
  - [Appearance system](#appearance-system)
  - [Presets](#presets)
  - [Opening documents](#opening-documents)
  - [Interface](#interface)
  - [Persistence](#persistence)
  - [Accessibility and motion](#accessibility-and-motion)
  - [Performance and memory](#performance-and-memory)
- [Keyboard shortcuts](#keyboard-shortcuts)
- [Architecture](#architecture)
  - [Layer overview](#layer-overview)
  - [Project layout](#project-layout)
  - [Engine API](#engine-api)
  - [State model](#state-model)
- [Getting started](#getting-started)
  - [Prerequisites](#prerequisites)
  - [Installation](#installation)
  - [Development](#development)
  - [Building for release](#building-for-release)
  - [Tests](#tests)
- [Configuration](#configuration)
- [Technology stack](#technology-stack)
- [License](#license)

---

## Highlights

- Two reading modes: single page and virtualized continuous scroll.
- Continuous appearance model with three base modes plus a hue and strength tint, replacing
  hard-coded themes.
- Five paper textures with adjustable opacity and scale, plus static or animated film grain.
- Eight built-in presets and unlimited user-saved presets organised into named groups.
- Full-text search with per-page results, highlight rectangles and wrap-around navigation.
- Document outline (table of contents) with automatic highlighting of the active section.
- Virtualized thumbnail grid backed by an LRU bitmap cache.
- Clickable internal links that navigate the reader, and external links that open in the browser.
- Native file dialog, drag-and-drop opening, and restoration of the last-opened document.
- Settings persisted to local storage with a migration path across schema changes.
- 85 unit tests covering the pure logic layer.

---

## Features

### Document viewing

**Single page view.** One page at a time, centred in the viewport, with a page-turn animation
whose direction follows the navigation direction.

**Continuous scroll view.** A vertically scrolling column of pages with a reading-progress bar.
The list is virtualized: only pages near the viewport are mounted, so a large document does not
allocate a canvas per page. Page height bookkeeping is computed from the intrinsic page sizes
returned when the document opens, which keeps the scrollbar honest even for pages that have never
been rendered.

**Mixed page sizes.** Fit calculations read the size of the page currently on screen rather than
assuming every page matches page one, so a landscape plate inside an otherwise portrait book is
not cropped.

**Selectable text.** A pdf.js text layer is overlaid on each page canvas. The layer is built
detached and swapped in atomically once complete, so a superseded render cannot append overlapping
spans. Selection paints a translucent tint over transparent text, so the canvas glyphs read
through and text is never doubled.

### Zoom and fit

- Fifteen zoom presets from 25 percent to 500 percent, clamped to that range.
- Zoom in and out step through the presets; repeated presses chain from the in-flight target, so
  a fast double press advances two steps rather than being swallowed.
- Fit width and fit page, recomputed on window resize and sidebar toggle.
- Shrink to fit: when the window is too narrow for the chosen zoom, the page shrinks to the space
  available and remembers the zoom to grow back to once space returns.
- Zoom is an anchored layout animation. The existing bitmap is stretched to the target size
  immediately and one crisp render is issued at the end, rather than re-rendering every frame.
- Fit recomputation is debounced so scrolling a mixed-size book does not trigger a zoom on every
  row.

### Navigation

- Page counter with direct page entry.
- Previous and next page controls.
- **Outline panel.** The document's table of contents, flattened in document order with depth
  preserved. The entry containing the current page is highlighted; where several entries share a
  page, the deeper one wins. Pages before the first entry highlight nothing, since a cover belongs
  to no section.
- **Thumbnails panel.** A virtualized grid that tracks the reader's current page and auto-centres
  on it, with the glide debounced so a rapid scroll does not chase every intermediate page.
- **Internal links.** Link annotations inside the document dispatch a navigation event that moves
  the reader to the target page through the same path the outline and thumbnails use.
- **External links.** Rendered as ordinary anchors and opened by the system browser. The
  `javascript:` and `file:` schemes are refused.

### Search

- Full-text index built across the document on demand.
- Results grouped by page with a surrounding text snippet for context.
- Match rectangles are returned in scale-independent coordinates and multiplied by the current
  scale, so highlights stay aligned at any zoom.
- Forward and backward navigation through results with wrap-around.
- A floating search overlay with a results dropdown, dismissible with Escape.

### Appearance system

Rather than a fixed theme list, appearance is composed from independent axes.

**Base mode** determines the structural family:

| Mode  | Canvas treatment | Texture blend | Intended use |
|-------|------------------|---------------|--------------|
| Light | Untouched        | Multiply      | Daylight reading |
| Dark  | Inverted         | Screen        | Dark rooms, text documents |
| Dim   | Dimmed, not inverted | Soft light | Preserves real colours in figures, photos and syntax highlighting |

**Colour tint** is a hue from 0 to 360 degrees and a strength from 0 to 100, applied by the same
maths regardless of base. A strength of zero short-circuits the colour pipeline entirely, so a
plain base is byte-identical to having no tint feature at all.

Tint hue is specified in sRGB, because the page tint is applied with a hue rotation that operates
in sRGB, but the interface tokens are emitted in OKLCH. The two hue circles are rotated relative
to one another by a non-constant amount, so the value is converted rather than reused; this is
what keeps the page and the surrounding interface in the same colour family at every setting.

**Textures** overlay the page with a repeating pattern: none, paper, lined, grid, dotted or cross.
Opacity is adjustable from 0 to 100 and scale from 25 to 400 percent of the natural pitch.
Textures are anchored to the page rather than the viewport, so they track the page during zoom
instead of sliding across it.

**Film grain** can be off, static or animated, with intensity from 0 to 100.

### Presets

Eight presets ship with the application, in two groups:

- **Basic:** Light, Dark, Dim, each with no tint.
- **Classic:** Sepia, Green, Night, Parchment, Cinema, each a specific point in the appearance
  space.

Sepia, Green and Night were previously hard-coded themes and are now reproduced exactly as tints,
which is what allows the appearance model to be the only mechanism.

Users can save the current appearance as a named preset and organise presets into named groups.
As soon as any slider is nudged, the active preset selection clears and the menu reports "Custom"
rather than claiming an unmodified preset is in effect.

### Opening documents

- Native file dialog through the Tauri dialog plugin.
- Drag and drop, handled through the Tauri drag-drop event with a DOM fallback.
- The last opened document is remembered and restored on the next launch.
- Six sample PDFs ship in `public/samples`, covering deep outlines, internal links and awkward
  title metadata.

**Filename display.** A PDF's `/Title` metadata is free-form and frequently contains junk written
decades ago by the producing tool, such as `file:///F|/Mis%20docum` from dvips or
`Microsoft Word - Chapter3.doc` from Word. The title is used only when it actually looks like a
title; anything path-shaped, URL-shaped, percent-encoded, extension-bearing or a known placeholder
is rejected in favour of the human-readable stem of the real file path. Path parsing splits on both
separators, so a Windows path reaching a macOS or Linux build is not treated as one long filename.

The toolbar title measures the live geometry of the surrounding controls and truncates only on a
genuine collision, adapting as the window resizes. Only widths are measured, never positions, so
there is no feedback loop between the label and the layout.

### Interface

- Glass toolbar with sidebar toggle, open button, document title, centred page navigation, zoom
  popover, view-mode switch, search and an overflow menu.
- Overflow menu with fullscreen, print and a keyboard shortcut reference.
- Animated sidebar with an outline and thumbnails rail. Panels stay mounted while the sidebar is
  collapsed, so thumbnails survive a toggle without re-rendering.
- Status bar showing the current page position, rendered as a click-through overlay.
- Toast notifications for errors, auto-dismissed after about 3.5 seconds.
- Tooltips on the icon controls.
- A placeholder view with drag-and-drop affordance when no document is open.

### Persistence

Settings are stored in local storage under `pdfreader.settings.v1` and cover appearance, the
active preset, user presets, default zoom and the last opened path.

The appearance model changed shape during development, from six fixed themes to base mode plus
computed tint plus presets, but the storage key was deliberately not bumped. A new key would have
silently reset every reader's last-opened file and zoom as well. Instead the retired fields are
retained as optional values, migrated on load to the preset that reproduces the theme previously
in use, and dropped when writing, so the migration runs at most once.

Writes are debounced by 350 milliseconds so dragging a slider does not hammer local storage.

### Accessibility and motion

- `prefers-reduced-motion` is honoured: page-turn animations, the animated grain and general
  transitions are disabled or reduced to a negligible duration.
- Interactive controls carry `aria-label`, `aria-pressed`, `aria-current` and `aria-disabled`
  as appropriate, and icon-only segments carry a title so they are labelled for screen readers
  and on hover.
- Decorative icons are marked `aria-hidden`.
- Keyboard shortcuts do not fire while focus is inside a form control, and chrome scrollers such
  as the thumbnail rail, the sidebar and popovers keep their own arrow-key behaviour.

### Performance and memory

- Release builds optimise for size with link-time optimisation and a single codegen unit, and the
  WebAssembly output is passed through `wasm-opt`.
- Thumbnails are cached as bitmaps in an LRU of 64 entries, roughly six screens of grid. A cache
  hit blits synchronously with no skeleton, no pulse and no transition, so a remounted row has
  nothing left to flicker.
- New thumbnail renders draw into a detached canvas, so a live canvas is never shown mid-render.
- Canvas backing stores are explicitly released on unregister, which avoids a WKWebView leak where
  memory grew with every page scrolled.
- Colour slider changes repaint CSS at most once per frame and commit to settings 180 milliseconds
  after the last tick, so dragging does not reallocate the page filter continuously.
- Resize observers are disconnected on cleanup. Without this, a resize queued during teardown
  could invoke a dropped closure and abort the WebAssembly runtime.
- Device pixel ratio is respected when sizing canvases, so pages stay crisp on high-density
  displays.

---

## Keyboard shortcuts

| Action | Shortcut |
|--------|----------|
| Open document | `Cmd/Ctrl` + `O` |
| Search | `Cmd/Ctrl` + `F` |
| Fit width | `Cmd/Ctrl` + `0` |
| Single page view | `Cmd/Ctrl` + `1` |
| Continuous view | `Cmd/Ctrl` + `2` |
| Zoom in | `+` or `=` |
| Zoom out | `-` or `_` |
| Previous page | `Left arrow` |
| Next page | `Right arrow` |
| Scroll up / down (continuous) | `Up arrow` / `Down arrow` |
| Turn page (single) | `Up arrow` / `Down arrow` |
| Screen up / down | `Page Up` / `Page Down` |
| Screen down / up | `Space` / `Shift` + `Space` |
| Dismiss overlay or search | `Escape` |

In continuous mode the reader owns the arrow keys and scrolls the page list directly. Leaving them
to the browser meant scrolling whatever held focus, which was usually a text-layer span; when
virtualization unmounted that page the focused node disappeared, key repeat died, and the next
press landed on the document body. The page list is focusable and reclaims focus when a descendant
is removed, so hold-to-repeat stays aimed at a node that outlives any single page.

A held arrow glides continuously at roughly 1000 pixels per second after a 350 millisecond delay,
rather than stepping discretely, so browser key repeat cannot chunk the motion. A single tap moves
8 percent of the viewport, clamped between 40 and 80 pixels, which matches native line scrolling.
Page-level keys move 90 percent of a screen, leaving a sliver of overlap so the last line read
stays visible.

---

## Architecture

### Layer overview

```
+-------------------------------------------------------------+
|  Tauri v2 shell (Rust)                                       |
|  native window, file dialog, asset protocol, fullscreen      |
+-------------------------------------------------------------+
|  Leptos 0.7 interface (Rust, compiled to WebAssembly)        |
|  atoms / molecules / organisms / views, reactive signals     |
+-------------------------------------------------------------+
|  Typed bridge (wasm-bindgen)                                 |
|  snake_case Rust mapped to camelCase engine calls            |
+-------------------------------------------------------------+
|  window.PDFReader engine (JavaScript, public/pdfEngine.js)   |
|  render queue, thumbnail cache, text layer, search index     |
+-------------------------------------------------------------+
|  pdf.js 6.2.108, vendored into public/vendor/pdfjs           |
+-------------------------------------------------------------+
```

Components follow atomic design. Pure logic lives in `src/core` with no WebAssembly dependencies,
so the zoom maths, layout maths, filename rules, colour conversion, search index arithmetic and
settings migration are all unit-testable on the host.

### Project layout

```
src/
  api/engine.rs         typed async wrappers over the engine bridge
  core/                 pure logic, no wasm dependencies
    appearance.rs       base mode, tint, texture, noise, and the CSS maths
    bridge.rs           wasm-bindgen extern declarations
    document.rs         document and outline types
    filename.rs         display-name derivation rules
    layout.rs           page offsets, column height, view mode
    math.rs             zoom presets, clamping, fit calculations
    oklch.rs            sRGB to OKLCH conversion
    open_flow.rs        the open-document sequence
    presets.rs          built-in and user presets
    search.rs           search result types and index arithmetic
    settings.rs         persisted schema and migration
    state.rs            the reactive application state tree
  components/
    atoms/              button, icon, slider, segmented, tooltip, kbd, hue picker
    molecules/          toolbar, page nav, zoom controls, appearance and more menus
    organisms/          sidebar, outline, thumbnails, search, toast, status bar
    views/              single page, continuous, reader shell
  effects/              cross-cutting reactive systems
    continuous_scroll.rs, fit.rs, link_nav.rs, page_tracking.rs,
    search_effects.rs, shortcuts.rs, theme_applier.rs
  util/                 DOM helpers and storage access
public/
  pdfEngine.js          the imperative engine wrapper
  vendor/pdfjs/         vendored pdf.js build, worker, viewer CSS and cmaps
  samples/              sample documents
src-tauri/              native shell, capabilities, icons, bundle configuration
styles/input.css        Tailwind v4 entry point and the full design system
```

### Engine API

`window.PDFReader` is the single boundary between Rust and pdf.js. Every function resolves and
never rejects: success is `{ok: true, ...}` and failure is `{ok: false, error: {name, message}}`,
so the Rust side reads `ok` first and then deserializes.

| Function | Purpose |
|----------|---------|
| `version` | Engine version string |
| `open` | Load a document, return page count and intrinsic page sizes |
| `destroy` | Tear down the current document |
| `pageCount` | Page count as the engine sees it |
| `registerPage` / `unregisterPage` | Bind and release a canvas for a page |
| `cancelPage` | Cancel an in-flight page render |
| `renderPage` / `renderPages` | Render one page or a batch |
| `renderThumb` / `cancelThumb` | Thumbnail rendering on a separate, cheaper path |
| `hasThumb` / `blitThumb` | Probe the bitmap cache and blit a cached frame |
| `updatePage` | Re-render an existing page at a new scale |
| `buildSearchIndex` / `search` / `clearHighlights` | Full-text search lifecycle |
| `stats` | Internal counters, used to assert memory is actually released |
| `storageGet` / `storageSet` | Local storage access |

Load order in `index.html` is deliberate. pdf.js is ESM-only in version 6 and must execute before
the engine so `globalThis.pdfjsLib` exists, and the engine must execute before the WebAssembly
module because the application reads persisted settings synchronously during mount.

### State model

A single `AppState` tree of Leptos signals is threaded through the component tree, divided into
document state, viewer state, search state and settings. Effects subscribe to it rather than
components talking to one another, which keeps ownership of each concern in exactly one place.
During a zoom, for example, a single system owns the display scale and render scale, and every
zoom control posts a request to it rather than writing the scale directly.

---

## Getting started

### Prerequisites

- **Rust** with the `wasm32-unknown-unknown` target
- **Node.js** 20 or newer, required by the Tailwind v4 CLI
- **Trunk**, the WebAssembly bundler
- **Tauri v2 system dependencies** for your platform, listed at
  <https://tauri.app/start/prerequisites/>

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk
```

### Installation

```bash
git clone https://github.com/codewiththiha/pdf-reader.git
cd pdf-reader
npm install
```

### Development

Run the desktop application with hot reload:

```bash
cargo tauri dev
```

This starts Trunk on port 1420 and opens the native window. Tailwind is compiled by a Trunk
pre-build hook, so stylesheet changes rebuild automatically. To run the interface in a plain
browser instead, without the native shell:

```bash
trunk serve
```

Note that the file dialog and drag-and-drop rely on Tauri and are unavailable in a browser.

### Building for release

```bash
cargo tauri build
```

Bundles are produced for the host platform in `src-tauri/target/release/bundle`. The bundle
configuration targets all available formats and ships icons for macOS, Windows and Linux.

### Tests

```bash
cargo test
```

85 unit tests cover the pure layer: zoom and fit maths, page layout, filename derivation, colour
conversion, appearance CSS generation, presets, settings migration, search index arithmetic,
outline activation and thumbnail geometry.

---

## Configuration

| File | Purpose |
|------|---------|
| `Trunk.toml` | Build target, the Tailwind pre-build hook, watch exclusions, dev server port |
| `src-tauri/tauri.conf.json` | Window size and title, bundle targets, asset protocol scope |
| `src-tauri/capabilities/default.json` | Permission grants for the main window |
| `styles/input.css` | Tailwind v4 entry point and the complete design system |
| `.taurignore` | Paths excluded from the Tauri file watcher |

The asset protocol is scoped to the home, desktop and documents directories, which is what allows
the engine to read a chosen file while keeping the rest of the filesystem out of reach.

The default window is 1200 by 800, with a floor of 640 by 480.

---

## Technology stack

| Layer | Technology |
|-------|------------|
| Desktop shell | Tauri v2 with the dialog and opener plugins |
| Interface | Leptos 0.7, client-side rendered |
| Language | Rust 2021, compiled to WebAssembly |
| Bundler | Trunk |
| Styling | Tailwind CSS v4, CSS-first configuration |
| PDF rendering | pdf.js 6.2.108, vendored locally |
| Serialization | serde with `serde-wasm-bindgen` |

pdf.js is vendored into the repository rather than fetched at runtime, so the application renders
with no network access and the exact engine build is pinned alongside the source.

---

## License

Released under the MIT License. See [LICENSE](LICENSE) for the full text.

This project bundles [pdf.js](https://github.com/mozilla/pdf.js), which is distributed under the
Apache License 2.0.
