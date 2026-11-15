# Mareader2 — repository analysis and review audit

**Analysed:** `feat/lab-icon-animations` @ `f58baa7` (2026-11-11), branch base `main` @ `bf2cd85`.
**Review audited:** https://01a1c8c6-da49-738a-9006-9ee46bf28d6b.arena.site/ (written at `bf2cd85`, 2026-11-07).
**CI at HEAD:** `CI` workflow **green** for `f58baa775` (Gate, Rust/lint, Rust/test, macOS, Windows).
**Verification mode:** `ci` — `tools/ci.py mode` reports 1.9 GiB RAM, 2 cores, 20 GiB free (local wants ≥8 GiB, ≥4 cores, ≥10 GiB). Nothing is compiled here.

---

## 1. What the repository is

A native Rust rewrite of *mareader* (Tauri + Leptos + pdf.js): one process, egui for the
interface, wgpu for the page pass, hayro for PDF, rbook + parley for EPUB. Ratified by
ADR 0001–0006. Parity is **functional**, not pixel or structural (ADR 0005).

| Measure | Value |
|---|---|
| Tracked files | 787 |
| Rust lines | 212,727 across 606 `.rs` files |
| Crates | 26 core + `ui` = 27, plus 2 apps and `xtask` |
| Core crates (no egui) | 184,015 lines |
| `crates/ui` src | 71,955 lines in 196 files |
| `crates/ui` tests + examples | 22,724 lines in 31 files |
| Other Rust (apps, tools) | 995 + 3,741 |
| Parity ledger | 124 entries: **122 shipped, 2 dropped**, 0 partial (P0 73 / P1 39 / P2 12) |
| Docs | 7,038 lines of Markdown in 27 docs + 6 ADRs |
| unsafe exceptions | exactly 6 files, all on the AGENTS.md audit list |

### Layering (`xtask/src/layers.rs`)

`Layer::Core(0..3)` → `Layer::Front` (rank **7**) → `Layer::Exempt` (rank MAX).
No crate is registered at L4/L5/L6; the two binaries and `xtask` are exempt from the gate.

### The 26 core crates, by responsibility

| Layer | Crate | Lines | Owns |
|---|---|---|---|
| L0 | appearance | 3,243 | look as data: base mode, tint, texture, grain, presets, colour maths, filter matrices, UI tokens, mark fills, pane looks |
| L0 | animated-icons | 10,445 | build-time Lucide/Quill declarations → typed geometry, timelines, tiny-skia atlases (no toolkit) |
| L0 | workspace | 901 | the pane split tree (≤4), dividers, drop edges, lift targets |
| L0 | fetch | 476 | mirror-in-turn resumable downloads, SHA-256 checked, kind-sniffed |
| L0 | audio | 1,709 | clip decode (symphonia), resample, same-pitch stretch, cpal device |
| L0 | memory | 51 | malloc_trim / malloc_zone_pressure_relief (the audited unsafe pair) |
| L0 | script | 551 | Boa JS sandbox over a page model, event/timer pump, DOM-ish tree |
| L0 | nn-core | 130 | SIMD dispatch, SplitMix64, protobuf wire primitives, error macro |
| L0 | kokoro | 4,818 | Kokoro-82M in candle (Full/Small/Tiny + Kitten ONNX), voices, chunks, prosody |
| L0 | g2p | 3,670 | misaki's English pipeline: spaCy tokenizer, perceptron tagger, lexicons, numbers, neural fallback |
| L0 | ipa | 5,665 | espeak-shaped IPA by own rules for es/fr/it/pt/hi |
| L0 | hanzi | 1,469 | jieba cut + pypinyin + misaki pinyin→IPA for Mandarin |
| L0 | vits | 3,290 | Piper VITS in candle straight from ONNX |
| L0 | phonon | 3,150 | Gradium Phonon 31M in candle: tokenizer, flow LM, Mimi decoder, normalisation |
| L1 | pdf | 3,181 | hayro: open/check, page sizes, rasters, tiles, text layer, links, outline, paper colour, title |
| L1 | reflow | 9,537 | parley blocks → pages, typography, hyphenation, pictures/SVG, vello_cpu raster, page text |
| L1 | library | 14,757 | SQLite store, books/positions, shelves (nested), arrange, folders/runs/gate, absorb (sync), removal, view |
| L1 | study | 603 | gloss rules, WordInfo, Explainer contract, page rewrite pieces |
| L1 | dict | 1,905 | packs, parquet→SQLite, lookups, search, bridges via English, service threads |
| L1 | cefr | 1,341 | dataset → SQLite, hard-word plan, role tagger, service threads |
| L1 | speech | 7,833 | sentences, Voicer trait, system voices, local-model engine, model shelf/downloads, pronunciation pack, word timing |
| L2 | epub | 22,091 | rbook container, guard, CSS cascade + calc/query, XHTML subset (split into 12 modules + 9 test files), generated content, decor, fonts, MathML, media, overlays/subtitles, triggers, script bridge, splice |
| L2 | settings | 3,265 | every setting typed, sanitize, dotted-path set, migration loader, AI/reader/study/text/words/workspace sections |
| L3 | session | 3,080 | open documents shared by panes, worker pool with per-frame demand + generations, doc/key/budget, typography typeset |
| L3 | assist | 4,318 | AI tasks, Backend seam (Apple / OpenAI-compatible), API keys, slots, page rewrite, organizer |
| L7 | **ui** | 71,955 | *everything else*: app state, reducer, every view, GPU passes, threads |

### `crates/ui` internal shape (196 files)

`app.rs` is already a **facade of 368 lines** (the S8 slice split it), with children
`app/{access, appearance, apply(+6 verbs), frame(+3), glosses, library, navigation,
options(+7), page_ai, panes, read_aloud, setup, study}.rs`. Other large families:
`library/` (24 files), `read_aloud/` (6), `page_ai/`, `dict/` (6), `study/` (8),
`settings_window/` (10), `toast/`, `dial_deck/`, `icons/`, `gpu/`, `pdf_view/`,
`word_list/`, `library/view/`, `thumbnails/`.

The reducer is `app/apply.rs` (74 lines) dispatching to `apply/{panes, appearance,
study, page_ai, read_aloud, library}.rs` — ~812 lines total, essentially the old
384-line `apply` spread over six verb files.

---

## 2. The review, summarised

**Thesis.** Mareader speaks the Craft dialect (never-crash rules, `cargo xtask layers`,
numbered layers, parity ledger, ATTRIBUTION, Lucide, hayro) except the one convention
that matters: there is **no engine crate**. Layers stop at `Core(3)` and jump to
`Front(rank 7)`. The app's single state owner is an `eframe::App` inside the egui crate.

**Seven findings at `bf2cd85` (all re-checked below):**

| # | Finding | Status at `f58baa7` |
|---|---|---|
| F1 | Layer table skips the engine floor; binaries exempt | **Still true.** `layers.rs` unchanged; `Front => 7`, `Exempt => u8::MAX` |
| F2 | One state owner is `MareaderApp: eframe::App`, holds `egui::Rect` | **Still true**, but `app.rs` is now a 368-line facade (was 4,082) |
| F3 | `apply` needs `&egui::Context`; 10 of 12 input reads are the clock | **Still true.** 61 `ctx.input` sites in `app/`; every `apply/*.rs` takes `&Context` and calls `ctx.input(\|i\| i.time)` |
| F4 | One `Command` enum mixes engine and chrome intents | **Still true.** Still 79 variants; imports view types (`Tab`, `Panel`, `Toast`, `ViewMode`, `Placement`) |
| F5 | 21 thread spawns in `ui`, each cloning `egui::Context` for repaint | **Still true.** Exactly 21 spawns across 19 files; 24 `ctx.clone()`; 145 `request_repaint` |
| F6 | Persistence through `eframe::Storage` | **Still true** (`app/frame.rs::save`, `app.rs::from_storage`) |
| F7 | `set_option` is a private string control channel (~140 keys) | **Still true**, now split across `app/options/*.rs` (1,142 lines) |

**The mandate.** One target layout (`engine` L5, `ui-egui` L6, `apps/*` L7, new
`mareader-cli` as the second client), **8 laws**, **8 limits**, **7 phases P0–P6**, each
ending at a machine gate, and a 10-item definition of done.

---

## 3. What changed since the review was written (the S-batch)

The review was authored at `bf2cd85`. Since then `feat/lab-icon-animations` carries:

1. `d670618` **ci: add review batch foundation** — shape/comment ratchets, duplicate
   scanner, pre-push hook, pinned Rust 1.99.1, gated CI, `docs/slices/BATCH.md` + `S0.md`,
   reuse index.
2. `11e85b2` **refactor: consolidate review batch S0–S12 and SZ** — merges twelve
   parallel slices (icons, animated-icons, settings/load, nn-core, parity tools, ui tests,
   library, app split, pdf_view, ui helpers, epub xhtml, reflow flow). Baseline tightened
   116→111 files, 95→80 functions; 11 duplicate groups listed; `--dups` still disabled;
   parity now validated 122/122.
3. `0eb6747` **Port the complete Quill animated icon set** — `animated-icons` gained
   `atlas.rs`, `raster.rs`, `timeline.rs`, `source_model.rs`, build split — 10,445 lines.
4. `f58baa7` **build: remove block future incompatibility** — vendored `block`.

**Consequence for the review:** the review's *measurements* are stale; its *findings are
not*. Every number in its § The evidence (70,012-line ui crate, 158 files, `app.rs`
4,082) predates the S-batch. Recomputed today: **71,955-line ui crate, 196 files,
`app.rs` 368**. The migration plan itself is untouched by the S-batch — P0 was never
started. There is **no `crates/engine`, no `ui-egui`, no `mareader-cli`, no `xtask wasm`,
no `xtask trapped`, no `Host`, no `Session::execute`** anywhere in the tree.

**Measurement drift worth knowing before slicing:**

| Review says | Today | Why |
|---|---|---|
| 70,012 lines in `ui` | 71,955 | Quill icon port + S-batch splits |
| 158 files, 6,398 lines never name a toolkit | 196 files, **53 files / 9,393 lines** never name one | module splits created more small toolkit-free files |
| 17 movable files / 3,361 lines (P1) | same 17 files all exist; 16 import nothing from `crate::`, 3 name a single `mareader_*` crate | S8/S10 added new toolkit-free files, but the P1 list itself is intact |
| 21 thread spawns, 19 files | identical | untouched |
| `apply` 384 lines | 812 in six verb files | moved, not reduced |
| 122 shipped parity entries | identical | untouched |

Two review statements are now **imprecise**:

- **P1's "move it untouched" rule** does not match the tree. The review's rule is *"a
  toolkit-free file whose `use` lines name exactly one `mareader_*` crate and nothing
  from `crate::` moves down into that crate; every other one moves to `engine` under the
  same module path."* Checked file by file at `f58baa7`: **16 of the 17 name no
  `crate::` import at all** (the exception is `transcript.rs`, which imports
  `crate::stable_hash`). But **only 3 name exactly one `mareader_*` crate** —
  `texts.rs`, `play.rs`, `library/picks.rs`. The other 13 name two to four
  (`paper.rs` names four), so under the review's own rule they all land in
  `mareader-engine`, and the "moves down into that crate" branch applies to almost
  nothing. The rule is workable; it just needs the destinations written out per file
  instead of derived.
- **The S-batch created new toolkit-free, engine-coupled files.** `app/options/*.rs`
  (1,142 lines) and `app/apply/*.rs` (812 lines) contain no toolkit name but sit on
  `MareaderApp`, `Command` and the controllers. They are not P1 material and must move
  in G2, not G1 — otherwise G1 would drag the reducer down with them.
- **"App behaviour testable without a toolkit: 1,193 tests in toolkit-free crates"** was
  a count at a point in time; the split does not change the *claim*, only the arithmetic.

---

## 4. Verified gaps, in dependency order

These are the things that are genuinely absent, ranked by what unblocks what:

1. **Layers.** `Core(5)` engine row, `Front` at 6, apps checked at 7, a `mareader-cli`
   row that may not link a toolkit. (`xtask/src/layers.rs`)
2. **Names.** `crates/ui` → `crates/ui-egui` / `mareader-ui-egui`; workspace dep key; both
   apps; `xtask` `SIDEWAYS`/`UI_CRATES` unchanged.
3. **Gates.** `xtask wasm` (L0–L5 + ui-egui + web on wasm32) and `xtask trapped`
   (toolkit-free files in `ui-egui` beyond an allowlist) do not exist; `remote.yml` still
   carries `|| echo "::warning::the wasm build has lint errors (best-effort)"` at line 787.
4. **Owner.** `Session` (settings, tree, active, recent, route, documents model, library,
   looks, texts, staged, last document) lives in `MareaderApp` and is mutated in place by
   views through `apply`.
5. **Door.** `execute(Command)` with no `&Context`: time as `now: f64`, focus as an
   `Event`, viewport as a `UiCommand`, repaint as the existing
   `Waker = Arc<dyn Fn() + Send + Sync>` alias (already exported by `speech`, `assist`,
   `dict`, `cefr` — the seam exists, the app just doesn't use it consistently).
6. **Registry.** No `CommandSpec`/`SPECS`/`Command::id()`; nothing enumerates the 79
   variants, so no `mareader-cli commands` output is possible.
7. **Split enums.** `Command` still carries `ToggleSidebar`, `Window(WindowCommand)`,
   `SetTitlebarPinned`, `ShowViewMenu`, `ShowAppearance`, `CloseToast`.
8. **Machine seam.** No `Host` trait (`state_dir`, `cache_dir`, `set_clipboard`,
   `reveal`, `open_external`); `reveal.rs`, `files.rs`, `library/io.rs` do their own.
9. **Persistence.** `eframe::Storage` is the only store; no `settings.json` /
   `session.json`, no atomic sibling-rename writer for them (the library already has
   `disk::write_atomic` — reusable).
10. **Second client.** Nothing can open a book without a window.

**What is already in place for the migration (do not rebuild):**

- The `Waker` closure pattern is the house style in four crates.
- `mareader_session::Session` already speaks in `Event`s and `Budget`s — it is the model
  the app-level engine should copy.
- `library::disk::write_atomic` gives the atomic write the review asks for.
- `settings::{load,to_json,set_path}` already parse per section with notes, so the
  importer and the dotted-path command channel are nearly free.
- `app.rs` is already a 368-line facade and `app/apply.rs` already routes per verb — the
  move to `Session::execute` is a rename-and-thread-`now` job, not a rewrite.
- `xtask` already has a clean `COMMANDS` table one line per gate — `wasm`/`trapped` slot in.

---

## 5. Proposed slice plan (branch-per-slice, all from `feat/lab-icon-animations`)

The review's phases assume one worker doing P0→P6 in order. With parallel agents the
safe decomposition is: **one foundation slice first** (it owns every shared registry),
then slices whose write-sets are disjoint, then a lock step. Branch names say what they do.

| Slice | Branch | Write-set | Depends on | Exit gate |
|---|---|---|---|---|
| **G0** foundation | `feat/engine-g0-fence` | `Cargo.toml`, `crates/engine/**` (new), `crates/ui` → `crates/ui-egui` move, `xtask/src/{layers,main}.rs`(+`wasm.rs`,`trapped.rs`), `apps/*` manifests, `.github/workflows/remote.yml`, `docs/architecture.md` | — | `cargo xtask layers` shows engine L5 / ui-egui L6 / apps L7; `xtask wasm` and `xtask trapped` exist and pass; trapped count committed |
| **G1** evacuate | `refactor/engine-g1-evacuate` | the ~17 toolkit-free files + their tests into `engine` (same module paths) | G0 | `cargo test -p mareader-engine` runs moved tests; `cargo tree -p mareader-engine` lists no toolkit |
| **G2** reducer | `refactor/engine-g2-reducer` | `engine/src/{session,command,route,event,specs}.rs`, `ui-egui/src/app{,/apply,/options}`, `command.rs`, `route.rs`, `toast.rs` | G0, G1 | `rg 'egui\|eframe' crates/engine` empty; a windowless script test opens→splits→pages→highlights→exports; `command.rs`/`route.rs` gone from `ui-egui` |
| **G3** controllers | `refactor/engine-g3-controllers` | `documents.rs`, `read_aloud/{mod,spoken,booth}`, `page_ai/{mod,follow,groups,cards}`, `study/mod.rs`, `search.rs`, `files.rs`, `library.rs`, `library/{io,folders}`, `dict/mod.rs`, `vocab.rs`, `sounds.rs`, `word_list/mod.rs`, `page_copy.rs`, `ai.rs`, `scripts.rs` | G2 | thread spawns in `ui-egui` drop to the 4-file pixel allowlist; `rg request_repaint crates/engine` empty; trapped count 0 |
| **G4** host | `feat/engine-g4-host` | `engine/src/host.rs`, `engine/src/save.rs`, `apps/mareader/src/main.rs`, `ui-egui/src/app/setup.rs` | G3 | `eframe::Storage` only in the importer + its fixture test; interrupted-save test; `set_option` keeps UI-only keys |
| **G5** cli | `feat/engine-g5-cli` | `apps/mareader-cli/**` (new), `xtask/src/layers.rs` (cli row), 5 script fixtures | G4 | `cargo tree -p mareader-cli` no toolkit; five headless scripts green on Linux CI |
| **G6** lock | `docs/engine-g6-lock` | `AGENTS.md`, `CLAUDE.md`, `docs/architecture.md`, `docs/adr/0007*`, `parity/mareader-features.toml` (`command=`/`ui_only=`), `ROADMAP.md` | G5 | every definition-of-done line holds on `main` |

**Conflict notes for the coordinator**

- G0 and G6 are the only slices that touch **AGENTS.md**, the parity ledger and
  `ROADMAP.md`. Nothing else may.
- G2 and G3 are the two big ones and **must not run concurrently**: G3's files call
  G2's `Session`. G3 can start as a *read-only* preparation after G2 pushes.
- Everything else (G0, G1 after G0, G4 after G3, G5 after G4) is sequential in practice
  because each produces types the next consumes. Genuine parallelism exists only if you
  split G3 by feature family (read-aloud / page-AI / library / dict-vocab), which needs
  G2 first and a per-family write-set agreement.
- `feat/lab-icon-animations` is the base for every branch; slice branches must never
  merge each other — the coordinator merges into one branch at the end and runs the one
  CI pass.

---

## 6. Honest limits of this analysis

- **Nothing was compiled or executed** except Python gates (`shape_check`, `ci.py mode`,
  `ci.py watch`). Rust conclusions come from reading source and `cargo metadata`-free
  static inspection.
- **Every `.rs` file's module doc was read**, plus the crate roots, `xtask`, workflows,
  the two binaries, `app.rs` and its whole child tree, `command.rs`, `route.rs`, and the
  docs listed in § 1. Deep bodies of the biggest files (`epub/src/css.rs`,
  `animated-icons/src/source.rs`, `reflow/src/flow.rs`, `ipa/*`) were sampled, not
  line-by-line; their contracts come from their module docs and public surfaces.
- The `ipa` / `g2p` / neural-voice crates are outside the engine mandate and were read
  only to confirm they are toolkit-free and stay where they are.
