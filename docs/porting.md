# Rebuilding mareader's features

> **Status:** living map · **Last updated:** 2026-11-15

mareader (`../mareader`, read-only) is the specification of *what* the app does:
its features, settings, edge cases and the regressions its comments record. It
is not a code source and not a structure to copy (ADR 0005). Each feature is
rebuilt from its goal, as the smallest native design that delivers it.

## How to rebuild one feature

1. **Pick** a `planned` entry in `parity/mareader-features.toml`: highest
   priority tier first (`P0` before `P1`).
2. **Find the goal.** Read the entry's `reference` paths, the README section and
   any `Mareader.md` or `docs/` notes. Write down, in a sentence or two, what
   the user gets.
3. **List the behaviour worth keeping:** the settings and their ranges, the
   edge cases a user would notice, the regressions recorded in comments and
   tests. Leave out what exists only because of the browser or because of an
   earlier design (the table below).
4. **Design natively.** Put pure rules in L0–L3, durable state and registered
   `Command`s in `crates/engine` (L5, ADR 0007), and views in `crates/ui-egui`
   (L6) using egui's own widgets. If the design differs from mareader in a way
   users would notice, write it in the feature's doc or an ADR; if it drops
   something users rely on, ask the owner.
5. **Test first.** Unit tests on the model and `engine::Session` (port
   mareader's cases where they apply), plus a kittest for UI interaction. Watch
   each fail before the fix.
6. **Look at it** with the shot tool (`docs/development.md` section The look
   loop): light, dark, a tint; wide and narrow.
7. **Record:** update the parity entry's status, evidence, `command`/`ui_only`
   link and notes; update `docs/design.md` if the look changed; update
   `ROADMAP.md` for major capabilities.

## What not to port

| mareader has | Because of | mareader2 uses |
|---|---|---|
| `frame-transport`, `runtime-contract`, `tauri-bridge`, pane iframes, separate pane runtimes and session ownership layers | A JS realm per pane and a Rust↔JS bridge | One process: `engine::Session` owns panes and shared documents |
| Leptos signals, memos, effects, `app-state` stores | Reactive DOM rendering | Plain structs in `engine::Session`; `Command`s through `Session::execute` |
| `virtual-list`, `virtual-list-leptos` | DOM node counts | `ScrollArea::show_rows` / `show_viewport` |
| The memory model's peaks, latches and floors | pdf.js and canvas backing stores | Owned textures with byte-capped caches; measured in `docs/memory.md` |
| Canvas release helpers, `WeakRef` registries | Garbage-collected DOM objects | `Drop` |
| Per-pane document copies, per-pane caches and budgets | Each pane was an iframe with its own realm | One `Arc<Document>` per book, one tile cache, one worker pool (ADR 0006) |
| Custom pointer-drag engine, hold/drag/tap arbitration | Browser drag-and-drop limits | egui `dnd_*`, `Response` drag and long-press state |
| Hidden DOM probes, resize observers | Measuring layout in a browser | `Galley` sizes, `Ui::available_size` |
| Focus-reclaim tricks for key repeat | DOM focus moving with unmounted nodes | egui input is global: a keymap reads `InputState` |
| The library's departures, rungs, name-conflict sheets, link rows, twin rows | Editable folder-mirror shelves (`docs/library.md`) | Read-only sources plus id-based shelves; a twin is a second book id |
| The txt and md pipelines (`txt-core`, `md-core`) | Formats now replaced | In-memory EPUB conversion (`docs/epub.md`) |
| Visual quirks (the sepia magenta) | An arc through hue space | Our design (`docs/design.md`) |

## Crate map

| mareader | mareader2 | Notes |
|---|---|---|
| `reader-core/src/appearance` | `crates/appearance` | Model, tokens, filter chain, presets |
| `reader-core/src/settings` | `crates/settings` | Equivalent settings with typed schema and migration |
| `reader-runtime/src/host/{tree,drop_target}` | `crates/workspace` | Split tree, limits, drop edges |
| `library-core`, `library-runtime` | `crates/library` | Redesigned SQLite sync-ready store (`docs/library.md`) |
| `dict-core`, `cefr-core`, `ai-core`, `download-core` | `crates/study`, `crates/dict`, `crates/cefr`, `crates/assist`, `crates/fetch` | Split by domain with desktop transports and pure rules |
| `pdf-core`, `pdf-engine`, `pdf-paper` | `crates/pdf` | `hayro` rendering, text extraction, paper-colour detection |
| `reflow-core` (+ `txt-core`, `md-core`) | `crates/reflow`, `crates/epub` | EPUB via `rbook` + `parley` + `vello_cpu` |
| App state, command routing, controllers, persistence | `crates/engine` | Toolkit-free `Session`, `Command`/`SPECS`, `Host`, atomic JSON save (ADR 0007) |
| `app-chrome`, `app-ui`, `reader-runtime` components | `crates/ui-egui` | Immediate-mode egui front end and wgpu passes |
| `styles/*.css` | `crates/ui-egui/src/theme.rs` | `docs/design.md` |

## Concept map

| In mareader | In mareader2 |
|---|---|
| A component with props and signals | `fn show(ui, &state, &mut Commands)` |
| An event handler that mutates a store | Push `engine::Command` (`Session::execute`) or `UiCommand` (`MareaderApp::apply`) |
| CSS variables and classes | `Style` / `Visuals` from `theme::apply`, `Tokens` for custom widgets |
| CSS `transition` | Instant hover, or `animate_*` where motion helps |
| `backdrop-filter: blur` | wgpu glass blur pass under title and page bars |
| CSS `filter` / `mix-blend-mode` on the page | GPU page pass (ADR 0003) |
| DOM text layer | Glyph geometry and painted selection rectangles |
| `aria-*` | AccessKit via `WidgetInfo`; kittest finds widgets by label |
| `setTimeout`, rAF | `request_repaint_after`, frame time |
| `localStorage` | Atomic JSON files under `Host::state_dir()` (`settings.json`, `session.json`) |
| Browser smoke tests | `mareader-cli` scripts, `egui_kittest`, and the `shot` tool |

## Revision history

| Date | Summary |
|---|---|
| 2026-10-13 | Initial porting map, functional parity (ADR 0005), shared documents (ADR 0006) |
| 2026-11-15 | Updated crate and concept maps for engine-first architecture (`crates/engine`, `crates/ui-egui`, ADR 0007) |
