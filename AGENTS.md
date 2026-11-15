# AGENTS.md

These rules bind every AI agent and every human contributor in this repository.
They come before any task, instruction or deadline. `CLAUDE.md` holds the
day-to-day working instructions; if the two disagree, **this file wins.**

## Project

mareader2 is a native rewrite of [mareader](https://github.com/codewiththiha/mareader)
(Tauri + Leptos + pdf.js) in Rust with egui and wgpu. The goal is every one of
mareader's capabilities — reading, panes, blend, appearance, library, study
tools — in a codebase that is smaller, faster, leaner on memory and easier to
maintain. It reads PDF (hayro) and EPUB (parley); EPUB replaces mareader's txt
and md formats.

- **Parity is functional** (ADR 0005). What a mareader user can do, they can
  do here, with equivalent settings. Pixels, timings and code structure are
  not part of parity.
- **The look is ours:** `docs/design.md`, starting from mareader's and free to
  improve.
- **Feature tracking:** every capability has an entry in
  `parity/mareader-features.toml`, checked by `cargo xtask parity`.
- **Architecture:** `docs/architecture.md` (read § How the app is built in
  egui before writing UI). Decisions: `docs/adr/`.

## The reference checkout

mareader is checked out beside this repository as `../mareader` (or at
`MAREADER_DIR`). It is read-only reference for *what* a feature does.

- **Rebuild from the goal, not the code.** For each feature, work out what the
  user gets and which edge cases matter, then design the smallest native model
  that delivers it (`docs/porting.md`). Don't port module layouts, workaround
  layers or web-only mechanisms; `docs/porting.md` § What not to port lists
  them. Don't add mareader's crates as dependencies.
- **Check claims about mareader against its source.** When a doc, test or
  parity note says "mareader does X", grep `../mareader` first. Memory has been
  wrong here before.
- **Read the comments and tests around a mareader feature.** They record the
  regressions it paid for; keep the ones that still apply to a native app.
- **Improve freely, drop deliberately.** A better design for the same outcome
  needs no permission; note it in the feature's doc. Removing something users
  rely on is the owner's decision.

## Assets

An asset is any non-code material the project ships, embeds or shows: icons,
images, fonts, sounds, sample documents and screenshots.

- Allowed licences only: MIT, ISC, BSD-2/3-Clause, Apache-2.0, OFL-1.1, CC0-1.0,
  CC-BY-4.0, or contributor-original under the project licence.
- **Every asset has an entry in `ATTRIBUTION.toml`:** path or crate, author,
  source with version, SPDX licence and SHA-256 for files. `cargo xtask assets`
  fails on an unlisted file, a wrong hash or a licence off the list.
- **Stock icons are Lucide**, compiled from build-only declarations into typed
  geometry behind `crates/ui/src/icons/` (ADR 0004). Code names an `Icon` by
  meaning; only `icons::pack` names a glyph. Custom provider and voice marks
  remain the documented static-SVG exceptions.
- **Fonts:** Inter in `assets/fonts`. A new face needs an OFL source and an
  `ATTRIBUTION.toml` entry. Never bundle a system font.
- If an asset's origin or licence is unclear, do not add it; ask the owner.

## Never crash

Every PDF, EPUB, settings file, drag payload and keystroke is untrusted. None of
it may crash the app or lose the user's place. In non-test code:

- No `unwrap`, `expect`, `panic!`, `unreachable!` or `todo!` unless provably
  infallible, with a comment saying why.
- No indexing or slicing with input-derived positions: use `get`, and slice
  strings only at char boundaries.
- Checked or saturating arithmetic on input-derived numbers. Guard division by
  zero and NaN or infinite casts (`f32::NAN.clamp(..)` stays NaN).
- Cap allocations sized by input; bound recursion with depth limits. EPUB adds
  zip bombs, deep XML, path escapes and huge images to the list.
- No `unsafe`: the workspace denies `unsafe_code`. The owner allowed six
  exceptions: `apps/mareader/src/traffic_lights.rs` and `open_files.rs` for
  AppKit; `crates/speech/src/mac_voices.rs`, which asks which listed voices
  have their sound; `crates/memory/src/lib.rs`, which hands freed memory
  back on macOS and Linux; and the twins `crates/kokoro/src/simd.rs` and
  `crates/vits/src/simd.rs`, which run a loop's AVX2 build only once the CPU
  is found to have AVX2 and FMA. `cargo xtask unsafe` fails if any other file
  allows it.
- Every crash fix gets a synthetic regression test.

## Layering

`cargo xtask layers` enforces the table in `docs/architecture.md`.

- Every crate marked Core in `xtask/src/layers.rs`
  never depends on egui, eframe, winit or wgpu. They are testable without a
  window.
- Only the front layer (`crates/ui`) draws. `apps/mareader` only opens the
  window.
- A new crate gets a row in the layer table in the same commit.

## Shape: one job per file, reuse before writing

### File and function size

- New files stop at 500 lines, tests included. Existing oversized files have
  ceilings in `tools/shape_baseline.toml`; they must not grow.
- Ceilings only go down; only SZ tightens the baseline. The declared data files
  in its `[exempt]` section are exempt; adding an exemption is an owner decision.
- New functions stop at 80 lines. Baseline functions must not grow.
- Move inline test modules over 150 lines into `foo/tests/<topic>.rs` files
  under 500 lines. Tests cited in the parity ledger stay at their paths.
- Matches over 12 arms for commands, options or properties only route to
  functions in child modules.

### Splitting and reuse

- Keep `foo.rs` as the facade; put private children under `foo/`. Split by
  responsibility, and use `pub(super)` at most for moved internals. The
  public API stays unchanged. Aim for a facade of at most 200 lines.
- Move-only commits do not rename items, alter signatures or change behavior;
  behavior changes follow separately. Avoid modules named `utils`, `helpers`,
  `misc` or `common`, except `tests/common/mod.rs`.
- Search for existing helpers first and consult `docs/reuse.md`. The commit body
  records the search. Reuse existing logic; extract on the second use. Do not
  add speculative type parameters, traits or options without two real callers.
- Shared feature code lives under its feature, cross-feature UI code under
  `crates/ui/src`, integration fixtures under `tests/common/mod.rs`, unit
  fixtures in `src/test_support.rs`, core logic in the lowest shared crate, and
  parity-tool code in `tools/parity-kit`. A new L0 crate needs owner approval.
  Index each shared item in `docs/reuse.md`.
- Delete dead code in the same commit. Avoid argument-forwarding-only functions
  and one-field types without an invariant.
- Commits changing more than 50 lines need a body describing what, why and
  verification. A `perf` commit records command, machine, before and after.
- `python3 tools/shape_check.py` checks tracked file and Rust function sizes;
  `--dups` reports matching bodies after the duplicate-removal slices. Its
  lightweight scanner is not a Rust parser, so review its results and rules
  it cannot enforce, including inline tests and routing matches.

## Code style

- Rust: default rustfmt (`max_width = 100`); clippy clean with `-D warnings`.
- Comments: see `### Docs and comments` below. It is a constraint, not an argument.
- No `TODO`/`FIXME`, no scaffolding for work not being done now, no
  speculative abstractions. A planned feature lives in the parity file, not as
  an empty function.
- One owner per responsibility. No duplicate state, no permanent
  compatibility layers, no unbounded fallbacks.
- No per-frame, per-page or per-scroll logging.

### Docs and comments

Docs (`///` items and `//!` modules) state contracts. The first line is one
sentence within 80 columns; after a blank doc line, item docs may use 12 lines
and module docs 20. State invariants, units, failure cases, ordering and at most
one example. Public core items need a first line; a private function under ten
lines gets at most one. Docs must not exceed the item they document. A
`comment-check: allow` marker on a doc is a reviewer-visible claim.

Ordinary comments are one line, at most 80 columns and 15 words, with no allow
marker. A diff earns one `//` line per 25 added code lines, with a floor of two;
a function has at most three. Comments state constraints code cannot show.
Remove narration, history, justification, restatement, filler, dividers,
commented-out code, TODOs and FIXMEs. Put reasoning in docs or commit bodies.
`tools/comment_check.py` checks blocks and `tools/comment_budget.py` checks a
diff; pass changed files explicitly. Review the rules the tools do not enforce.

## Resource rules

- Everything that allocates has an explicit owner and teardown: textures,
  render jobs, worker threads, caches, file watchers, timers. Dropping a view
  is not teardown unless the owner's `Drop` releases it.
- Closing a pane or a document releases everything it allocated: page
  textures, tiles, the document handle, its search index.
- Job results carry the document id, generation and zoom they were made for;
  stale ones are dropped at the channel edge, never uploaded.
- Every deferral has a guaranteed wake (`request_repaint_after`, a channel
  message, a timer); nothing may wait on an event that might not come. An idle
  window renders no frames.
- PDF pages render blank until the full-resolution raster lands; no
  low-resolution placeholders (mareader's rule).
- Caches are capped by bytes or entries, never unbounded.
- Memory claims need measurements: what, how, workload, before and after, and
  known limits. Record them in `docs/memory.md`.

## egui, natively

The UI is immediate mode. Write it as egui code, not as a web app in egui
syntax (`docs/architecture.md` § How the app is built in egui).

- **State lives in plain structs** owned by `MareaderApp` or by L0–L3 types.
  No signals, stores, observers or effects; no state mirrored in two places.
  egui `Memory` holds only transient widget state.
- **Views are functions** that take the state they show and push `Command`s.
  They never mutate app state directly; `MareaderApp::apply` is the one place
  commands take effect. Buttons, menus, shortcuts and drops share commands.
- **Use egui's containers and widgets:** `Panel`, `Popup`, `Modal`, `Area`,
  `ScrollArea::show_rows`, `Sides`, `Button`, `Slider`, `TextEdit`, drag and
  drop via `dnd_*`. Write a custom widget only where egui has nothing close,
  and give it an AccessKit label.
- **Every modal is a `modal::Dialog`:** centred, one card, one backdrop, with
  the shared heading, switch card and buttons. Never call `Modal::new`
  directly; a test fails if anything else does.
- **The theme is `Tokens` applied to `Style` and `Visuals`,** once per change
  (`docs/design.md` § How the theme works). Views don't pick colours or sizes;
  custom widgets read `Tokens`. Appearance changes show on the next frame and
  never re-render pages: page looks are uniforms of the page pass.
- **Don't recreate per-pane isolation.** A pane is a view over a shared
  `Arc<Document>`, one tile cache and one worker pool (ADR 0006).
- **Animate meaning, not decoration.** Hover is instant; motion goes through
  `motion::secs` so the Animations switch can turn it off.
- **Never block the UI thread.** I/O, parsing, raster and scans run on workers
  that report over channels and call `request_repaint`.

## Always checkable

Phases are never "done before visible". The owner checks work while a phase
is in progress, by looking and by using it.

- **Slice vertically.** Each slice adds one thing a person can see or do,
  end to end (model, view, command, test), and lands on `main` on its own.
  Don't stack invisible plumbing for a whole phase and reveal it at the end.
- **`main` always runs** on the desktop (`cargo run -p mareader`). This is a
  desktop app; a slice that breaks it isn't finished.
- **The browser preview is a viewer, not a target.** `tools/web.sh --serve 8080`
  runs the same `App` so the owner can look and click from a sandbox. It is
  best effort and has no CI job. Never shape desktop code for it: when a
  desktop-only piece (threads, mmap, file watching, the library database)
  doesn't build for wasm, stub it in `apps/mareader-web` or let the preview
  lag.
- **Every state is reachable by name.** New views get a `set_option` key, so
  the shot tool (`key=value`) and the browser preview (`?key=value`) can open
  them directly.
- **Be honest in the app.** A control whose effect lands in a later phase says
  so in a muted line, and its parity entry stays `partial` with notes.
- **Log each slice** in `ROADMAP.md` (what to try, where).

## Parallel slices

- Each slice owns the exact write-set in `docs/slices/BATCH.md`. Write-sets in
  the same batch must be disjoint. Stop and report required outside edits.
- Shared registries have one writer per batch; see the batch table. S0 is the
  opening slice; SZ alone tightens the baseline and folds slice reports into
  ROADMAP and `docs/reuse.md`.
- A slice reports what changed, what was verified, limits and seen-but-untouched
  issues in `docs/slices/<id>.md`; do not edit ROADMAP during parallel work.
- Branch parallel slices from S0 and do not merge or rebase another slice into
  them. Each slice must stand alone and keep parity evidence addresses intact.
- No drive-by edits. Moves and behavior changes are separate commits.

## UI work: look at the result

- **Every UI change is looked at, not assumed.** Render it with
  `cargo run -p mareader-ui --example shot -- OUT.png key=value…` (see
  `CLAUDE.md`), open the PNG, and check it against `docs/design.md`: light,
  dark and one tint, wide and narrow, and each platform's chrome the change
  touches. Say what was checked.
- **Behaviour is tested with egui_kittest** (`crates/ui/tests/`): hover, click
  and keyboard through AccessKit labels, at 60 fps steps
  (`with_step_dt(1.0 / 60.0)`).

## Review every update, at four zooms

Not an end-of-task pass: a habit per update, so a defect is never built on.

- **Clause:** checked arithmetic, `get()` not `[i]`, char-boundary slicing,
  `<` against `<=`, the 0-based/1-based page seam, points against pixels.
- **Function:** is it total? Name an input that panics; walk empty, zero, max,
  malformed. Is the return type honest?
- **File:** two sources of truth, dead code, is the public surface smallest,
  is the doc comment still true?
- **Project:** layering, teardown, idle frames, egui idioms, the parity entry
  and `docs/design.md` still true.

## Testing

- Never weaken an assertion or edit `.github/workflows/` to make a check pass.
  If one blocks the task, report the blocker.
- **Prove a test fails before the fix by reverting the fix.** A test nobody has
  seen fail is not evidence.
- Tests mirror the zooms: unit in the crate, integration in `tests/`, kittest
  for the shell.
- Document runtime changes cover open → use → close, close during a render,
  rapid open/close/reopen, reader ↔ library, several panes, and stale results
  after close.
- Reuse mareader's test cases where the behaviour is the same; cite them in
  the parity entry.

## Parity, honestly

- `shipped` means it exists and a test proves it: `evidence` cites
  `path::test_fn` in this repository, and the test exercises the feature, not a
  neighbour of it.
- `partial` and `dropped` say in `notes` what is missing or why.
- Update the entry in the same commit as the feature. A status that became
  untrue is a bug.

## Gates

Before every commit:

```sh
cargo fmt --all --check
cargo clippy -p <crate> -j 2 --all-targets -- -D warnings
cargo test -p <crate> -j 2
cargo xtask layers && cargo xtask parity && cargo xtask assets
cargo xtask comments
```

Scope builds and tests to the crates you touched, under a `timeout`. The full
workspace sweep (`cargo xtask ci`) is CI's job; run it locally only when the
machine is free.

## Where the gates run: local mode or CI mode

The gates are the same everywhere; only where they run differs. Details,
numbers and the full loop are in [docs/verification.md](docs/verification.md).

- **Local mode** (a capable machine): run the gates here, push when green,
  and **don't poll CI**. It repeats checks that already passed.
- **CI mode** (a sandbox or a low-end machine): compile nothing locally.
  Push a `<type>/<slug>` branch, then run `python3 tools/ci.py watch` until
  the verdict; merge into `main` once it is green. Take screenshots with
  `python3 tools/ci.py shots`, never with a local wasm build and browser.
- **CI discipline.** Install `tools/hooks/pre-push` for format, shape,
  comments, budget and commit checks. Never knowingly push a locally visible
  failure. The Gate job never compiles; lint, test and platform jobs follow it.
  Only main writes Rust build caches. Do not add sccache or SHA-based cache keys.
  Change pinned toolchain and cache-key environment deliberately. Report a
  `Rust / test` run over 12 minutes; read the whole failure before a fix push.
  Small core crates can be checked locally in CI mode if the owner permits;
  explicit owner instructions take precedence over local-build guidance.
- **Choosing**, highest precedence first:
  1. The user's explicit word, in either direction ("use CI" on a fast
     machine, "build locally" on a slow one).
  2. `MAREADER_VERIFY=local|ci`.
  3. `python3 tools/ci.py mode`, which detects RAM, cores and free disk.

  Say which mode you chose, and why, in the first report of a session.
- **After a sandbox reset,** and only if local work is wanted,
  `python3 tools/ci.py cache restore` unpacks a build that CI made, so the
  cold build is skipped. Every green push refreshes that cache on its own.
  It holds lint builds only (about 1.2 GB, a few minutes to restore); a
  local test run first needs `cache build --full`.
  If `restore` calls the cache stale, lint in CI, not locally.
- **CI time.** Keep timing claims measured in `docs/verification.md`. Install
  apt packages through `.github/actions/apt`. SZ updates timings after the
  revised pipeline has enough runs.

## Commits and pull requests

- Conventional commits: `type(scope): summary`, imperative, lower case, no
  trailing period, subject ≤ 50 characters — GitHub's own truncation sits near
  72, so 50 is what survives a log line, a UI row and a checklist cell intact.
  Enforced rather than remembered: `python3 tools/commit_check.py --install`
  writes a `commit-msg` hook that refuses an over-long subject, and
  `python3 tools/commit_check.py --range main..HEAD` audits history. The hook
  is per checkout, so a fresh clone installs it once; `--status` reports
  whether it is armed — git skips a non-executable hook in silence.
- Every commit builds alone, so `git bisect` never stops on a broken one:
  a change spanning crates lands in one commit, or in an order that builds.
  CI's `Build / each commit` job runs `cargo check` on every pushed commit.
- A squash merge makes the **PR title** the subject and appends ` (#NN)`, so a
  title must fit 50 characters *after* that suffix: 43 for a two-digit number.
  Check the merged form:
  `printf '%s (#12)\n' "$title" > /tmp/m && python3 tools/commit_check.py /tmp/m`.
- Types: `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `build`, `ci`,
  `chore`. Scope is the area touched (`shell`, `panes`, `pdf`, `epub`,
  `appearance`, `library`, …).
- One coherent change per commit; context goes in the body, not the subject.
  No progress reports, phase labels or ticket numbers in the subject.
- Work on a branch named `<type>/<slug>` and merge into `main` through a PR
  once CI is green. Commit only green states.
- Squash fixups before pushing; rewrite remote history only with
  `git push --force-with-lease`.
- Never commit credentials, tokens or personal documents.

## Definition of done

1. The change is implemented, and looked at (shots) if it touches UI.
2. The gates above pass and `CI` is green for the pushed SHA.
3. Docs describe the current behaviour: `docs/design.md` for the look,
   `docs/architecture.md` for structure, `docs/memory.md` for memory, and the
   parity entry for status.
4. Every added comment passes the diff-blind test (`### Docs and comments`) and
   `python3 tools/comment_check.py` is clean on the files the round changed.
   The round that adds a rule also applies it in the same round.
5. The summary states what changed, what was verified and any limits. If a
   requirement cannot be met, report the blocker instead of dropping it.
