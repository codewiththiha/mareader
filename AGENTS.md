# AGENTS.md

## Project

MaReader is a desktop document reader for PDF, Markdown and plain text:
a Tauri v2 shell, a Rust/WebAssembly UI in Leptos (CSR), and pdf.js
vendored at `public/vendor/pdfjs`.

- The Shell (`src/`) owns routing, persistence and the runtime manager.
- Library and Reader workspace chrome run in separate disposable route
  iframe/WASM artifacts. Shell links neither runtime. Both routes are removed
  on departure; neither prewarms/recycles behind the other. Library return
  creates a fresh Library realm and removes Reader plus every document realm.
- Each document pane has its own iframe, JS realm and WASM instance;
  removing that frame releases its document runtime. PDF and reflow panes
  are separate artifacts selected by explicit Cargo features.
- The reader (`crates/reader-runtime`) hosts up to four panes. Each PDF
  pane owns its pdf.js engine and worker (`public/engine/`,
  `public/pdfEngine.ts`); text panes never link or load that engine.

Read `docs/architecture.md` before changing runtime, pane, engine or memory
behaviour. Read `docs/memory/rules.md` before writing code that allocates
surfaces, caches, timers or workers.

## Commands

CI (GitHub Actions) is the build. Do not run `cargo build`, `trunk` or
large installs locally. Run the fast checks that match what you touched:

- Engine TypeScript: `npx tsc --noEmit -p tsconfig.json`
  (install with `npm ci --ignore-scripts` if `node_modules` is missing)
- Engine behaviour and teardown:
  `node tools/bundle-engine.mjs && node scripts/test-engine-smoke.js`
- Doc paths in `README.md`, `Mareader.md` or Rust comments:
  `node scripts/check-doc-paths.js`
- Session ownership docs: `node tools/check-session-ownership.mjs`
- Host boundary: `node tools/check-host-boundary.mjs`
- Browser suite syntax: `node --check tests/browser/lifecycle.mjs`

After pushing, find the workflow run for the pushed SHA (not the branch's
latest run) and wait for `CI` to finish. Wait for `Deep CI` when that push
started one — the rule for whether it should is under "Deep CI" below. Fix and
repeat until every lane that ran is green. `docs/**`-only pushes trigger no
run.

## Deep CI

`Deep CI` answers one question: does everything a change allocated come back?
It boots the real wasm app in a real browser and drives open → scroll → zoom →
close-during-work → dispose → reopen against the disposal baseline
(`docs/memory-baseline.md`), replays a split read in Chromium and WebKit, and
boots the Tauri window under Xvfb: three jobs capped at 45, 75 and 45 minutes.

It runs on a push only when a path listed in `.github/workflows/deep-ci.yml`
changed. When it would run and the change cannot move a byte, a wake or a
release, put `[skip deep]` in the subject of the LAST commit of the push: the
workflow reads that subject and nothing else (a body quoting the marker changes
nothing), and the marker skips all three jobs. The nightly cron ignores it and a
`workflow_dispatch` run can force one with `ignore-skip`, so a skip is never
the last word.

- Skip it for presentation and prose: docs, release notes and comments; copy,
  labels, spacing, a control's placement or visibility, `styles/**`, and the
  menu or settings rows that only read and write an existing signal; AI,
  toolbar and shelf-surface presentation; `tools/**` scripts that neither build
  nor gate artifacts; pure logic in `reader-core`, `pdf-core`, `md-core`,
  `txt-core`, `ui-geom` and `ai-core`, whose answers `CI`'s `cargo test` gives.
- Never skip it for anything that allocates, retains, counts or releases:
  `public/**`, `src/**`, `src-tauri/**`, `crates/reader-runtime/**`,
  `crates/library-runtime/**`, `crates/frame-transport/**`,
  `crates/runtime-contract/**`, `crates/pdf-engine/**`, `crates/pdf-paper/**`,
  `crates/app-state/**`, `crates/virtual-list*/**`, `crates/tauri-bridge/**`
  and the disposal counters in `src/diagnostics.rs` and
  `crates/reader-runtime/src/diagnostics.rs`; a new cache, timer, observer,
  listener, queue, canvas or
  worker — or a change to when one dies; a retention policy, a cap or a
  ceiling; a build, staging, artifact or boot change (`tools/build-dist.sh`,
  `tools/dev.mjs`, `*.Trunk.toml`, `index.html`, `package.json`); a change to
  `tests/browser/**`, `tools/engine-smoke/**` or `.github/workflows/**`; and
  any push that follows a red `Deep CI`. In `crates/app-ui/**` and
  `crates/app-chrome/**` the marker covers a control's LOOK only: a diff that
  adds a listener, observer, memo, registry entry or effect is an owner and runs
  the deep lane.
- A skip is a claim, not a shortcut: name in the summary which list the change
  fell in, and say that nightly will see it. When the lists disagree, or an
  owner is touched by a change that does not read like memory work, run the
  lane. Never add the marker to dodge a failure, and never edit the workflow's
  path list to avoid a run — narrowing a gate is a decision for the repository,
  not for a task.

## Code style

- Rust: default rustfmt (`max_width = 100`); clippy must be clean.
- Comments explain why, invariants or lifecycle; never restate code.
- No `TODO`/`FIXME`, no scaffolding for work not being done now, no
  speculative abstractions.
- One owner per responsibility. No duplicate state, no permanent
  compatibility layers, no unbounded fallbacks.
- No per-frame, per-page or per-scroll logging.

## Resource rules

- Everything that allocates has an explicit teardown: tasks, render queues,
  workers, page registrations, canvases, caches, virtualizers, observers,
  event listeners, timers and idle callbacks. Unmounting is not teardown.
- Closing a pane or a document releases everything it allocated.
- Canvases are released by zeroing the backing store (`releaseCanvas`,
  `remove_snapshots`); DOM removal alone does not free them.
- Module-level registries hold sessions by `WeakRef` and drop them first in
  teardown. Engine lanes drop dead work at the queue edge and the rAF edge.
- Every deferral has a guaranteed wake (settle, timer or trigger); nothing
  may wait on an event that might not come.
- PDF pages render blank until the full-resolution raster lands; no
  thumbnail or low-resolution placeholders.
- Memory claims need measurements: what, how, workload, before/after and
  known limits. Record audits in `docs/memory/audit.md`.

## Testing

- Never weaken an assertion or edit `.github/workflows/` to make a check
  pass. If one blocks the task, report the blocker.
- Runtime changes cover open → use → dispose, dispose during async work,
  rapid open/close/reopen, reader ↔ library transitions, multiple panes and
  stale-result rejection after disposal.
- UI changes are verified visually in a running browser at desktop and
  narrow widths; say what was checked.
- Preserve existing behaviour (virtualization, look-ahead, retention, zoom,
  appearance, search, selection, AI features). Read the comments around a
  rendering path before changing it; they record past regressions.

## Commits and pull requests

- Conventional commits: `type(scope): summary`, imperative, lower case, no
  trailing period, subject ≤ 72 characters. Check with
  `git log -1 --format=%s | awk '{print length}'` before pushing.
- Types: `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `build`, `ci`,
  `chore`. Scope is the area touched (`pdf`, `split`, `shell`, `engine`, …).
- One coherent change per commit; context goes in the body, not the subject.
  No progress reports, phase labels or ticket numbers in the subject. The one
  non-descriptive token a subject may carry is `[skip deep]` (see "Deep CI"),
  and the 72-character check counts it.
- Squash fixups before pushing; rewrite remote history only with
  `git push --force-with-lease`.

## Definition of done

1. The change is implemented, and visually verified if it touches UI.
2. Matching local checks pass and `CI` is green for the pushed SHA; `Deep CI`
   is green for it as well unless the push carried `[skip deep]` under the rules
   above, which the summary states either way.
3. Docs describe the current behaviour (`docs/architecture.md`,
   `docs/memory/` for memory behaviour).
4. The summary states what changed, what was verified and any limits. If a
   requirement cannot be met, report the blocker instead of dropping it.

<!-- // only the changed file was rewritten -->
