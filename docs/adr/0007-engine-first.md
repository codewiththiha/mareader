# ADR 0007: engine-first architecture

- **Status:** accepted, 2026-11-15
- **Context:** By v0.0.5 the workspace already kept its low-level domain crates
  (L0–L3) free of egui, yet application behaviour (settings mutations, pane
  tree ownership, document opens, reading places, library commands, marks and
  notes, read-aloud, study lookups, AI jobs and persistence) lived in the
  front-end crate (`crates/ui`, now `crates/ui-egui`). A headless test, script
  runner or second client could not drive the application without linking egui,
  eframe and wgpu. The engine-first architecture lifts that application layer
  into a toolkit-free `mareader-engine` crate (L5) while keeping `ui-egui` (L6)
  as a direct, immediate-mode egui front end rather than introducing a retained
  UI abstraction.
- **Decision:** Adopt an engine-first boundary governed by eight laws and eight
  limits.
  - **The eight laws:**
    1. **One owner.** `engine::Session` owns every piece of state that outlives
       a frame or reaches disk: settings, pane tree, focus, route, open
       documents and each pane's reading place, zoom, view mode and selection,
       the library, marks and notes, read-aloud, page-AI results, dictionary
       and vocabulary state, and running background jobs. `ui-egui` owns only
       window-ephemeral state (`UiState`: hover, open popovers, drags,
       animations, GPU textures and caches).
    2. **One door.** State changes enter `Session` only through
       `Session::execute(&mut self, Command)` (and `execute_id(id, params)` for
       textual callers). `Session` exposes no public mutable fields and no
       `_mut()` escape hatches.
    3. **Every command registered.** Every variant of `engine::Command` has a
       static `CommandSpec { id, label, params, enabled, parse }` row in
       `engine::SPECS`, enforced by an exhaustive `Command::id()`.
    4. **Two enums.** `engine::Command` holds every application action;
       `ui_egui::UiCommand` holds chrome-only actions (popovers, focus, window
       caption controls, OS file dialogs). `ui-egui` never matches over
       `engine::Command`.
    5. **No toolkit below L6.** No crate at L0–L5 (and never `apps/mareader-cli`)
       may depend on or name `egui`, `eframe`, `egui_extras`, `egui_kittest`,
       `winit`, `wgpu` or `rfd`, even in dev-dependencies. Time enters as
       `now: f64`, wakeups travel through `engine::Waker`, and geometry uses
       plain `Bounds` / `(f32, f32)` values.
    6. **The machine is a trait.** `engine::Host` isolates the five platform
       capabilities the engine needs: `state_dir`, `cache_dir`,
       `set_clipboard`, `reveal` and `open_external`. Interactive file pickers
       remain `UiCommand`s handled by the front end.
    7. **Results leave as events.** `Session::drain() -> Vec<Event>` reports
       toasts, title updates, document lifecycle notices and completion signals
       back to whichever client drives the session.
    8. **The engine saves itself.** `Session` writes `settings.json` and
       `session.json` atomically under `Host::state_dir()`. `eframe::Storage`
       is touched in exactly one place: the one-time v0.0.5 importer on launch.
  - **The eight limits:** no second GUI; no abstraction layer over egui; no
    visible regressions in the existing desktop shell; no async runtime added
    to `engine`; no pixel rasterisation below L6; no MCP/TCP transport daemon;
    L0–L3 crates unchanged in responsibility; no transitional shim outlives its
    phase.
- **Consequences:**
  - `apps/mareader-cli` proves the boundary in CI: it links `mareader-engine`
    with zero toolkit dependencies (checked transitively by
    `cargo xtask layers`), prints every registered command spec via
    `mareader-cli commands`, and runs five headless end-to-end scripts
    (`01-page-text` through `05-import-folder`) without a display or GPU.
  - Five automated gates lock the boundary permanently: `cargo xtask layers`
    (rank order and headless CLI tree), `cargo xtask wasm` (every L0–L5 crate,
    `ui-egui` and `mareader-web`, with `assist` as the sole documented
    exemption), `cargo xtask trapped` (zero toolkit-free files left in
    `crates/ui-egui/src` outside the two-file allowlist `lib.rs` and
    `icons/pack.rs`), the four-file pixel thread ceiling in `ui-egui`, and
    `cargo xtask parity` (every shipped parity entry must cite a registered
    `SPECS` command id or explicitly declare `ui_only = true`).
  - Adding a new application feature now starts in `crates/engine` with a
    `Command` variant, a `SPECS` row, unit tests against `Session` and a
    `parity/mareader-features.toml` link before any egui view renders it.
