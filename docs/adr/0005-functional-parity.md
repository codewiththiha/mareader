# ADR 0005: Functional parity, native architecture

- **Status:** accepted (owner), 2026-10-13
- **Context:** mareader2 began by matching mareader value for value: CSS sizes,
  timings and even quirks such as the sepia tint turning the chrome magenta.
  The owner's goal is different: the same capabilities, delivered by a cleaner,
  more efficient, more maintainable codebase that is properly egui. mareader is
  ~100k lines of Rust across Tauri, Leptos and pdf.js; a large share solves
  browser problems (iframes per pane, frame transport, reactive signals, DOM
  measurement, pdf.js memory latches) or carries accidental complexity (the
  library's departures, tombstones and per-level name conflicts).
- **Decision:**
  - **Parity is functional.** A feature is matched when a mareader user can do
    the same thing with the same result: read, arrange panes, blend, tint, file
    books, find them again, gloss a word. Settings that exist in mareader exist
    here, with equivalent effect.
  - **The look is ours.** `docs/design.md` owns it. mareader's palette and
    proportions are the starting point; any change that reads better, fits
    egui better or removes a quirk is welcome. A changed behaviour users rely on
    is still a decision for the owner.
  - **The structure is ours.** For each area, find what mareader's code is *for*
    (its goals and edge cases worth keeping) and design the smallest native model
    that delivers it. Do not port module layouts, workaround layers or web-only
    mechanisms. `docs/porting.md` lists what not to port.
  - **egui idioms.** State in plain structs, views as functions, commands as the
    only way to change state, egui's own containers and widgets themed through
    `Style` (`docs/architecture.md` section How the app is built in egui).
- **Consequences:**
  - `parity/` entries describe outcomes, not CSS values. `reference` paths
    point at the mareader code that defines the behaviour.
  - The sepia tint no longer detours through magenta: a tint gives every token
    its hue directly, gamut-mapped (`crates/appearance/src/tokens.rs`).
  - The library is redesigned (`docs/library.md`) rather than ported.
  - Compatibility with mareader's files (settings, library) is an optional
    import (P2), not a constraint on our formats.
