# Architecture

> **Status:** engine-first · **Last updated:** 2026-11-15

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
| L0 | `search` | **exists** | finding text in a document: case and diacritic folding, searchable page text, hit spans and snippets |
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
| L1 | `pdf` | **exists** | hayro: open, page sizes, tile raster, text and glyph boxes, outline, links, search index |
| L1 | `reflow` | **exists** | styled blocks → parley lines → pages; typography knobs; hit-testing |
| L1 | `library` | **exists** | books, locations, shelves, sources, scans, SQLite store (`docs/library.md`) |
| L1 | `study` | **exists** | gloss requests, answers, marks and highlights |
| L1 | `dict` | **exists** | dictionary packs: download with mirrors and resume, parquet → SQLite, card lookups, search, bridges through English; downloads and lookups need the desktop |
| L1 | `cefr` | **exists** | word levels: the CEFR dataset (download, parquet → SQLite), which words of a text are hard, a clicked word's role from the tagger; downloads and answers need the desktop |
| L2 | `epub` | **exists** | rbook container, spine, ToC, XHTML+CSS subset → `reflow` blocks |
| L2 | `settings` | **exists** | the persisted schema, defaults, ranges and migration |
| L3 | `session` | **exists** | open documents shared by panes, the tile cache, the job scheduler |
| L3 | `assist` | **exists** | AI tasks and their prompts, the providers behind one `Backend` seam, API keys (`docs/assist.md`); desktop only, the one exception to rule 3, listed in `xtask/src/wasm.rs` `EXEMPT` with its reason |
| L5 | `engine` | **exists** | the app without a window (ADR 0007): `Session` owns every piece of durable state (settings, pane tree, focus, route, open documents, reading places, library, marks and notes, read-aloud, study/dict/vocab, page AI, running jobs), `Command` + `SPECS` are the single door in, `Event` drains results out, `Host` isolates the five platform capabilities, and `Session` writes `settings.json` and `session.json` atomically; no toolkit may be named here |
| L6 | `ui-egui` | **exists** | one front end in egui: theme, icons, title bar, panes, every view, popover, GPU textures and window-ephemeral `UiState` / `UiCommand` |
| L7 | `apps/mareader` | **exists** | the desktop window, `main`, OS file pickers and `desk.rs`: the `Host` the engine reaches this machine through, with `system.rs` for its file manager and opener |
| L7 | `apps/mareader-web` | **exists** | the browser preview: the same app through eframe's `WebRunner`, and a `Host` with a clipboard and no files |
| L7 | `apps/mareader-cli` | **exists** | the headless second client (`commands`, `exec`, `run`, `inspect`) driving `mareader-engine` directly with zero toolkit dependencies |
| — | `xtask` | **exists** | gates (`layers`, `wasm`, `trapped`, `parity`, `assets`, `unsafe`, `comments`, `ci`) |

The rules (enforced with the eight laws in `AGENTS.md` § Layering and ADR 0007):
1. A crate depends only on lower layers. Same-layer edges need an entry in
   `SIDEWAYS`.
2. egui, eframe, egui_extras, egui_kittest, winit, wgpu and rfd appear only in
   L6, in `apps/mareader`, `apps/mareader-web` and tooling. `apps/mareader-cli`
   may link none of them (checked transitively with `cargo tree` by
   `cargo xtask layers`), so the dependency graph itself proves the engine is
   usable without a window.
3. Every crate at L0–L5, `ui-egui` and `mareader-web` compiles for
   `wasm32-unknown-unknown`. `cargo xtask wasm` checks them all; `assist` is the
   one exemption, because AI needs threads, a keychain and TLS (see below).
4. A file under `crates/ui-egui/src` that names no toolkit is a file that belongs
   in the engine, and `cargo xtask trapped` fails on it: the gate is absolute.
   Only `lib.rs` and `icons/pack.rs` are allowlisted, each being front-end
   presentation with nowhere else to go, and a unit test pins that two-file list.
   `tests/` and `examples/` are harnesses and are not counted.
5. Every shipped entry in `parity/mareader-features.toml` cites a registered
   command id from `engine::SPECS` (`command = "<id>"`) or explicitly declares
   `ui_only = true`, checked by `cargo xtask parity`.

## How the app is built in egui

egui is immediate mode: every frame, the UI is a function of the state. Write
it that way rather than as a web app in egui syntax.

**State.** Durable application state has one owner: `engine::Session` (L5),
holding plain structs — settings, the pane tree, focus, route, open documents
and per-pane reading places, the library, marks and notes, read-aloud, study
and page-AI results, and running background jobs. `MareaderApp` holds
`{ session, ui }`: `ui` (`UiState`) keeps only window-ephemeral state (open
popovers, hover timers, drag ghosts, animations, GPU textures and caches), and
egui's `Memory` keeps transient widget state (scroll offsets, text cursors).
There are no signals, effects, observers or stores, and no state is mirrored in
two places.

**Views are functions.** A view takes the state it shows and pushes commands:
`engine::Command` (`crates/engine/src/command.rs`) for application actions and
`ui_egui::UiCommand` (`crates/ui-egui/src/ui_command.rs`) for chrome-only
actions (popovers, focus, window captions, OS dialogs). Views do not mutate
application state, call other views' handlers or keep their own copies of the
data.

**Commands are the only way to change state.** Buttons, menus, shortcuts, drops
and `mareader-cli` all produce the same registered `engine::Command` values
(`engine::SPECS`). After the frame's UI pass, every `engine::Command` enters
`Session::execute` (the one door into the engine) and every `UiCommand` runs in
`MareaderApp::apply`. `ui-egui` never matches over `engine::Command`. Results
leave the engine via `Session::drain() -> Vec<Event>`.

**Frame order.**
1. `logic`: tick `Session` (`Session::tick(now)`), drain engine `Event`s into
   chrome state (toasts, window title, notifications) and finished pixel rasters
   into GPU textures.
2. `ui`: panes, then the floating chrome (title bar, popovers, toasts) above
   them, each pushing `Command` or `UiCommand`.
3. Apply `Command`s through `Session::execute` and `UiCommand`s through
   `MareaderApp::apply`.

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

**Persistence.** The engine saves itself (Law 8): `Session` writes
`settings.json` and `session.json` atomically (temp file + rename) under
`Host::state_dir()`, debounced during interactive edits and flushed on
shutdown (`Session::flush()`). The library keeps its own SQLite store under the
same directory. `eframe::Storage` is touched only in the one-time v0.0.5
importer on first launch.

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
- **The page pass** is an `egui_wgpu::CallbackTrait` paint callback. It
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
- **File watch** (`crates/engine/src/watch.rs`). One thread looks at the open
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
| [0007](adr/0007-engine-first.md) | Engine-first architecture: `mareader-engine` (L5) owns all durable state and commands; `ui-egui` (L6) draws; `mareader-cli` (L7) proves zero-toolkit execution |

## Revision history

| Date | Summary |
|---|---|
| 2026-10-13 | Foundation workspace, crate layers, and initial architecture |
| 2026-10-13 | Functional parity and immediate-mode egui architecture (ADR 0005) |
| 2026-10-13 | Shared documents, cache and workers across panes (ADR 0006); appearance as uniforms |
| 2026-10-13 | Long-document scrolling rules: viewport admission, high-zoom tiling and upload budgets |
| 2026-11-11 | Engine boundary and workspace gates: rename front end to `crates/ui-egui` (L6), introduce `mareader-engine` (L5), check binaries at L7, and enforce `cargo xtask wasm` and `cargo xtask trapped` |
| 2026-11-11 | Core rules extracted into `mareader-engine` across the crate boundary |
| 2026-11-12 | Command split and single reducer: separate `engine::Command` from `ui_egui::UiCommand`, route mutations through `Session::execute`, register dotted ids in `SPECS`, and pair `{ session, ui }` in `MareaderApp` |
| 2026-11-13 | Stateful controllers moved to engine: documents, marks, read-aloud, study/dict/vocab, library and page-AI move to `mareader-engine` behind `Waker`, leaving only the four pixel worker files (`icons/anim.rs`, `machine_fonts.rs`, `library/covers.rs`, `image_viewer.rs`) in `ui-egui` |
| 2026-11-14 | Host trait and atomic persistence: `engine::Host` isolates platform calls, `Session::new(host, waker)` wires services, engine writes `settings.json` and `session.json` atomically with one-time v0.0.5 `eframe::Storage` import, and `cargo xtask trapped` becomes an absolute zero-trapped gate |
| 2026-11-15 | Headless CLI client: `apps/mareader-cli` ships `commands`, `exec`, `run` and `inspect` with zero toolkit dependencies (checked transitively by `cargo xtask layers`) and five end-to-end scripts in CI |
| 2026-11-15 | Architecture lock and parity enforcement: ADR 0007 adopted, layer laws documented, and all 122 shipped parity entries linked to `engine::SPECS` or `ui_only = true` in `cargo xtask parity` |
| 2026-11-15 | Post-split cleanup and performance pass: retired transitional shims, tightened session boundaries, streamed EPUB covers and Kokoro weights, cached PDF `RenderSession`, reduced search/G2P/GPU allocations |
