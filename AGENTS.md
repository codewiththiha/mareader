# AGENTS.md

Operating instructions for AI agents working in this repository. Read this
file before the first change in a session. Branch-specific planning lives in
`docs/` on that branch; this file is branch-agnostic.

## Product

MaReader: a web reader for manga/comics and light novels, targeting mobile
Safari/WebKit first, then Chromium Android, with a desktop-class reading
experience. The app is offline-capable (Service Worker) and works as a
PWA.

Current architecture, on every branch that has merged it:

- The shell is Leptos CSR Rust compiled to WASM. It owns routing, the
  library, the shelf cache, downloads, and the reader's chrome (toolbar,
  menus, reader state).
- The reading runtime runs in a shell-owned `iframe` per reader so its JS
  realm and WASM linear memory can be released by removing the frame. The
  reader inside the frame is Leptos CSR + the `reader-runtime` crate
  (virtualization, strips, pages, zoom, gestures, effects).
- Page rasterization runs in the engine realm: the engine service,
  pdf.js, its workers, and the bake worker. The realm holds no per-session
  strong references.
- pdf.js is vendored at `public/vendor/pdfjs` and checked into the repo
  (npm is a source of packages, not a runtime dependency).

## Session protocol

Do these steps in order at the start of every session:

1. Read this file.
2. Read the branch's `docs/branch-state.md` if it exists. It records task
   state and MUST be trusted over conversation memory, summaries, and
   assumptions. When they disagree, the file wins.
3. Read the docs the branch points to before inferring. Never assume
   feature completion from commit messages, tags, or PR titles: inspect
   the raw source, tests, and exports.

## Standing rules

These apply to every change on every branch. Each rule states what a
violation looks like.

### 1. Teardown is a first-class feature

Every runtime that owns resources MUST have an explicit teardown path.
Teardown MUST cover the actual resources it owns, including when
applicable:

- async tasks and stale requests,
- render queues,
- prefetch/look-ahead work,
- PDF loading/document objects,
- PDF workers,
- page registrations,
- canvases/ImageBitmap-like raster resources,
- thumbnail caches,
- search/index structures,
- virtualizers,
- ResizeObservers and event listeners,
- timers/idle callbacks,
- reactive resources and DOM mounts.

A `drop` or component unmount alone is NOT evidence of correct teardown.
Removing a document or closing a pane MUST release everything that page
allocated.

### 2. The memory rules are binding

Read `docs/memory/rules.md` before writing code that allocates surfaces,
caches, timers, or workers. Its rules are enforced exactly as written:
frame-scoped release, dwell before expensive work, bounded caches with
drains, zero-and-remove for canvases, weak references from module state,
observable teardown counters. A new audit result goes into
`docs/memory/audit.md`; do not leave it only in the pull request.

### 3. Do not claim memory was released without evidence

"The component unmounted" and "the object went out of scope" are not
memory measurements. For memory-related work, record:

- what was measured,
- how it was measured,
- the workload,
- before/after values,
- known limitations of browser/Tauri memory reporting.

WebKit does not return process memory promptly, and the shipped replay
harness scrolls once per pane, so motion-path changes need a scroll-heavy
workload before their numbers mean anything. The goal is to eliminate
retained references and active resource ownership; the OS may not return
memory to the process immediately.

### 4. One authoritative owner per responsibility

Never implement two competing sources of truth. Do not duplicate state in
two managers. Do not add a compatibility layer that becomes the permanent
implementation: every temporary bridge MUST have an owner, a removal
condition, and a tracked follow-up. Do not add an unbounded fallback.

### 5. Preserve product behavior

The reader's virtualization, look-ahead/prerender, retention,
placeholders, zoom behavior, appearance system, search, selection, and AI
features are existing product behavior. Do not disable or simplify them
to make a change compile. Preserve behavior first, then improve its
ownership and resource accounting.

### 6. Do not weaken invariants

Never relax test assertions to make them pass. Never modify CI workflows
to make failing checks pass. If an invariant or a workflow blocks the
task, report the concrete blocker instead of changing it.

### 7. UI changes need visible proof

Implement and visually verify in a running browser at desktop width and
at narrow width, then describe what was checked. Text-only claims of
"looks good" are not acceptable.

### 8. Lazy loading is not fresh instantiation

Dynamic import/lazy loading may reuse a cached JavaScript module
namespace. When true independent state is required, use explicit
instances/sessions with clear ownership and disposal. Do not regress one
into the other.

### 9. Do not reintroduce known bugs

The codebase comments record past regressions (double filtering,
raw-canvas lifetime, window-term caps, effect stretch). Read the
surrounding comments before changing rendering paths.

### 10. No speculative scaffolding

Do not add code for tasks not yet started. Do not add TODO/FIXME markers;
if work is needed, do it or leave the code as-is. Do not create a large
pile of dead scaffolding in anticipation of later work: create the
contracts the current task can actually exercise.

## Build and validation

CI is the only build. The sandbox has no cargo and should not fetch large
toolchains; heavy compilation happens in CI.

- **No heavy local builds.** Do not run `cargo build`, `trunk`, bundlers,
  or package installs beyond what a specific script requires.
- **Fast local checks** (when the affected files change):
  - `./node_modules/.bin/tsc --noEmit --project tsconfig.json` for engine
    TypeScript (node_modules may need `npm install --no-save
    --no-package-lock typescript@5.6.3 esbuild@0.25.0`).
  - `node ./tools/bundle-engine.mjs && node scripts/test-engine-smoke.js`
    for engine behaviour and teardown pairing.
  - `node scripts/check-doc-paths.js` after touching paths in `README.md`,
    `Mareader.md`, or path references in Rust comments.
  - `node tools/check-session-ownership.mjs` after touching
    `docs/branch-state.md` or `docs/session-ownership.md`.
- **CI is the authority.** After pushing, use the GitHub Actions API to
  find the run for the pushed SHA (do not assume the branch's latest run
  is yours) and poll until completion. Fix failures and repeat until the
  run for the pushed SHA is green. Docs-only changes may trigger no run
  (`.github/workflows/ci.yml` ignores `docs/**`); verify the filter before
  waiting.

## Testing

Prefer focused tests close to the feature and integration/smoke tests for
lifecycle boundaries. For runtime work, test at least:

- open -> use -> dispose,
- dispose during active async work,
- rapid open/close/reopen,
- route transition reader -> library,
- route transition library -> reader,
- multiple panes when split support is present,
- stale task/result rejection after disposal.

Run the project's existing checks before inventing new test
infrastructure. Add new tooling only when it gives a repeatable signal
for the requirement being implemented.

## Repository conventions

- **Rust formatting:** no `rustfmt.toml`; defaults apply (`max_width=100`).
  `view!` macro bodies are exempt.
- **Engine lanes** drop dead work at both the queue edge and the rAF edge
  (`st.dead || s.disposed || st.queueGen !== gen` in
  `public/engine/renderer.ts`). New lanes follow the same pattern.
- **Module-level registries** reference sessions with `WeakRef` and prune
  dead entries; deterministic removal happens at the top of teardown.
- **Canvas release** zeroes the backing store (`releaseCanvas` /
  `remove_snapshots`); DOM removal alone does not free a canvas.
- **Doc cross-references** in the root docs follow the link style enforced
  by `scripts/check-doc-paths.js` for `README.md` and `Mareader.md`; keep
  backticked paths in those files resolving to real modules.

## Git Commit Message Standard

Every commit created by an agent MUST use this format. The purpose is to
keep `git log`, GitHub commit lists, GitHub search results, changelogs, and
review history readable without opening every commit.

### 1. Commit subject format

The first line of every commit is the **subject**. Use Conventional Commits:

```text
<type>[optional scope]: <description>
```

Examples:

```text
fix(pdf): bound fast-scroll raster work
perf(virtualizer): cap concurrent page renders
test(lifecycle): add close-during-render coverage
refactor(reader): isolate virtualizer ownership
docs(memory): record the subsystem audit
ci: run browser lifecycle baseline
```

The subject MUST:

* be exactly one line;
* begin with a valid commit type;
* use an optional lowercase scope when useful;
* contain `: ` after the type/scope;
* describe the actual change, not the entire implementation history;
* use imperative/present-tense wording;
* start with a lowercase description unless a proper noun requires capitalization;
* avoid a trailing period;
* contain no newline;
* never contain a long explanation that belongs in the body.

Prefer:

```text
fix(pdf): cancel stale raster jobs
```

Not:

```text
Fixed the PDF rendering lifecycle by adding cancellation and various
resource cleanup mechanisms to make fast scrolling more memory efficient
```

### 2. Hard subject-length limit

**HARD LIMIT: 72 characters total.**

The limit counts **every character**: letters, numbers, spaces,
punctuation, parentheses, colon, slash, hyphen, underscores, and scope
characters. For example, this subject is measured as the complete line
exactly as written, including every space and punctuation character:

```text
fix(pdf): cancel stale raster jobs
```

Do NOT interpret the limit as "72 visible letters." Do NOT count only the
description. Do NOT exclude the `type(scope): ` prefix. The complete first
line MUST be `<= 72` characters.

### 3. Preferred subject length

```text
PREFERRED: <= 50 characters
HARD MAX:  72 characters
```

When a subject naturally fits under 50 characters, do not stretch it
toward 72 merely to provide more detail. Prefer:

```text
fix(pdf): stop stale page renders
```

over:

```text
fix(pdf): prevent stale page render requests from surviving scroll changes
```

when the shorter form communicates the change accurately.

### 4. What belongs in the subject

The subject answers exactly one question: **What changed?** It does NOT
answer why it changed, how it works, what tests were run, what files
changed, or what historical problem led to it. Those belong in the body.

Good:

```text
perf(pdf): limit concurrent page renders
```

Bad:

```text
perf(pdf): limit concurrent page renders to reduce fast-scroll memory usage by changing the page queue, adding lifecycle counters, draining stale jobs, fixing close races, and preserving lookahead behavior
```

### 5. Use the body for important context

A commit body is OPTIONAL. Use one when the change would otherwise be
difficult to understand from the subject alone. Format:

```text
type(scope): short summary

Why this changed.

Important implementation detail or behavioral constraint.

Tests/checks performed.
```

Body rules, enforced exactly:

* exactly ONE blank line between subject and body;
* wrap body lines at ~72 characters;
* no AI attribution of any kind;
* if the commit fixes a numbered GitHub issue, end the body with
  `Fixes #123`;
* if the commit changes memory behaviour, state the measured baseline and
  post-change numbers in the body.

Example:

```text
perf(pdf): limit concurrent page renders

Fast scrolls could queue multiple full-page raster jobs even when
most skipped pages were no longer visually relevant.

Bound the render lane and drop superseded jobs before they reach
pdf.js while preserving the existing look-ahead behavior.

Tests: browser lifecycle baseline, engine smoke, cargo test.
```

### 6. Commit types

Use these types consistently, and always the most specific applicable one:

```text
feat      new user-visible functionality
fix       bug correction
perf      performance or memory improvement
refactor  structural change without intended behavior change
docs      documentation-only change
test      tests or test infrastructure
ci        CI workflow/check changes
build     build/dependency/toolchain changes
chore     maintenance that does not fit another type
```

Never use vague subjects such as:

```text
update stuff
changes
fix things
improvements
more changes
final changes
```

### 7. Scope rules

A scope is OPTIONAL. Use it when it makes the history easier to search.
Prefer repository terminology already used by the codebase:

```text
pdf  virtualizer  reader  library  lifecycle  engine  appearance
shell  thumbs  memory  ci  docs
```

Do not invent elaborate scopes merely to make the subject longer. Prefer:

```text
fix(pdf): cancel stale renders
```

over:

```text
fix(pdf-renderer-lifecycle-management): cancel stale renders
```

The scope exists to improve classification and searchability, not to
encode the entire implementation tree.

### 8. One commit should communicate one coherent change

Do not create a subject that lists several unrelated jobs.

Bad:

```text
fix(pdf): fix memory leak, update CI, refactor reader, clean docs
```

When changes are logically independent, create separate commits:

```text
fix(pdf): release stale page surfaces
ci: add lifecycle regression gate
docs(memory): record baseline results
```

A commit may contain multiple files and several internal edits when they
form one coherent change. The rule is about **logical scope**, not file
count.

### 9. Breaking changes

Use Conventional Commits' breaking-change syntax only when the change
intentionally breaks an interface:

```text
feat(engine)!: replace global PDF session API
```

or:

```text
feat(engine): replace global PDF session API

BREAKING CHANGE: callers must create an explicit engine session.
```

Do not label a change as breaking merely because it touches many files.

### 10. Phase naming

DO NOT put a phase description into the commit subject.

Bad:

```text
feat(phase-0): implement all lifecycle diagnostics and memory measurement baseline with fast-scroll, zoom, lookahead, prefetch, search race, reopen and teardown validation
```

Better:

```text
test(lifecycle): add browser resource baseline
perf(pdf): bound fast-scroll raster work
fix(lifecycle): drain stale render work on close
```

A phase number may appear in the scope only when it helps history/search,
as in `docs(phase-0): record lifecycle baseline`. Do not use phase names
as a substitute for describing the actual change.

### 11. Do not use commit messages as progress reports

Bad:

```text
Phase 0 implementation complete
Phase 0 almost done
final fixes
complete after lots of fixes
```

Those are temporary project-status statements, not technical history.
Describe what changed:

```text
test(lifecycle): race close against active renders
```

The body can explain which acceptance gap this closes.

### 12. Do not put implementation essays in the subject

Do not use:

```text
Record the matrix baseline with the raced closes and empty lanes
```

even though it is informative. Rewrite it as:

```text
test(lifecycle): record resource baseline
```

and explain the raced closes and empty lanes in the body. Similarly,
instead of:

```text
Fix the thumbnail prefetch lifecycle by adding an epoch and cancellation signal to prevent resource leaks during document teardown
```

use:

```text
fix(thumbs): cancel stale prefetch work
```

and put the lifecycle explanation in the body.

### 13. Verify the subject before committing

Agents MUST verify the actual subject length before creating the commit.
The check MUST count the complete subject, including spaces. Use this
Node one-liner, which counts Unicode code points cleanly:

```bash
node -e '
const s = process.argv[1];
const n = [...s].length;
console.log(`${n}/72`);
if (n > 72) process.exit(1);
' "fix(pdf): cancel stale raster jobs"
```

The invariant is `subject length <= 72`. Do not rely on visual wrapping
in the terminal. Do not rely on GitHub truncation. Do not manually
estimate the length.

### 14. Verify the final commit after creation

After committing, run:

```bash
git log -1 --pretty=%s
```

and verify the same exact subject again. The commit is not correctly
formatted merely because the command that created it looked correct. For
multi-line commits use:

```bash
git log -1 --pretty=format:%s   # one subject line, <= 72 characters
git log -1 --pretty=format:%B   # the full message, for body review
```

### 15. Never silently truncate the meaning

If the useful subject is longer than 72 characters: rewrite it more
precisely, move explanatory detail into the body, and keep the subject
focused on the primary change. Do NOT blindly cut the string at character
72.

Bad:

```text
fix(pdf): prevent stale render jobs from surviving document clos
```

Good:

```text
fix(pdf): cancel stale render jobs
```

The subject must remain a complete, natural statement.

### 16. Commit body and subject separation

If a body exists, there MUST be exactly one blank line between the
subject and body. Git treats the text before the first blank line as the
commit title, and that title is used throughout Git tooling.

Correct:

```text
fix(pdf): cancel stale render jobs

Superseded page requests were able to remain queued after the page
left the virtualizer window.
```

Incorrect:

```text
fix(pdf): cancel stale render jobs
Superseded page requests were able to remain queued after the page
left the virtualizer window.
```

### 17. Agent commit workflow

Before every commit, follow this sequence exactly:

```text
1.  Inspect the staged diff.
2.  Identify the single primary change.
3.  Choose the most specific commit type.
4.  Add a short scope when useful.
5.  Write the subject in imperative language.
6.  Count the COMPLETE subject, including spaces and punctuation.
7.  Ensure length <= 72 characters.
8.  Prefer <= 50 characters when practical.
9.  Add a body only when context is useful.
10. Wrap body lines near 72 characters.
11. Verify the final commit with git log.
```

### 18. Required default style

When there is no strong reason otherwise, use exactly this shape:

```text
<type>(<scope>): <short imperative description>
```

Examples:

```text
fix(pdf): cancel stale page renders
perf(pdf): cap raster concurrency
fix(reader): dispose virtualizer bindings
test(lifecycle): cover rapid reopen
docs(memory): record baseline results
ci: run lifecycle regression suite
```

The default objective is:

```text
precise + searchable + imperative + technically descriptive
+ <= 72 characters + preferably <= 50 characters
```

The subject is the index entry for the change. The body is where the
explanation lives. Note that 72 is the hard project rule, not a
Git/GitHub protocol limit: Git's own guidance favors <= 50, GitHub
documents 72 as the title maximum, and Conventional Commits defines the
syntax rather than a subject-length cap.

References:

- Git commit documentation: https://git-scm.com/docs/git-commit
- Conventional Commits: https://www.conventionalcommits.org/

## Code quality

- Prefer clear names and small ownership-focused types.
- Comments MUST explain why, invariants, or non-obvious lifecycle
  behavior. Do not add comments that restate code.
- Do not add speculative abstractions "for future use" unless the current
  task needs them.
- Do not make fields optional when the domain guarantees their presence.
- Prefer compiler-enforced invariants over defensive runtime branching.
- Do not swallow errors merely to keep an old path alive.
- Keep logging useful. Do not add per-frame, per-page, or per-scroll logs
  for ordinary operation.
- Do not add broad error context that only repeats the underlying error.

## Scope discipline

- Make the smallest change that completes the requested task. No drive-by
  refactors, no incidental formatting sweeps.
- Separate unrelated changes into separate commits.
- Do not modify CI workflows to make failing checks pass. If a workflow is
  genuinely wrong, say so and propose the change instead of editing it
  silently.

## Definition of done

1. The requested change is implemented and visually verified where it
   touches UI.
2. Applicable local checks pass; the CI run for the pushed SHA is green.
3. Commit messages follow the `## Git Commit Message Standard` exactly;
   the subject length was verified with the commands in its sections 13
   and 14, and the final commit re-checked with `git log`.
4. Docs updated: `docs/branch-state.md` reflects task state; memory-
   relevant changes cite measurements; new memory behaviour updates
   `docs/memory/`.
5. The summary reports what changed, what was verified, and any
   limitations.
6. If a criterion cannot be satisfied, report the concrete blocker. Do
   not silently revert to an old implementation and do not silently drop
   the requirement.
