# Developing Mareader

Contributor guide for building, testing, adding commands, and visually verifying
Mareader. Binding engineering rules live in [`AGENTS.md`](../AGENTS.md); if
the two disagree, `AGENTS.md` wins.

## Repository layout

| Path | What |
|---|---|
| `apps/mareader` | Desktop binary (L7): opens the window, supplies desktop `Host`, runs `MareaderApp` |
| `apps/mareader-web` | Browser preview (L7): `MareaderApp` via eframe `WebRunner` with a web `Host` |
| `apps/mareader-cli` | Headless CLI client (L7): `commands`, `exec`, `run`, `inspect` over `mareader-engine` with zero toolkit deps; CI scripts in `scripts/` |
| `crates/engine` | The app without a window (L5, ADR 0007): `Session`, `Command`, `SPECS`, `Event`, `Host`, atomic `settings.json`/`session.json` persistence, open documents, library, marks, read-aloud and study controllers |
| `crates/ui-egui` | egui front end (L6): theme, icons, title bar, panes, shell, GPU textures and window-ephemeral `UiState` |
| `crates/appearance` | Base modes, tints, filters, presets, UI tokens (no egui) |
| `crates/workspace` | Pane split tree and layout geometry (no egui) |
| `crates/settings` | User settings: schema, defaults, ranges, migration, mareader import (no egui) |
| `crates/engine/src/command.rs` | `Command`: every application action, applied by `Session::execute` |
| `crates/engine/src/command_spec/` | `SPECS`: static registry mapping every `Command` variant to its id, label, params, enabled check and parser |
| `crates/engine/src/host.rs` | `Host`: the five-method platform seam (`state_dir`, `cache_dir`, `set_clipboard`, `reveal`, `open_external`) |
| `crates/ui-egui/src/ui_command.rs` | `UiCommand`: chrome-only actions, applied in `MareaderApp::apply` |
| `xtask` | Workspace gates: `layers`, `wasm`, `trapped`, `parity`, `assets`, `unsafe`, `comments`, `ci` |
| `parity/mareader-features.toml` | Feature parity ledger: status, test evidence, and `command` / `ui_only` links |
| `docs/` | Architecture, ADRs, design system, format pipelines, memory audits and verification guides |
| `tools/` | Verification, static checks (`shape_check`, `comment_check`, `commit_check`), CI driver and voice-model parity tools |

## Common commands

```sh
cargo run -p mareader                       # desktop app
cargo run -p mareader-cli -- commands       # list every registered engine command
cargo test -p mareader-cli --test scripts   # headless CLI end-to-end scripts
tools/web.sh --serve 8080                   # browser preview, best effort (?panes=2)
cargo test -p mareader-ui-egui -j 2         # one crate's tests
cargo clippy -p mareader-ui-egui -j 2 --all-targets -- -D warnings
cargo xtask parity                          # feature parity report (checks SPECS)
cargo xtask parity --list planned           # remaining planned entries
cargo xtask layers                          # dependency rules (incl. headless CLI tree)
cargo xtask wasm                            # L0-L5, ui-egui and web for wasm32
cargo xtask trapped                         # toolkit-free files in ui-egui (must be 0)
cargo xtask assets                          # ATTRIBUTION.toml verification
cargo xtask unsafe                          # audited unsafe allowlist check
cargo xtask comments                        # comment rule on changed files
cargo xtask ci                              # full workspace verification sweep
python3 tools/shape_check.py --dups         # file/function size & duplicate checks
python3 tools/commit_check.py --install     # install commit-msg length hook (once per clone)
python3 tools/ci.py mode                    # detect local, cache or ci verification mode
python3 tools/ci.py watch                   # quick CI verdict on pushed branch head
python3 tools/ci.py final                   # full CI matrix on branch head
```

`cargo xtask parity` checks `reference` paths against `../mareader` (or
`MAREADER_DIR`) when the checkout is present, and notes when it is absent.

## Adding a feature or command

1. **Start in `crates/engine` (L5)** for any durable state or application
   behaviour (see `AGENTS.md` § Layering and ADR 0007). Put pure format or
   domain rules in the lowest L0–L3 crate that fits.
2. **Register a `Command`** in `crates/engine/src/command.rs` and add its
   static `CommandSpec` row (`id`, `label`, `params`, `enabled`, `parse`) in
   `crates/engine/src/command_spec/`. Handle it inside `Session::execute` and
   emit any user-visible outcomes through `Event`.
3. **Keep chrome-only actions in `UiCommand`** (`crates/ui-egui/src/ui_command.rs`)
   for popovers, window captions, focus and OS file pickers.
4. **Write tests first:** unit tests against `engine::Session` (no window),
   headless CLI scripts where end-to-end workflows apply, and `egui_kittest`
   tests in `crates/ui-egui/tests/` for interactive UI behaviour.
5. **Update `parity/mareader-features.toml`** with the test evidence and
   `command = "<id>"` (or `ui_only = true`), plus `docs/design.md` or
   `docs/architecture.md` when visuals or boundaries change.

## The look loop

The shell renders headlessly so UI changes can be inspected without a display:

```sh
cargo run -p mareader-ui-egui --example shot -- /tmp/a.png \
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

Open the PNG and check it against `docs/design.md`. Interactions belong in
kittest tests (`crates/ui-egui/tests/shell.rs` shows the pattern:
`Harness::builder().with_step_dt(1.0 / 60.0)`, `hover_at`,
`get_by_label(..).click()`).

## Working environment

- Builds are heavy: pass `-j 2` (or what the machine allows) and a `timeout`,
  and scope builds/tests with `-p <crate>`.
- Parallel agents or workspaces use separate `CARGO_TARGET_DIR`s and git
  worktrees.
- Headless wgpu renders require a Vulkan driver; on Linux install
  `mesa-vulkan-drivers` (lavapipe), as CI does.
- Stock icons come from build-generated Lucide geometry (`crates/animated-icons`).
  Resting glyph and animation-atlas textures stay neutral white and are tinted
  by `Icon::paint`.
- egui's named font families exist only from the second frame;
  `MareaderApp::ui` skips frame one for that reason.
