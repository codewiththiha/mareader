# AGENTS.md

## Project

MaReader is a desktop document reader for PDF, Markdown and plain text:
a Tauri v2 shell, a Rust/WebAssembly UI in Leptos (CSR), and pdf.js
vendored at `public/vendor/pdfjs`.

- The Shell (`src/`) owns routing, persistence and the runtime manager.
- Library and Reader workspace chrome run in separate disposable route
  iframe/WASM artifacts. Shell links neither runtime; Library return removes
  the Reader host and every document realm, with no Reader prewarm/recycle.
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
latest run) and wait for `CI` and `Deep CI` to finish. Fix and repeat until
both are green. `docs/**`-only pushes trigger no run.

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
  No progress reports, phase labels or ticket numbers in the subject.
- Squash fixups before pushing; rewrite remote history only with
  `git push --force-with-lease`.

## Definition of done

1. The change is implemented, and visually verified if it touches UI.
2. Matching local checks pass, and `CI` and `Deep CI` are green for the
   pushed SHA.
3. Docs describe the current behaviour (`docs/architecture.md`,
   `docs/memory/` for memory behaviour).
4. The summary states what changed, what was verified and any limits. If a
   requirement cannot be met, report the blocker instead of dropping it.
