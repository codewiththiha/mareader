# mareader2 — instructions for agents

mareader2 rebuilds [mareader](https://github.com/codewiththiha/mareader) natively
in Rust (egui + wgpu, hayro for PDF, parley for EPUB). `AGENTS.md` holds the
binding rules and wins over this file.

## Start every session here

1. Read `ROADMAP.md`: the current milestone, the **Next** list and the progress
   log.
2. Pick work with `cargo xtask parity --list planned` (or `partial`): the lowest
   milestone first, then the highest tier (P0 before P1).
3. Read the entry's `reference` paths in `../mareader` to learn the goal and the
   edge cases, `docs/architecture.md` § How the app is built in egui, and
   `docs/design.md` if it is visible.
4. Rebuild it with `docs/porting.md` § How to rebuild one feature: goal →
   behaviour worth keeping → native design → failing test → implementation →
   look at the result → parity entry and `ROADMAP.md`.
5. At the end of the session, add a line to the `ROADMAP.md` progress log and
   update its **Next** list.

Don't stop to ask unless the decision is on the owner's list below.

## Owner decisions (ask, don't guess)

- Allowing any new `unsafe` (the complete exception list is in `AGENTS.md`).
- Adding a dependency that is heavy (over ~50 transitive crates), copyleft, or
  replaces one named in an ADR (parley for EPUB is settled).
- Dropping or changing a behaviour mareader users rely on. (Improving the
  look or the design for the same outcome needs no permission.)
- Adding an asset whose licence is not on the `AGENTS.md` list.
- Dropping a parity feature.

## Where things are

| Path | What |
|---|---|
| `apps/mareader` | The binary: opens the window, runs `MareaderApp` |
| `crates/ui` | The front layer: theme, icons, title bar, panes, shell (egui) |
| `crates/appearance` | Base modes, tints, filters, presets, UI tokens (no egui) |
| `crates/workspace` | The pane split tree and its layout (no egui) |
| `crates/settings` | Every user setting: schema, defaults, ranges, migration, mareader import (no egui) |
| `crates/ui/src/command.rs` | `Command`: every user intent, applied in `MareaderApp::apply` |
| `xtask` | `layers`, `parity`, `assets`, `unsafe`, `comments`, `ci` |
| `parity/mareader-features.toml` | Every mareader feature, its status and evidence |
| `docs/design.md` | The design system: colour, type, metrics, chrome, motion |
| `docs/library.md` | The library redesign: goals, model, UI |
| `docs/architecture.md` | Layers, frame model, page pipeline, threads |
| `docs/epub.md` | The EPUB pipeline and how mareader's typography maps onto it |
| `docs/porting.md` | How to rebuild a feature, what not to port, crate and concept maps |
| `docs/memory.md` | Measured memory, budgets |
| `docs/adr/` | Decisions: egui+wgpu, EPUB via parley, page textures, Lucide, functional parity, shared documents |
| `tools/` | Scripts and parity tools; see the directory |

## Commands

```sh
cargo run -p mareader                       # the app
tools/web.sh --serve 8080                   # browser viewer, best effort (?panes=2)
cargo test -p mareader-ui -j 2              # one crate's tests
cargo clippy -p mareader-ui -j 2 --all-targets -- -D warnings
cargo xtask parity                          # progress by area and tier
cargo xtask parity --list planned           # what is left
cargo xtask layers                          # dependency rules
cargo xtask assets                          # ATTRIBUTION.toml check
cargo xtask comments                        # comment rule on changed files
cargo xtask ci                              # the full sweep (CI does this)
python3 tools/commit_check.py --install     # once per clone
```

`cargo xtask parity` checks `reference` paths against `../mareader` (or
`MAREADER_DIR`) when the checkout is there, and says so when it is not.

## The look loop

The shell renders headlessly, so a UI change can be seen without a display:

```sh
cargo run -p mareader-ui --example shot -- /tmp/a.png \
  platform=windows theme=dark pinned=true panes=2 hover=1180,20 size=1240x720
```

| Key | Values |
|---|---|
| `theme` | `light`, `dark`, `dim` (base mode) |
| `preset` | `sepia`, `green`, `night`, `parchment`, `cinema` |
| `platform` | `mac`, `windows`, `gnome` (chrome to draw) |
| `pinned`, `chromeless`, `animations` | `true` / `false` |
| `panes` | 1–4 (splits the newest pane, alternating right and down) |
| `open` | `appearance` (the popover), `settings`, `menu` (View & tools) |
| `toast` | a message; `error:` before it for the error tone |
| `settings` | a tab: `layout`, `theme`, `animations`, `vocabulary`, `dictionary`, `workspace` |
| `sidebar` | `thumbnails`, `outline`, `library`, `closed` |
| `float` | `true`: the sidebar floats over the panes |
| `hover` | `x,y` in points: where the pointer rests |
| `wait` | frames before the shot, at 60 per second |
| `click` | accessible label to press after the wait; `_` for a space |
| `pause_read_aloud` | `true`: pause the player before the wait |
| `size` | `WxH` in points; output is at 2× |

To tune colours or sizes without recompiling, run a debug build with
`MAREADER_TOKENS=tokens.json` (token names → `"#rrggbb"` or numbers); it
re-applies when the file is saved, in the app and in `shot`.

Open the PNG and check it against `docs/design.md`. If something looks worse
than mareader, improve it and update the doc. Interactions belong in kittest tests (`crates/ui/tests/shell.rs`
shows the pattern: `Harness::builder().with_step_dt(1.0 / 60.0)`, `hover_at`,
`get_by_label(..).click()`).

## Working environment

- Builds are heavy: always pass `-j 2` (or what the machine allows) and a
  `timeout`. Scope to `-p <crate>`.
- Parallel agents use separate `CARGO_TARGET_DIR`s and git worktrees.
- Headless wgpu renders need a Vulkan driver; on Linux install
  `mesa-vulkan-drivers` (lavapipe), as CI does.
- Stock icons come from build-generated Lucide geometry. Resting glyph and
  animation-atlas textures stay neutral white and are tinted by `Icon::paint`.
- egui's named font families exist only from the second frame;
  `MareaderApp::ui` skips frame one for that reason.
