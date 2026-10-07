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
- Gate or watch CI for a revision: `python3 tools/ci_watch.py` (`--once` reads
  once, `--detach` watches in the background, `--tail` reads it back)
- Enforce the commit-subject limit: `python3 tools/commit_check.py --install`
- Audit comment length: `python3 tools/comment_check.py <file or directory>`
  (directories recurse; `--json` for the payload, `--strip` to remove what
  owns its lines)

`tools/` holds the hand-written tooling; `scripts/` is generated — it is
`tsconfig.tools.json`'s `outDir`, so nothing authored belongs there and `.gitignore`
ignores the directory wholesale on purpose.

Keep the cheap checks in the editor and the expensive ones in CI. What to wait
for after a push — and what not to — is in "The CI loop". `docs/**`-only pushes
trigger no run.

## The CI loop

`CI` is the fast lane — `Rust / format`, `Rust / lint`, `Rust / test`,
`Web / contracts`, `macOS / shell`, in parallel, two to four minutes (measured:
2.8 green, 1.8 to a red format job). `Deep CI` is the slow one — three jobs
capped at 45, 75 and 45 minutes, twenty minutes even when all is well — and its
verdict cannot be read while `CI` is red, because one build break fails all three
jobs for a reason `cargo test` already states. So each commit decides which lane
owes an answer and which owes nothing.

### Which lane a commit owes

| The commit | `Deep CI` | Wait for |
| --- | --- | --- |
| a `cargo fmt` hunk, whitespace, a blank line | `[skip deep]`, always | `Rust / format` — that job *is* the check |
| comments, doc text, a rename no caller can see | `[skip deep]` | `CI` |
| `docs/**` only | no run at all | nothing — say the tree is unchanged |
| root prose (`AGENTS.md`, `README.md`) | no run | `CI` |
| logic in `*-core`, `ui-geom`, `ai-core`, `tools/**` | `[skip deep]` | `CI`, `cargo test` included |
| anything on the never-skip list below | on the round's last code commit, once | `CI` green, then that run |
| a version bump, a build or workflow change, a push after a red `Deep CI` | on that head | both, in that order |

Housekeeping commits are never the round's gate. `deep-ci.yml` filters `push` by a
source-path allowlist, and a `.rs` file whose only change is a blank line matches
it, so the marker is the only thing that stops a twenty-minute lane from checking
nothing — and `CI` still proves it.

### Reading a verdict without losing the afternoon

- **Poll only `CI` while the round is open:** `python3 tools/ci_watch.py --once`,
  which resolves the branch head (or an explicit SHA), reads the runs gating that
  exact SHA, and exits 0 green, 1 red, 2 still running — so a poll is one cheap
  command, and `--json` gives the same to a script. `--final` adds `Deep CI`.
  It digests the red job's log, the only place rustfmt's diff and clippy's
  message exist (`GET /actions/jobs/:id/logs`, one run per workflow: the newest
  decides, so a `[skip deep]` run beside a dispatched one cannot confuse it).
  Anonymous reads allow 60 requests an hour; pass `--token-file` for more.
- **Stop at the first red `CI` job.** The diagnosis is in hand; the rest cannot
  change what to fix.
- **Never block on a `Deep CI` verdict** — not on a sleep, not on a poll loop, not
  on a tool call that waits for a run that size. Run
  `python3 tools/ci_watch.py --detach --final`, do the owed work (notes, docs, the
  next fix), then `--tail` once: it prints the log and carries the verdict as its
  exit code, and `--stop` ends a watcher nobody needs. If the turn has nothing
  left, end it with the run live and name the SHA that owes the verdict; the next
  turn reads it. A long lane is only expensive when it is spent waiting.
- **Cancel a `Deep CI` run that gates nothing** instead of watching it:
  `POST /actions/runs/:id/cancel`. That covers a marker-less whitespace head, a
  dispatch against a stale SHA, and any live run whose SHA has since gone red in
  `CI`. Push the fix; the round's final head runs the lane again.
- **Do not dispatch `Deep CI` onto a ref whose push-triggered run is still queued
  or live**: the per-ref concurrency group settles that by cancelling one of the
  two, and a cancelled run is not a verdict — the round loses its gate for
  nothing. Let the push-triggered run be the gate, or dispatch once `CI` has
  landed. Say in the summary which of the two it was.
- **`Deep CI` runs once per round**, on the last commit that carries code, after
  every `CI` lane on that SHA is green. `docs/**` triggers no lane, so a closing
  prose push can be the round's literal last commit and never its gate; a
  follow-up that moves only whitespace or prose under an already-gated tree does
  not reopen it — the summary says so instead of paying for another rebuild.
- **Dispatch the final run rather than pushing for it**, because a prose or empty
  commit starts nothing at all:

  ```sh
  curl -sf -X POST -H "Authorization: token $TOKEN" \
    -H "Content-Type: application/json" \
    -d '{"ref":"<branch>","inputs":{"lanes":"both"}}' \
    https://api.github.com/repos/OWNER/REPO/actions/workflows/deep-ci.yml/dispatches
  ```

  A dispatched run reports its check on that head, so it gates exactly the tree
  under review. `ignore-skip` covers a head whose subject still says `[skip deep]`;
  drop the marker instead when you can.
- **A red `Deep CI` on the gated SHA is fixed like any other red**: read the
  failing job, fix, push — and that push is the round's final one, so it runs both
  lanes. `CI` is never skipped, and a push after a red `Deep CI` never carries the
  marker.

Never shorten this loop by narrowing a gate: no edited workflow path list, no
dropped assertion, no `[skip deep]` on the reported SHA.

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
the last word. Deferring the lane to a round's final commit is the marker's only
other legitimate use, and it is described in "The CI loop".

- Skip it for presentation and prose: docs, release notes and comments; a
  `cargo fmt` hunk, whitespace or a blank line inside a listed path — the
  file's name does not decide, the diff does; copy,
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

- Rust: default rustfmt (`max_width = 100`); clippy must be clean. One-line
  joining inside a function-like macro call (`assert_eq!`, `write!`) is gated by
  `attr_fn_like_width` — 60 by default — so a 98-character `assert_eq!` is still
  a diff: let the format job's own hunks decide, and replay them verbatim.
- Comments: see [`### Comments`](#comments) below — a constraint, not an argument.
- No `TODO`/`FIXME`, no scaffolding for work not being done now, no
  speculative abstractions.
- One owner per responsibility. No duplicate state, no permanent
  compatibility layers, no unbounded fallbacks.
- No per-frame, per-page or per-scroll logging.

### Comments

Write clean, self-documenting code. Do not write comments that explain what the
code is doing; only write comments explaining why something non-obvious was done.
Avoid repeating variable names in comments.

A comment carries a constraint the code cannot show; it never argues for a
decision. Test every added line: **would a reader who never saw the diff need
this?** If not, cut it and put what it said in the commit message. Measure it with
`python3 tools/comment_check.py` — three lines, eighty characters a line counted
from the left margin, fifteen words per block by default, all three raisable with
`--max-lines`, `--max-line`, `--max-words`. Run it on the files you touched, not on
the repository: the style this rule replaces is still in the tree, and the tool
names 6,246 blocks across 714 files today. A comment that genuinely needs more than the ceiling states so
itself with `comment-check: allow` on the line above it, which the tool honours and
a reviewer can read as a claim, not an escape.

Blocking — remove rather than shorten:

- **Narration** of the change: "now uses…", "no longer FIFO", "this round
  added…", "changed because…". Git holds the change; the file holds the result.
- **Justification** of a choice the code cannot debate: why the alternative is
  worse, why the design is right, a debate with a deleted sibling. Pick the
  better shape and name it; the argument goes in the commit body.
- **Restatement**: what the next line says, or a parameter whose name already
  carries it. `// the page index` above `page: usize` is noise.
- **History**: what the code used to be, or what something removed did.
- An **essay on a leaf function**: a doc comment longer than its body belongs in
  `docs/` as design, not in the file.

Wanted, at one to three lines next to the thing it governs:

- An invariant another file owns, or an order a callback depends on.
- A unit, precision or width trap: `ms` against `px/s`, the `camelCase` names the
  bridge reads by, `attr_fn_like_width` deciding a macro's wrapping.
- A hazard that makes the obvious edit wrong: a disposed handle, a lane slot
  nobody pops, a write that must precede a counter.

When the rule needs more than three lines, the fix is usually a better name or a
smaller function. Reasoning is not banned — it has two homes, `docs/` for the
design and the commit message for the decision — and neither is a comment.

## Resource rules

- Everything that allocates has an explicit teardown: tasks, render queues,
  workers, page registrations, canvases, virtualizers, observers,
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
  trailing period, subject ≤ 50 characters — GitHub's own truncation sits near 72,
  so 50 is what survives a log line, a UI row and a checklist cell intact.
  Enforced rather than remembered:
  `python3 tools/commit_check.py --install` writes a `commit-msg` hook that
  refuses an over-long subject, and `python3 tools/commit_check.py
  --range main..HEAD` audits history. The hook is per checkout, so a fresh clone
  installs it once; `--uninstall` removes it, `--force` replaces a hook this
  repository does not own, and `--status` reports whether it is armed — git skips a
  non-executable hook in silence, so the check is worth running after a reset.
- A squash merge makes the **PR title** the commit subject and appends ` (#NN)`, so a
  title must fit 50 characters *after* that suffix: 43 for a two-digit number, 42 once
  the repo passes 999. Check the merged form, never the title as typed —
  `printf '%s (#62)\n' "$title" > /tmp/m && python3 tools/commit_check.py /tmp/m`.
  No CI job reads a subject or a title, so the hook, `--range`, and the reviewer are the
  entire enforcement; a PR is exactly where the habit leaks.
- Types: `feat`, `fix`, `perf`, `refactor`, `docs`, `test`, `build`, `ci`,
  `chore`. Scope is the area touched (`pdf`, `split`, `shell`, `engine`, …).
- One coherent change per commit; context goes in the body, not the subject.
  No progress reports, phase labels or ticket numbers in the subject. The one
  non-descriptive token a subject may carry is `[skip deep]` (see "Deep CI"),
  and the 50-character check counts it — 39 characters left for the description,
  so keep the scope short.
- Squash fixups before pushing; rewrite remote history only with
  `git push --force-with-lease`.

## Definition of done

1. The change is implemented, and visually verified if it touches UI.
2. Matching local checks pass and `CI` is green for the pushed SHA. `Deep CI`
   is green for that SHA too — on the round's final commit, never on a mid-round
   one that deferred it with `[skip deep]`, which the summary states either way.
3. Docs describe the current behaviour (`docs/architecture.md`,
   `docs/memory/` for memory behaviour).
4. Every added comment passes the diff-blind test (`### Comments`): no
   narration, no defence, no restatement, and `python3 tools/comment_check.py`
   is clean on the files the round changed. The round that adds a rule to this
   repository also applies it to the code in the same round — a guideline the
   author ignores is not a guideline.
5. The summary states what changed, what was verified and any limits. If a
   requirement cannot be met, report the blocker instead of dropping it.
