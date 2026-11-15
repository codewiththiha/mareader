# Architecture

> **Status:** M0 (foundation) · **Last updated:** 2026-10-13

mareader2 delivers what mareader does — the same features, the same reading
experience, the blend backdrop, panes, appearance, library and study tools — as
one native Rust process: egui for the interface, wgpu for drawing, hayro for
PDF and parley for EPUB.

It is a rewrite, not a port (ADR 0005). mareader is the specification of *what*
the app does. *How* is designed fresh for egui: much of mareader's structure
exists to work around a browser (iframes per pane, a frame transport, reactive
signals, DOM measurement, pdf.js memory latches) and has no reason to exist
here.

## Layers

Lower layers never see egui. `cargo xtask layers` enforces this from
`xtask/src/layers.rs`, which also lists planned crates so a new crate lands in
the right place.

| Layer | Crate (`mareader-…`) | State | Owns |
|---|---|---|---|
| L0 | `appearance` | **exists** | base mode, tint, texture, grain, presets; colour maths; the page filter chain as matrices |
| L0 | `animated-icons` | **exists** | build-time-normalised Lucide animation packs as typed paths, ordered layers and timelines; bounded tiny-skia atlas rasterisation, with no UI toolkit dependency |
| L0 | `workspace` | **exists** | the pane split tree (≤ 4), layout, dividers, drop edges |
| L0 | `fetch` | **exists** | resumable downloads from mirrors, checked by kind before kept; desktop only, empty in a browser |
| L0 | `audio` | **exists** | a book's narration: clips decoded (symphonia), brought to the device's rate, sped up at the same pitch, and played on the system output (cpal); also samples a voice made in memory, queued behind what plays; silent in a browser |
| L0 | `memory` | **exists** | freed heap handed back to the system when the app is quiet (`malloc_trim` on Linux, pressure relief on macOS, nothing on Windows, which returns it already); the one small audited `unsafe` file for it; used when caches rest and when a voice model unloads |
| L0 | `script` | **exists** | a book's scripts in a sandboxed JavaScript engine (Boa) over a small page model: elements, events, timers, form controls; pages written back as markup; desktop only |
| L0 | `nn-core` | **exists** | CPU feature-dispatched kernels, bounded protobuf wire primitives and SplitMix64 stepping shared by the neural voice crates |
| L0 | `kokoro` | **exists** | a small neural voice: Kokoro-82M's inference ported to candle, phonemes in and 24 kHz speech out, chunked at pauses, its noise seeded so equal text sounds equal; weights read from the downloaded PyTorch file or a GGUF build (16- or 8-bit, widened on load), voices from their PyTorch files; Kitten's smaller models of the same design read straight from their ONNX files (full or 8-bit; weights named by the module using them, sizes read off their shapes) with Kitten's alphabet and voices file; credits in its `NOTICE`; desktop only |
| L0 | `g2p` | **exists** | English text to the phonemes Kokoro was trained on: misaki's pipeline ported (spaCy's tokenizer, a part-of-speech tagger, the misaki lexicons, numbers, and a small neural model for unknown words); phonemes also written for people, as IPA or an easy respelling; its data downloads with Kokoro or as the pronunciation pack; credits in its `NOTICE` |
| L0 | `ipa` | **exists** | text in other languages to the IPA Kokoro's voices read, by our own spelling rules with no data to download: Spanish (Castilian), Italian, Brazilian Portuguese, French and Hindi, with stress, links between words (French liaison too), the `a` Hindi leaves unsaid, and numbers in words; written as espeak-ng writes it, so the voices hear what they were trained on, and checked against it on CI |
| L0 | `hanzi` | **exists** | Chinese text to the IPA Kokoro's Mandarin voices read: the reader Kokoro-82M was trained with, reimplemented (numbers to numerals, jieba's word cut with its model for unknown words, pypinyin's readings by phrase, then misaki's pinyin-to-IPA table with tones as arrows); its dictionaries download from pinned commits and load in about 20 MB; checked against misaki on CI; credits in its `NOTICE` |
| L0 | `vits` | **exists** | Piper's VITS voices: inference ported to candle and read straight from each voice's ONNX file (weights named by the module using them, convolution shapes from the graph), so every quality and multi-speaker voice loads with no tables; misaki's phonemes respelled the way espeak writes them for each voice; noise seeded so equal text sounds equal; credits in its `NOTICE`; desktop only |
| L0 | `phonon` | **exists** | Gradium's Phonon 31M ported to candle: English text in and 24 kHz speech out, no phonemes; its own unigram tokenizer, a flow language model primed once per voice that writes Mimi latents a frame at a time, and Mimi's decoder; the reference's English normalisation (numbers, money, addresses, abbreviations and symbols spelled out; country and generic web domains only) and sentence grouping; a pace setting through the model's own duration condition; weights read from the 8-bit GGUF or the full safetensors (widened on load), voices from their safetensors; credits in its `NOTICE`; desktop only |
| L1 | `speech` | **exists** | reading text aloud: sentences cut from page text with their page and character spans, and the engines behind one `Voicer` trait (the system's own voices through `tts`; local models, Kokoro, Piper, Kitten and Phonon, behind one engine that asks each only to load a voice and say a line, their sentences made a few ahead on a thread, the one wanted now in a short first piece so it sounds sooner, kept up to a set size, and played without gaps; a model loads on first use, says so while loading, and leaves memory after a set idle time; a silent one for tests); voice models on offer, each in builds of several sizes that share its voices (or, like Piper, voices that bring their own weights; or, like Kitten, builds that bring every voice, a file two builds share kept once; or, like Phonon, one build and a small file per voice), the build picked for the first download and more added or removed later, downloaded one at a time through `fetch` and checked by SHA-256, a file another model already has linked rather than fetched; a file several voices share, such as the Chinese dictionaries, fetched with the first and kept until the last goes; each model's own settings, such as Piper's expression and rhythm, as shares of each voice's normal; Phonon's speeds past the paces it keeps steady made up by `audio`'s same-pitch stretch; files of voices no longer offered deleted at start; the pronunciation pack the word cards use, a shelf of its own beside the voice models, sharing their files both ways; desktop only |
| L1 | `pdf` | planned M2 | hayro: open, page sizes, tile raster, text and glyph boxes, outline, links, search index |
| L1 | `reflow` | planned M4 | styled blocks → parley lines → pages; typography knobs; hit-testing |
| L1 | `library` | planned M5 | books, locations, shelves, sources, scans (`docs/library.md`) |
| L1 | `study` | planned M6 | dictionary packs, CEFR levels, gloss requests and marks |
| L1 | `dict` | **exists** | dictionary packs: download with mirrors and resume, parquet → SQLite, card lookups, search, bridges through English; downloads and lookups need the desktop |
| L1 | `cefr` | **exists** | word levels: the CEFR dataset (download, parquet → SQLite), which words of a text are hard, a clicked word's role from the tagger; downloads and answers need the desktop |
| L2 | `epub` | planned M4 | rbook container, spine, ToC, XHTML+CSS subset → `reflow` blocks |
| L2 | `settings` | planned M1 | the persisted schema and defaults |
| L3 | `session` | planned M2 | open documents shared by panes, the tile cache, the job scheduler |
| L3 | `assist` | **exists** | AI tasks and their prompts, the providers behind one `Backend` seam, API keys (`docs/assist.md`); desktop only, the one exception to rule 3 |
| L7 | `ui` | **exists** | egui: theme, icons, title bar, panes, every view and popover |
| — | `apps/mareader` | **exists** | the window and `main` |
| — | `xtask` | **exists** | gates |

The rules:
1. A crate depends only on lower layers. Same-layer edges need an entry in
   `SIDEWAYS`.
2. egui, eframe, winit, wgpu and rfd appear only in L7 and the app.
3. L0–L3 compile for `wasm32-unknown-unknown`. A gate for this arrives with the
   first L1 crate.

## How the app is built in egui

egui is immediate mode: every frame, the UI is a function of the state. Write
it that way rather than as a web app in egui syntax.

**State.** One owner, `MareaderApp`, holds plain structs: settings, the pane
tree, the open documents, the library, which popover is open. Domain state lives in
the L0–L3 crates and has methods and tests of its own. egui's `Memory` keeps
only transient widget state (scroll offsets, hover animations, a text field's
cursor). There are no signals, effects, observers or stores, and no state is
mirrored in two places.

**Views are functions.** A view takes the state it shows and a `&mut Commands`:
`fn show(ui: &mut Ui, state: &T, out: &mut Commands)`. It draws and pushes
`Command`s (`crates/ui/src/command.rs`) for what the user asked. It does not
mutate app state, call other views' handlers or keep its own copy of the data.

**Commands are the only way to change state.** Buttons, menus, shortcuts and
drops all produce the same `Command` values, and `MareaderApp::apply` is the
one place they take effect, after the frame's UI pass. So a shortcut and its
button cannot drift apart, and every user action can be tested by pushing a
command.

**Frame order.**
1. `logic`: apply settings changes (restyle, motion switch), drain finished
   background jobs into state.
2. `ui`: panes, then the floating chrome (title bar, popovers, toasts) above
   them, each pushing commands.
3. Apply the commands.

**Use egui's tools, not re-creations of browser ones.**

| Need | egui tool | What mareader needed on the web |
|---|---|---|
| Sidebar, slide in and out | `egui::Panel::left(..).show_collapsible` | CSS transitions, layout observers |
| Popovers, menus | `egui::Popup`, `Ui::menu_button`, `Response::context_menu` | a floating layer, outside-click and Escape wiring |
| Settings window, confirmations | `egui::Modal`, `egui::Window` | modal components, focus traps |
| Tooltips | `Response::on_hover_text` | a tooltip component |
| Long lists and grids (thumbnails, library) | `ScrollArea::show_rows` / `show_viewport` | the `virtual-list` crates |
| Drag and drop inside the app | `Ui::dnd_drag_source` / `dnd_drop_zone`, `Response::dnd_*` | a custom pointer-drag engine |
| Files dropped from the OS | `ctx.input(|i| i.raw.dropped_files)` | Tauri drop events |
| Shortcuts | a keymap table of `KeyboardShortcut` → `Command`, `InputState::consume_shortcut` | key listeners per component |
| Theme | `Style` and `Visuals`, set once per appearance change | CSS variables |
| Animation | `Context::animate_bool_with_time` / `animate_value_with_time` where motion helps | CSS transitions on everything |
| Text layout in the chrome | `LayoutJob`, `Galley` (truncation, wrapping) | DOM measurement probes |
| Accessibility | AccessKit, through `WidgetInfo` on custom widgets | ARIA |

Stock widgets (`Button`, `Slider`, `ComboBox`, `Checkbox`, `TextEdit`) are
styled by the theme and used as they are. A custom widget is written only when
egui has nothing close (caption buttons, page views, library cards); it reads
its colours from `Tokens` or `Visuals` and gives AccessKit a label.

**Repaint is demand-driven.** Every deferred thing schedules its own wake with
`request_repaint_after` (the title bar's hide grace, a toast's time line); worker
threads call `Context::request_repaint` when a result is ready. Nothing polls,
and an idle window renders no frames.

**Persistence.** Settings and the library are serde structs written through
eframe's storage or our own file with an atomic rename, debounced, off the UI
thread for anything large.

## The page pipeline: every page is a texture

A page from either format ends up as GPU textures that one compositor draws.
`docs/adr/0003-pages-are-textures.md` gives the reasoning.

```
PDF:  hayro → RGBA tile ─┐
                         ├─▶ TextureHandle (LRU, capped) ─▶ page pass (wgpu callback)
EPUB: parley → vello_cpu ┘                                    filter · blend · texture · grain
```

- **Raster off the UI thread.** A small worker pool renders tiles at the
  settled scale. During a zoom, the existing texture stretches and is replaced
  once, crisp.
- **The page pass** is an `egui_wgpu::CallbackTrait` paint callback (M3). It
  takes `Appearance::page_filter()` as a ≤ 7-entry matrix uniform
  (`MAX_FILTER_OPS`) and `page_blend()`, and composites the texture tile and the
  grain. `apply_filter` in `crates/appearance` is its CPU reference, and tests
  compare the two. Appearance values are uniforms, so dragging a control
  re-renders no page (`docs/design.md` § How the theme works).
- **Blend backdrop** (PDF): the dominant paper colour of the whole page or its
  edges is sampled from the raster, and the pane background takes that colour
  so the page melts into it.
- **Text overlays are geometry.** Selection, search hits and gloss marks are
  rectangles from the page's glyph index (hayro text extraction or parley
  clusters), painted over the texture.
- **Grabbing empty space pans.** A drag begun off the text lines and links
  (margins, gutters, pictures; word gaps count as text) scrolls the pane
  both ways with a hand cursor. A release while still moving flings it on,
  slowing to a stop; any press halts it. A click there acts as before.

### Scrolling long documents

mareader2 scrolls 10,000-page files at low memory with no virtual-list
crate. The rules:

- **Layout from page sizes alone.** Page rects come from the document's page
  boxes, never from rendering. `ScrollArea::show_viewport` allocates the full
  content height; each frame draws and requests only the pages meeting the
  viewport plus a small prefetch window. A test opens 10,000 pages and checks
  that demand is the visible pages plus prefetch.
- **Visible work is always admitted.** Byte and count caps limit prefetch and
  what stays cached; the viewport already bounds what is on screen, so a
  visible page is never left blank to honour a cap.
- **Whole pages first, tiles only for huge rasters.** hayro interprets the
  whole page for each tile, so a page renders as one
  raster until its long side passes ~4096 px. Past that, a ≤ 2048 px backdrop
  plus at most four tiles per side, the tile side growing with zoom.
- **Hold tiles while panning.** New tiles are requested only after the scroll
  offset rests for ~0.25 s; meanwhile the backdrop or older tiles show,
  stretched. A drag never queues one interpret per frame.
- **Demand first, results bounded.** Each frame publishes its demand before
  receiving. A result no longer in demand is dropped before it becomes a
  `ColorImage`. Finished results wait in a bounded queue (~32 MiB, 32 items).
- **Upload budget per frame.** At most ~8 results, ~16 MiB or ~4 ms of texture
  uploads per frame, so a burst of finished tiles never stalls scrolling.
- **Stale stays until replaced.** Textures of an old zoom are drawn stretched
  until the sharp ones cover the page, then evicted.
- **Text layers are lazy.** Glyph data is extracted on the first selection or
  search, cached by count separately from rasters.
- **Touchpads scroll 1:1.** Precise wheel deltas bypass egui's easing
  or the page lags the fingers on Windows.
- **Colours never re-render.** Appearance is uniforms of the page pass, not
  a recolour while rasterising, so a colour change re-renders nothing.

## EPUB

EPUB replaces mareader's txt and md formats. `docs/epub.md` has the plan and
ADR 0002 the decision: rbook reads the container, a small XHTML+CSS subset
becomes styled blocks, parley lays out and paginates them, and vello_cpu (the
rasteriser hayro uses) draws page textures. EPUB resume stores a stable spine
href and block-relative offset, so a bounded target-centered preview can open a
deep place without laying out its prefix. The preview uses local page indices
with a saved global offset/count; after its target raster arrives, one complete
parse and layout continues in worker batches. Stable spine mapping moves the
view and reusable rasters into the completed layout without intermediate
prefix snapshots. Legacy global-block saves still open in one complete pass.
PDFs enumerate their full page tree before showing a view, then rasterise
visible pages on demand; scrolling can request any indexed page, and the
centered loading mark only covers pages whose raster is missing. Every
mareader typography setting has a parley equivalent.

## Threads and resources

- **UI thread:** egui only. It never blocks on I/O or raster work.
- **Workers:** raster, parse and scan jobs run on threads and report over
  channels. Each result carries the document id and generation it was made
  for; a result for a closed pane, an old zoom or an old document is dropped
  before it is uploaded.
- **File watch** (`crates/ui/src/watch.rs`). One thread looks at the open
  files' size and modified time every second, and wakes the window only once
  a change has held for two looks; the window stays idle meanwhile. A file
  is first seen the moment it is shown, so a save right after it opens
  counts; with nothing open the thread sleeps until told. A save while the
  file is opening again opens it once more after. The file
  opens again on the workers and replaces the old document when it is ready:
  a book lands on its reading place, a PDF on its page and scroll. A file
  unreadable mid-save keeps the old one until the next change.
- **One document, many views** (ADR 0006). A book opens once into an
  `Arc<Document>` held by the documents table; a pane holds only its view
  (document handle, scroll, zoom, selection). Two panes on one book share the
  parse, the glyph index and the tiles.
- **One cache, one budget, one pool.** A single byte-capped tile cache keyed by
  (document, page, scale, tile) serves every pane. One worker pool takes jobs
  in priority order: visible tiles in the active pane, then other visible
  tiles, then prefetch. Jobs carry a generation, so a superseded one is skipped.
- **Teardown is ownership.** When the last view of a document closes, its
  `Arc` drops, its pending jobs are cancelled and its tiles leave the cache. A
  test proves the texture count returns to the baseline.
- **Caps everywhere.** Tile cache capped by bytes, thumbnails by count, and the
  glyph index built on the first search, not at open.
- **Memory claims need measurements:** what was measured, how, the workload,
  before and after, and known limits, recorded in `docs/memory.md`. The starting
  baseline: at 150 %, native hayro peaked 12–118 MiB above baseline, about
  3–15× less than pdf.js on light files and 2–4× less on heavy ones. That run
  was in Node, not a webview, so the comparison is indicative only.

## Window chrome

- **macOS:** full-size content view, title hidden, native traffic lights.
- **Windows and Linux:** `with_decorations(false)`, our own caption buttons in
  the platform's style, and 5 pt resize edges.
- Behaviour: `docs/design.md` § Title bar.

## Decisions

| ADR | Decision |
|---|---|
| [0001](adr/0001-egui-wgpu-native.md) | Native egui + wgpu in one process; no webview |
| [0002](adr/0002-epub-via-parley.md) | EPUB via rbook + our XHTML subset + parley, not a browser engine |
| [0003](adr/0003-pages-are-textures.md) | Both formats rasterise to textures; one GPU page pass applies the look |
| [0004](adr/0004-lucide-icons.md) | Build-generated Lucide geometry behind an `Icon` facade; bounded resting glyphs and active part atlases are tinted at draw time |
| [0005](adr/0005-functional-parity.md) | Match mareader's features, not its pixels or its structure |
| [0006](adr/0006-one-document-many-views.md) | A book opens once; panes are views sharing one cache and worker pool |

## Revision history

| Date | Summary |
|---|---|
| 2026-10-13 | Created with the M0 foundation |
| 2026-10-13 | Functional parity and the egui architecture section (ADR 0005) |
| 2026-10-13 | Shared documents, cache and workers across panes (ADR 0006); appearance as uniforms |
| 2026-10-13 | Scrolling long documents: admission, tiling and upload rules |
