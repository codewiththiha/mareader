# Rebuilding mareader's features

> **Status:** living map · **Last updated:** 2026-10-13

mareader (`../mareader`, read-only) is the specification of *what* the app does:
its features, settings, edge cases and the regressions its comments record. It
is not a code source and not a structure to copy (ADR 0005). Each feature is
rebuilt from its goal, as the smallest native design that delivers it.

## How to rebuild one feature

1. **Pick** a `planned` entry in `parity/mareader-features.toml`: lowest
   milestone first, then highest tier.
2. **Find the goal.** Read the entry's `reference` paths, the README section and
   any `Mareader.md` or `docs/` notes. Write down, in a sentence or two, what
   the user gets.
3. **List the behaviour worth keeping:** the settings and their ranges, the
   edge cases a user would notice, the regressions recorded in comments and
   tests. Leave out what exists only because of the browser or because of an
   earlier design (the table below).
4. **Design natively.** The smallest model in an L0–L3 crate (no egui), a view
   that pushes `Command`s, egui's own widgets and containers. If the design
   differs from mareader in a way users would notice, write it in the feature's
   doc or an ADR; if it drops something users rely on, ask the owner.
5. **Test first.** Unit tests on the model (port mareader's cases where they
   apply), a kittest for the interaction. Watch each fail before the fix.
6. **Look at it** with the shot tool: light, dark, a tint; wide and narrow.
7. **Record:** the parity entry's status, evidence and notes; `docs/design.md`
   if the look changed; a line in the `ROADMAP.md` progress log.

## What not to port

| mareader has | Because of | mareader2 uses |
|---|---|---|
| `frame-transport`, `runtime-contract`, `tauri-bridge`, pane iframes, `docs/pane-runtimes.md`, `docs/session-ownership.md` | A JS realm per pane and a Rust↔JS bridge | One process: a pane owns a `Session` struct |
| Leptos signals, memos, effects, `app-state` stores | Reactive DOM rendering | Fields on plain structs; the code that changes a value does its follow-up; `Command`s |
| `virtual-list`, `virtual-list-leptos` | DOM node counts | `ScrollArea::show_rows` / `show_viewport` |
| The memory model's peaks, latches and floors (`docs/memory/*`) | pdf.js and canvas backing stores | Owned textures with byte-capped caches; measure in `docs/memory.md` |
| Canvas release helpers, `WeakRef` registries | Garbage-collected DOM objects | `Drop` |
| Per-pane document copies, per-pane caches and budgets | Each pane was an iframe with its own realm | One `Arc<Document>` per book, one tile cache, one worker pool (ADR 0006) |
| Custom pointer-drag engine, hold/drag/tap arbitration | Browser drag-and-drop limits | egui `dnd_*`, `Response` drag and long-press state |
| Hidden DOM probes, resize observers | Measuring layout in a browser | `Galley` sizes, `Ui::available_size` |
| Focus-reclaim tricks for key repeat | DOM focus moving with unmounted nodes | egui input is global: a keymap reads `InputState` |
| The library's departures, rungs, name-conflict sheets, link rows, twin rows | Editable folder-mirror shelves (`docs/library.md`) | Read-only sources plus id-based shelves; a twin is a second book id |
| The txt and md pipelines (`txt-core`, `md-core`) | Formats now replaced | EPUB (`docs/epub.md`) |
| Visual quirks (the sepia magenta) | An arc through hue space | Our design (`docs/design.md`) |

## Crate map

| mareader | mareader2 | How |
|---|---|---|
| `reader-core/src/appearance` | `crates/appearance` | **rebuilt** (M0): model, tokens, filter chain, presets |
| `reader-core/src/settings` | `crates/settings` | **rebuilt** (M1): the same settings, our schema |
| `reader-runtime/src/host/{tree,drop_target}` | `crates/workspace` | **rebuilt** (M0): split tree, limits, drop edges |
| `library-core`, `library-runtime` | `crates/library` (M5) | **redesigned** (`docs/library.md`) |
| `dict-core`, `cefr-core`, `ai-core`, `download-core` | `crates/study` (M6) | the behaviour; transports are new |
| `pdf-core`, `pdf-engine`, `pdf-paper` | `crates/pdf` (M2) | on hayro; paper-colour detection for blend |
| `reflow-core` (+ `txt-core`, `md-core`) | `crates/reflow` + `crates/epub` (M4) | EPUB via parley |
| `app-chrome`, `app-ui`, `reader-runtime` components | `crates/ui-egui` | view by view, egui-native |
| `src-tauri/src/{ai,cefr,dict,download.rs}` | `crates/study` + `crates/ui-egui` | the Apple Intelligence bridge needs a design (M6) |
| `styles/*.css` | `crates/ui-egui/src/theme.rs` | `docs/design.md` |

## Concept map

| In mareader | In mareader2 |
|---|---|
| A component with props and signals | `fn show(ui, &state, &mut Commands)` |
| An event handler that mutates a store | push a `Command`; `MareaderApp::apply` changes state |
| CSS variables and classes | `Style` / `Visuals` from `theme::apply`, `Tokens` for custom widgets |
| CSS `transition` | nothing (egui hover is instant), or `animate_*` where motion helps |
| `backdrop-filter: blur` | a wgpu blur pass under the bar (M1) |
| CSS `filter` / `mix-blend-mode` on the page | the page pass (ADR 0003) |
| DOM text layer | glyph geometry and painted selection rectangles |
| `aria-*` | AccessKit via `WidgetInfo`; kittest finds widgets by label |
| `setTimeout`, rAF | `request_repaint_after`, frame time |
| localStorage | serde files written atomically |
| Browser smoke tests | kittest and the shot tool |

## Revision history

| Date | Summary |
|---|---|
| 2026-10-13 | Created with the M0 foundation |
| 2026-10-13 | Rebuild from goals, not ports (ADR 0005); what not to port |
| 2026-10-13 | Per-pane isolation is not ported (ADR 0006) |
