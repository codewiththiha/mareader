# PDF Reader

A desktop document reader built for long-form reading. Native Tauri v2 shell, Rust/WebAssembly
interface written in Leptos, and Mozilla's pdf.js vendored locally as the PDF rendering engine.
Alongside PDFs, the reader opens plain text and Markdown files, which it reflows into pages with
reader-controlled typography.

The design goal is eye comfort over hours of reading: instead of a fixed set of themes, the
appearance system exposes a continuous colour space (base mode plus a computed tint) layered with
optional paper textures and film grain, all persisted between sessions.

---

## Table of contents

- [Highlights](#highlights)
- [Features](#features)
  - [Document viewing](#document-viewing)
  - [Text and Markdown formats](#text-and-markdown-formats)
  - [Zoom and fit](#zoom-and-fit)
  - [Motion](#motion)
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

- Three document formats: PDF (through pdf.js), plus plain text and Markdown, which the reader
  paginates and renders itself with full typography control.
- Four view modes: single page, two-page spread, and virtualized continuous scroll in
  vertical and horizontal orientations, with a unified zoom pipeline across all of them.
- A Fonts settings tab for the text formats: default and per-family font pickers, size, weight,
  line spacing, paragraph margin, word/letter spacing, text indent, book layout, full
  justification and hyphenation.
- Continuous appearance model with three base modes plus a hue and strength tint, replacing
  hard-coded themes.
- Five paper textures with adjustable opacity and scale, plus static or animated film grain.
- Five built-in presets, and unlimited user-saved ones organised into named groups.
- Full-text search with per-page results, highlight rectangles and wrap-around navigation.
- Document outline (table of contents) with automatic highlighting of the active section.
- Virtualized thumbnail grid backed by an LRU bitmap cache.
- Clickable internal links that navigate the reader, and external links that open in the browser.
- Word glossing: select a word (or long-press a saved mark) for a dictionary-style card —
  Apple Intelligence on Apple Silicon, a deterministic mock everywhere else.
- Native file dialog, drag-and-drop opening, and restoration of the last-opened document.
- Settings persisted to local storage with a migration path across schema changes.
- Roughly 460 Rust unit tests across the workspace, plus a stub-vm smoke suite for the
  TypeScript layer, and five scripts that keep facts written down twice from drifting.

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

### Text and Markdown formats

Plain text (`.txt`) and Markdown (`.md`) files open through the reader's own pipeline instead of
pdf.js: the file is parsed into blocks, the blocks are packed into A4 pages, and the pages render
as real text in the DOM.

- **Every view mode works.** Single and spread show pages (spread under the book layout reads as
  a facing pair); the horizontal strip scrolls page by page; the vertical mode streams the text as
  one continuous column with no visible page breaks.
- **Typography you control.** The Fonts settings tab (shown only while a text document is open)
  offers a default font plus serif/sans/monospace family pickers, font size and weight, line
  spacing, paragraph margin, word and letter spacing, first-line indent, a book layout with
  spine-side gutters, full justification, hyphenation, and where the reading column sits while
  streaming (left, centre or right). The Appearance menu adds a body-ink intensity slider for
  text documents — a comfort dial that softens the ink toward the paper without filtering
  anything. Font pickers list the system faces today; the schema already reserves room for fonts
  bundled with the app.
- **Markdown is rendered, not displayed.** Headings, lists, tables, blockquotes, links and fenced
  code (GFM included; raw HTML is refused) render block by block, so a heading never gets split
  across a page break. An image is the one construct that does not arrive: the app's
  content-security policy admits no network origin and the asset protocol serves documents only,
  so a Markdown image displays when — and only when — its source is a `data:` URI.
- **Pagination follows the type.** A hidden measure column renders the document once at scale 1,
  reads the true block heights and re-cuts the pages, so what you see is paginated by the real
  fonts — and re-cuts again whenever a typography knob moves, keeping your place on the block you
  were reading. Zoom never re-paginates: pages scale uniformly, which provably preserves the cut.
- **Search and theming carry over.** Full-text search scans the document in-process (no engine
  round-trip) and paints its hits over the type with the same boxes a PDF gets — one per line
  fragment, the current match in the same amber, stepping to a result scrolling to the block it
  sits in. Base mode, tint, textures and grain apply to text pages exactly as they do to PDF
  pages. Blend mode stays a PDF feature — a text page is recoloured by its tokens directly.

### Zoom and fit

- Fifteen zoom presets from 25 percent to 500 percent, clamped to that range.
- Zoom in and out step through the presets; repeated presses chain from the in-flight target, so
  a fast double press advances two steps rather than being swallowed.
- Fit width and fit page, recomputed on window resize and sidebar toggle.
- Turning to a page of a different size never touches the zoom while Settings → Layout → Auto
  Resize is off: the scale, the scroll position and the measured layout all stay exactly where
  the reader left them, and a page too wide for the window overflows and scrolls instead. Turn
  the switch on and each page re-fits itself to the window as it comes into view.
- Shrink to fit: when the window is too narrow for the chosen zoom, the page shrinks to the space
  available and remembers the zoom to grow back to once space returns.
- Every zoom — a button step, a preset, a fit, a window constraint — runs through one transition
  pipeline. The layout is what animates: the virtualized strips are rescaled every frame and the
  engine holds the document point under the middle of your window exactly where it was, working
  gap-aware because page heights scale and the space between pages does not. Pages stretch the
  bitmap they already hold while the scale moves, the crisp re-render happens once at the settled
  scale, and recently evicted pages linger a moment as zombies so the surface never pops.
- The sidebar slides its width over 300ms and the page rides the slide: every frame of it reports
  a new container width, and the layout follows each one, so the column narrows with the rail
  instead of snapping when a debounced refit finally lands. The crisp re-render is what waits — it
  is issued once the width has been quiet — and the page host never lets flex-shrink resize it, so
  there is no frame of squished paper either way.
- The same follow covers a window drag, with or without a fit: a hand-picked zoom shrinks to stay
  out of the way as the window narrows and grows back to exactly the zoom you chose when the room
  comes back, because the ceiling is computed from the remembered zoom rather than from the last
  frame's scale.
- Zooming a horizontal strip keeps its vertical position: the strip is always at least as tall as
  the tallest page, so the overflow — and with it the scroll position — shrinks and grows with the
  zoom instead of resetting.
- Zooming in at 500 percent, or out at 25 percent, does nothing at all rather than wrapping to the
  other end of the preset ladder — and it leaves the active fit mode alone, so leaning on a button
  that has nothing left to do cannot quietly take you out of fit width.
- The reader keeps written motion principles (see ARCHITECTURE.md): no entrance animations on
  document content, no per-frame virtualizer work, one bounded zombie bridge across commits.

### Motion

Every motion the reader *interpolates* is a switch, and the master for all of them sits in
Settings → Layout. What a switch turns off is the interpolation, never the change: the end frame
still arrives, in the frame the change is asked for. That is what makes freezing the reader safe —
a disabled animation loses nothing, it just skips the frames in between. The converse also holds,
and is why some resizes have no switch: a motion that is the only way to stay correct is not
decorative, so it is not offered.

- **Animations** (the master, in the Layout tab) collapses everything, including the
  micro-transitions that have no switch of their own: menu pops, toasts, the theme cross-fade,
  hover fades. One surface is exempt, because it is the exception to the sentence above: the
  loading mark's motion is the whole message, and a frozen spinner cannot be told apart from a
  hung app — so under either net it trades its travel for a fade and keeps going.
  It is the reader's own `prefers-reduced-motion`, and while it is off the Animations tab is not
  shown at all, because there is nothing left there for it to offer. The other
  switches keep their saved values through it, so turning the master back on returns exactly the
  set of motions you had chosen.
- **Sidebar slide** — the rail tweens its width over 300ms. Off, it appears at its new width, and
  the panel it was holding open is released in the same frame instead of waiting out a slide. The
  page follows the rail either way, and that is what keeps a frozen slide to one step.
- **Canvas follows the window** — the page re-fits on every frame of a window drag. Off, it takes
  the new space in one step once the drag has stopped, and a page too wide for the window overflows
  and scrolls for as long as it is being shown. A rail slide is deliberately not gated here: the
  page has to follow the rail, because the alternative is a page sitting outside the space it was
  given. Dragging a window is different — one relayout per drag frame is a real cost, and nothing
  is wrong in the meantime — so it is the burst that is offered up.
- **Zoom in / out** — a zoom eases to its target over the profile's duration. Off, every zoom
  (button step, preset, fit, window constraint) lands on the first frame.
- **Scroll to page** — a jump to a page, a search hit or a keyboard page turn glides the column
  over. Off, it lands there. The continuous scroll under a *held* arrow is not gated: that is the
  scrolling itself, not a decoration on top of it.

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

- Full-text index built across the document on demand. Text and Markdown documents are scanned
  in-process — the blocks are already Rust strings, so the document is its own index and there is
  no engine round-trip.
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

Five presets ship with the application, in one group:

- **Classic:** Sepia, Green, Night, Parchment, Cinema, each a specific point in the appearance
  space.

Sepia, Green and Night were previously hard-coded themes and are now reproduced exactly as tints,
which is what allows the appearance model to be the only mechanism. The plain bases (Light, Dark,
Dim) are deliberately not presets: the Mode & colour section's buttons are the one home for that
choice, so the gallery holds only looks the buttons cannot express.

Users can save the current appearance as a named preset and organise presets into named groups.
As soon as any slider is nudged, the active preset selection clears and the menu reports "Custom"
rather than claiming an unmodified preset is in effect.

### Opening documents

- Native file dialog through the Tauri dialog plugin, admitting PDFs, plain text (`.txt`,
  `.text`) and Markdown (`.md`, `.markdown`, `.mdown`).
- Drag and drop, handled through the Tauri drag-drop event with a DOM fallback.
- OS file associations for the same formats, so double-clicking or "Open with" hands the file
  to the reader.
- The last opened document is remembered and restored on the next launch.
- Five sample PDFs ship in `public/samples`, covering deep outlines, internal links and awkward
  title metadata. Nothing in the app lists them: they are opened by path (`samples/Outlined
  Book.pdf`, which the engine's loader resolves as a served URL) when a bug needs a specific
  document shape to reproduce against.

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
- Overflow menu with fullscreen and a keyboard shortcut reference.
- Animated sidebar with an outline and thumbnails rail. Panels stay mounted while the sidebar is
  collapsed, so thumbnails survive a toggle without re-rendering.
- Status bar showing the current page position, rendered as a click-through overlay.
- Toast notifications for errors, auto-dismissed after about 3.5 seconds.
- Tooltips on the icon controls.
- A placeholder view with drag-and-drop affordance when no document is open.

### Persistence

Settings are stored in local storage under `pdfreader.settings.v1` and cover appearance, the
active preset, user presets, default zoom, the layout and motion switches, and the last opened
path. A group added later simply defaults: a document opened by an older build keeps the behaviour
it had, because every new switch defaults to what the app used to do.

The appearance model changed shape during development, from six fixed themes to base mode plus
computed tint plus presets, but the storage key was deliberately not bumped. A new key would have
silently reset every reader's last-opened file and zoom as well. Instead the retired fields are
retained as optional values, migrated on load to the preset that reproduces the theme previously
in use, and dropped when writing, so the migration runs at most once.

Writes are debounced by 350 milliseconds so dragging a slider does not hammer local storage.

### Accessibility and motion

- `prefers-reduced-motion` is honoured: page-turn animations, the animated grain and general
  transitions are disabled or reduced to a negligible duration. Settings → Layout → Animations
  applies the same collapse on request, and Settings → Animations then narrows it per motion (see
  [Motion](#motion)). The one exemption is the loading mark, which keeps a motionless fade rather
  than a still frame: an indicator that stops moving reports the wrong thing. Its animation is a
  `transform` on three dots, which the compositor keeps running while the main thread parses a
  document, so the mark is never frozen by the work it stands for.
- Interactive controls carry `aria-label`, `aria-pressed`, `aria-current` and `aria-disabled`
  as appropriate, and icon-only segments carry a title so they are labelled for screen readers
  and on hover.
- Decorative icons are marked `aria-hidden`.
- Keyboard shortcuts do not fire while focus is inside a form control, and chrome scrollers such
  as the thumbnail rail, the sidebar and popovers keep their own arrow-key behaviour.

### Performance and memory

- Release builds optimise for size with link-time optimisation and a single codegen unit, and the
  WebAssembly output is passed through `wasm-opt`.
- Thumbnails are cached as bitmaps in an LRU of 16 entries — each thumbnail is a pair of rasters,
  so the tight cap is what keeps the whole grid near eight megabytes. A cache hit blits
  synchronously with no skeleton, no pulse and no transition, so a remounted row has nothing left
  to flicker.
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
| Auto-scroll on or off (the two scrolling modes) | `Shift` + `A` |
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
|  Leptos 0.8 interface (Rust, compiled to WebAssembly)        |
|  features / components / state / effects, reactive signals   |
+-------------------------------------------------------------+
|  Typed bridge (wasm-bindgen)                                 |
|  pdf-engine: snake_case Rust mapped to camelCase engine      |
+-------------------------------------------------------------+
|  reader bundle (JavaScript, public/readerEngine.js)          |
|  selection tracking for every format; needs no pdf.js        |
+-------------------------------------------------------------+
|  window.PDFReader engine (JavaScript, public/pdfEngine.js)   |
|  render queue, thumbnail cache, text layer, search index     |
+-------------------------------------------------------------+
|  pdf.js 6.2.108, vendored into public/vendor/pdfjs           |
+-------------------------------------------------------------+
```

Pure logic lives in `reader-core`, `pdf-core`, `reflow-core`, `txt-core`, `md-core`,
`ui-geom` and `ai-core` — no DOM and no Leptos — so the view-mode arithmetic, the zoom
ladder, filename rules, colour conversion, the search index, text typography and pagination,
settings migration, the floating-panel placement and the AI word-card's geometry and spring
are all unit-testable on the host.
The layering is a fan with a rule: `reader-core` knows no format at all, the format cores
depend on it, and no core depends on another format's core — which is why adding a format is
a new crate plus a new directory, not an edit to the ones already there. `virtual-list` is the
generic windowing-math library under the viewer, and `ui-geom` the geometry every floating
surface places itself with — a leaf with no dependencies, so the window chrome and the AI
card can share one placement rule and one spring without either depending on the other.

PDFs and text documents share one page/zoom/navigation machinery above the rendering layer; only
the leaf differs. A PDF page is a canvas the pdf.js engine paints; a text page is an A4 host the
reader lays out with real type. Both report the same per-page sizes into the same virtualized
strips, so view modes, zoom and navigation are format-agnostic.

### Project layout

```
src/
  main.rs                 mount entry point
  app/                    bootstrap, routes, the shell that hosts the sidebar
  components/
    primitives/           button, switch, popover, floating positioning,
                          motion and interaction hooks (the chrome's own
                          primitives — icon, icon button, tooltip, the
                          generic DOM/timer hooks — live in app-chrome)
    shell/                the unified application shell: the ShellController
                          (one source of truth for layout), the titlebar
                          family, the sidebar rail family
    menus/                app menu, appearance menu, reader menu
    settings/             the settings modal and its tabs (layout, theme,
                          animations, fonts); the theme tab composes sections
                          it does not own — the AI's from ai/, the raster ones
                          from its own paper module
    viewer/               the SHAPE of reading: the mode dispatch, the four
                          layouts (single, two-page, continuous, horizontal),
                          the shells that own the scroll container,
                          page_host — the one seam that picks a format —
                          refresh, the fingerprints an overlay repaints on,
                          and controls/ (bottom bar, overlay scrollbar, page
                          indicator, page navigation)
    formats/              the SUBSTANCE of a document: pdf/ (canvas + strip),
                          reflow/ (A4 page host, continuous stream, strip,
                          measure column, the spot walk that finds a block's
                          characters in the DOM, and the search-hit layer that
                          paints over them), txt/ and md/ block views, and
                          block_render, the renderer dispatch
    search/               floating search bar and result list
    ai/                   selection pill, word card, gloss popover, the anchor
                          resolvers that place a mark's stroke, and the AI
                          appearance section of the settings modal
    app_overlays/         drag-and-drop feedback, toast host
  effects/
    app/                  window title, shortcuts, persistence wiring
    reader/               fit and zoom follow, page tracking
    appearance/           the appearance-to-CSS bridge (shared, raster,
                          reflow)
  features/
    library/              the shelf: book cards, empty state, sorting
    reader/               the reader page and its two virtualizers
  state/                  the reactive state tree: app (chrome + UI), reader
                          (document, viewer, zoom, search, gloss, AI selection),
                          library
  services/               the document open pipeline, the AI chunk bridge
  storage/                loads and saves over localStorage (settings,
                          library, covers, gloss marks)
  zoom/                   the zoom pipeline: posted commands, target
                          resolution, the tween, and the actuator that owns
                          the one relayout path over both strips
  dom_contract.rs         the attribute, class and element-id names the engine
                          reads and the app writes — one table, both sides
  events.rs               the window-event names the engine dispatches and the
                          app listens for
crates/
  ai-core/                the format-agnostic AI core: the word-explanation
                          wire types (WordInfo, AiError, the chunk envelope),
                          the gloss card's geometry (stepping `ui-geom`'s
                          spring), the gloss mark + MarkAnchor trait
                          (PageAnchor is the PDF impl), the per-document gloss
                          cache and its persistable JSON, and the Tauri
                          explain_word kickoff (the reader settings those types
                          feed live in reader-core, because they are the
                          reader's)
  reader-core/            the reader with no format in it: the format list, the
                          view modes and spread arithmetic, the settings schema
                          (layout, animation, typography, gloss), the colour
                          pipeline, the presets, filename rules, zoom maths, the
                          outline shape, and the shared search model — result
                          shape, the scan both pipelines run, the snippet window
  pdf-core/               pure PDF domain math: page layout constants, the
                          outline wire entries and their clamping, the
                          device-pixel grid the page hosts snap to, and the
                          engine-side page-text index a query scans
  reflow-core/            the shared maths of reflowable text: the block shape
                          and its splitting, the A4 geometry and spine sides,
                          the page cutter, the height estimate, typography
                          resolution and the search over blocks
  txt-core/               plain text: normalisation, paragraph parsing and the
                          line-bounded subdivision a tight page pack needs
  md-core/                Markdown: construct classification, prose subdivision,
                          front-matter metadata and the heading outline
  pdf-engine/             wasm-bindgen bridge to the imperative engine
  pdf-paper/              the blend backdrop's colour brain: the detection
                          area, dominant-colour detection (whole page or edge
                          margins), the per-page palette
  virtual-list/           generic windowing math: the prefix-sum strip,
                          windows, budgets, anchor correction
  virtual-list-leptos/    the Leptos adapter: virtualizer, rows, retention
  ui-geom/                pure geometry for the surfaces that float: panel
                          placement and viewport clamping, plus the damped
                          spring the floating panels and the gloss card ride
  tauri-bridge/           the raw window.__TAURI__ externs (invoke, event
                          listen, window handle, dialog) + the has_tauri
                          probe, declared once for every frontend crate
  app-chrome/             format-agnostic window chrome: the platform probe,
                          the window commands, the caption cluster (Windows
                          squares, GNOME circles), the native macOS traffic
                          lights, the generic titlebar shell, the floating
                          surface adapters (placement glue, shared dismissal),
                          and the shared UI primitives + hooks those surfaces
                          render with
public/
  readerEngine.ts         the format-agnostic bundle (the selection tracker),
                          loaded first and needing no pdf.js
  reader/                 its modules
  pdfEngine.ts            the imperative engine wrapper (bundled to .js)
  engine/                 the engine modules the wrapper imports
  vendor/pdfjs/           vendored pdf.js build, worker, viewer CSS, cmaps
  samples/                five fixture PDFs for manual testing (the smoke test
                          runs against stubs, not these)
src-tauri/                native shell, AI providers, capabilities, icons
styles/
  input.css               Tailwind v4 entry point assembling the design system
  tokens.css              the @theme block, base palettes and runtime vars
  page_host.css           the .pdf-page host, its canvas and the zoom snapshot
  text.css                the reflowable page host and its measure column
  textures.css, noise.css texture modes, and the grain overlay + its crawl
  library.css             the bookshelf and its drag overlay
  components/             shell, title bar, animations, ai, gloss, appearance,
                          thumbnails, pdf.js's text layer, and the search-hit
                          box both format families share
tools/                    engine bundling, the engine smoke test, the
                          repo-reading prelude the checks share, and the
                          consistency checks CI runs (versions, formats, doc
                          paths, event names, the DOM contract)
scripts/                  generated only: the compiled tools above. Gitignored,
                          and ignored wholesale by Trunk's watcher, so the hook
                          rewriting them on every build cannot retrigger one
tests/                    source-level tests (e.g. the conditional-class lint)
release-notes/            one file per version; the release workflow publishes
                          the one matching the tag as the release body
```

### Engine API

`window.PDFReader` is the single boundary between Rust and pdf.js. Every function resolves and
never rejects: success is `{ok: true, ...}` and failure is `{ok: false, error: {name, message}}`,
so the Rust side reads `ok` first and then deserializes.

| Function | Purpose |
|----------|---------|
| `version` | Engine version string |
| `open` | Load a document, return page count and intrinsic page sizes (the outline is NOT resolved here — that is `resolveOutline`'s job, so opens stay fast on chapter-heavy books) |
| `resolveOutline` | Flatten the open document's chapter tree after the reader is up |
| `destroy` | Tear down the current document |
| `registerPage` / `unregisterPage` | Bind and release a canvas for a page |
| `cancelPage` | Cancel an in-flight page render |
| `renderPage` | Render one page |
| `renderThumb` / `cancelThumb` | Thumbnail rendering on a separate, cheaper path |
| `hasThumb` / `blitThumb` | Probe the bitmap cache and blit a cached frame |
| `extractPageText` | One page's text items with their rects — the input to the search index, which is Rust (`crates/pdf-core`'s `SearchIndex`), not the engine's |
| `setSearchContext` / `setActiveMatch` / `clearHighlights` | Paint, move and clear the engine's highlight rects in the text layer |
| `refreshTheme` / `setScrubMode` / `setLivePipeline` / `isLivePipeline` | The appearance pipeline: rebake canvases with the theme, or keep them raw under the CSS filter chain while a slider moves |
| `setPaper` / `setPaperActive` / `takePaperFrame` / `samplePaperPage` | The paper session: the backdrop's own raster, handed to and sampled from the pages |
| `coverDataUrl` / `prefetchThumb` | The shelf cover and thumbnail prefetch |
| `stats` | Internal counters, used to assert memory is actually released |

Load order in `index.html` is deliberate. The reader bundle goes first because it needs nothing;
then pdf.js, which is ESM-only in version 6 and must execute before the engine so
`globalThis.pdfjsLib` exists; then the engine. All three run before the WebAssembly module, which
top-level-awaits its own init: the app reaches for `window.PDFReader` and for the selection state
as soon as its first components mount, and a module script that had not run yet leaves both
undefined.

### State model

A single `AppState` tree of Leptos signals is threaded through the component tree: the persisted
`settings`, the reader, the library, and the app's own `ui` state. The reader's branches are the
document, the viewer — page, mode, fit, container size, and the zoom transaction inside it —
search, the gloss marks and the AI's selection state. Effects subscribe to it rather than
components talking to one another, which keeps ownership of each concern in exactly one place.
During a zoom, for example, a single system owns the display scale and render scale, and every
zoom control posts a request to it rather than writing the scale directly.

---

## Getting started

### Prerequisites

- **Rust** with the `wasm32-unknown-unknown` target
- **Node.js** 20 or newer, required by the Tailwind v4 CLI
- **Trunk**, the WebAssembly bundler
- **The Tauri CLI**, which the `cargo tauri dev` and `cargo tauri build` commands below run
  through
- **Tauri v2 system dependencies** for your platform, listed at
  <https://tauri.app/start/prerequisites/>

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk
cargo install tauri-cli --version "^2" --locked
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

This generates the TypeScript bundles, starts Trunk on port 1420 and opens the native window. Tailwind is compiled by a Trunk pre-build hook, so stylesheet changes rebuild
automatically. To run the interface in a plain browser instead, without the native shell,
generate the engine bundle once and serve:

```bash
npm run build:ts
trunk serve
```

Note that the file dialog and drag-and-drop rely on Tauri and are unavailable in a browser.

### Building for release

```bash
cargo tauri build
```

Bundles are produced for the host platform in `target/release/bundle` — the workspace shares one
target directory, so nothing lands under `src-tauri/`. The bundle
configuration targets all available formats and ships icons for macOS, Windows and Linux.

### Tests

```bash
cargo test --workspace --exclude pdf
```

The manifest root is also the `pdf-reader` app package, so a bare `cargo test` would test
only the app and silently skip every member crate. The `pdf` shell crate is excluded because
`tauri::generate_context!` resolves the frontend dist at compile time; it is clippy-checked
and unit-tested natively on the macOS CI job instead.

Around 460 tests cover the pure layer: zoom and fit maths, page layout and spread stepping,
filename derivation, colour conversion, appearance CSS generation, presets, settings
migration, search index arithmetic, outline activation, thumbnail geometry, the frame delta
the animation loops share, and the virtual-list windowing invariants. On top of that, the
TypeScript layer has its own stub-vm smoke suite (`node scripts/test-engine-smoke.js` in CI)
covering open, render, theme baking, scrub mode, thumbnails, search, teardown, and the reader
bundle's selection tracker — the last in a sandbox with no engine and no pdf.js in scope,
which is the point of it.

Five small scripts guard facts that are written down more than once, where nothing else
would notice a drift: `check-versions.ts` (the app version in four files),
`check-formats.ts` (the openable formats in the reader-core registry, the shell's
filesystem gate and the bundle's file associations), `check-doc-paths.ts` (every module and
file path named in a Rust comment still resolves, every path named in a stylesheet comment,
every component name the two documents put in backticks, and every method in the README's
engine table), `check-events.ts` (the window-event names
the engine dispatches match the app's table) and `check-dom-contract.ts` (the attribute,
class and element-id names the app writes match the ones the engine reads, and appear
nowhere as a raw literal). Each is TypeScript in `tools/`, compiled by the same Trunk pre-build
hook into `scripts/`, and each fails CI rather than warning. They share one prelude, `repo.ts` —
the repo root, `read`, `isFile`, and the tree walk with its list of directories that are not
source — because five copies of a skip list is five chances for one tool to start scanning
`node_modules`. `scripts/` holds nothing but their compiled output, which is why git ignores the
directory and Trunk's watcher does too: the hook rewrites those files on every build, and a
watcher that notices would rebuild forever.

---

## Configuration

| File | Purpose |
|------|---------|
| `Trunk.toml` | Build target, the Tailwind pre-build hook, watch exclusions, dev server port |
| `src-tauri/tauri.conf.json` | Window size and title, bundle targets, asset protocol scope |
| `src-tauri/capabilities/default.json` | Permission grants for the main window |
| `styles/input.css` | Tailwind v4 entry point and the complete design system |
| `.taurignore` | Paths excluded from the Tauri file watcher |

The asset protocol is scoped to the home, desktop, documents and downloads directories, which
is what allows the engine to read a chosen file while keeping the rest of the filesystem out
of reach.

The default window is 1200 by 800, with a floor of 640 by 480.

---

## Technology stack

| Layer | Technology |
|-------|------------|
| Desktop shell | Tauri v2 with the dialog and single-instance plugins |
| Interface | Leptos 0.8, client-side rendered |
| Language | Rust 2024, compiled to WebAssembly |
| Bundler | Trunk |
| Styling | Tailwind CSS v4, CSS-first configuration |
| PDF rendering | pdf.js 6.2.108, vendored locally |
| Text/Markdown rendering | In-app pagination plus `leptos-md` (GFM, no raw HTML) |
| Serialization | serde with `serde-wasm-bindgen` |

pdf.js is vendored into the repository rather than fetched at runtime, so the application renders
with no network access and the exact engine build is pinned alongside the source.

---

## License

Released under the MIT License. See [LICENSE](LICENSE) for the full text.

This project bundles [pdf.js](https://github.com/mozilla/pdf.js), which is distributed under the
Apache License 2.0.
