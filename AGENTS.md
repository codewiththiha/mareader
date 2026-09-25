# MAReader Agent Instructions

## Mission

This repository is being migrated from a unified reader application toward explicit runtime boundaries:

- Shell: routing, window integration, persistence, module lifecycle.
- Library runtime: library UI and library state only.
- Reader runtime: reader host, shared chrome, focus, pane layout, and reader coordination.
- Format runtimes/sessions: PDF, Markdown, and TXT document work owned by the active pane.

The goal is real lifecycle isolation and lower retained memory, not merely new files or parallel abstractions.

## Non-negotiable rules

### 1. Do not preserve the unified architecture as the production path

Do not build a new architecture beside the existing one and leave the old path in control.

A migration is incomplete when any of these are true:

- old unified routing still mounts the real reader,
- new runtime code exists but production still calls the legacy code,
- a feature silently falls back to legacy behavior,
- both old and new implementations remain active for the same responsibility,
- a compatibility layer becomes the permanent implementation.

Compatibility may exist temporarily only when it is part of a measured migration step. Every such bridge must have an owner, removal condition, and tracked follow-up. Do not add an unbounded fallback.

### 2. Optimize for the stated goal, not for the smallest diff

Do not reject an architectural change merely because it touches many files.

The target is a stronger ownership boundary:

`leave reader -> dispose reader workspace -> tear down document engines/workers -> release runtime references -> mount library`

A larger refactor is preferable to a safe-looking patch that cannot achieve this.

### 3. Never implement two competing sources of truth

There must be one authoritative owner for each responsibility.

Examples:

- navigation: Shell
- library state: Library runtime
- reader workspace state: Reader runtime
- pane layout: Reader host
- document rendering: format pane/runtime
- PDF engine session: one explicit session owner
- pane focus: Reader host
- appearance authority: focused pane or explicit blend policy

Do not duplicate state in both legacy and new managers.

### 4. Runtime boundaries are real boundaries

A Rust crate split is not enough.
A WASM bundle split is not enough.
A route split is not enough.

When a phase claims runtime isolation, verify that a specific runtime instance owns document state and can be disposed as a unit.

Use explicit session/runtime objects rather than process-wide or module-global document state whenever possible.

### 5. Teardown is a first-class feature

Every runtime that owns resources must have an explicit teardown path.

Teardown must cover the actual resources it owns, including when applicable:

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

A `drop` or component unmount alone is not evidence of correct teardown.

### 6. Do not remove useful reader behavior to make the migration easier

The reader's virtualization, look-ahead/prerender, retention, placeholders, zoom behavior, appearance system, search, selection, and AI features are existing product behavior.

Do not disable or simplify them merely to make the new architecture compile.

During migration, preserve behavior first, then improve its ownership and resource accounting.

### 7. Split mode must be designed into the architecture, not bolted on later

Do not model split panes as independent routes.

Routes select high-level surfaces. Split mode belongs to a ReaderWorkspace/Panes tree.

A pane must be able to represent PDF, Markdown, or TXT without the host needing format-specific rendering code.

### 8. Do not confuse lazy loading with fresh instantiation

Dynamic import/lazy loading may reuse a cached JavaScript module namespace.
When true independent state is required, use explicit instances/sessions with clear ownership and disposal.

The agent must verify this assumption from current platform/toolchain documentation when implementing the runtime loader.

### 9. Do not claim memory was released without evidence

"The component unmounted" and "the object went out of scope" are not memory measurements.

For memory-related work, record:

- what was measured,
- how it was measured,
- the workload,
- before/after values,
- known limitations of browser/Tauri memory reporting.

The goal is to eliminate retained references and active resource ownership. The OS may not return memory to the process immediately.

## Existing architecture facts to preserve during migration

The current repository already has useful domain boundaries such as `library-core`, `reader-core`, `pdf-core`, `pdf-engine`, `md-core`, `txt-core`, `app-chrome`, and the virtualizer crates.

These crate boundaries should be reused. Do not duplicate their domain models into new ad-hoc modules unless there is a demonstrated ownership problem.

The current application still has one root Leptos mount and one `AppState`; `/` mounts the library and `/reader` mounts the reader. The current reader/document code also has explicit destroy/sweep behavior. Those are migration starting points, not reasons to keep the unified runtime forever.

## Implementation workflow

For every phase:

1. Read the phase document before editing.
2. Inspect the current implementation that owns the responsibility.
3. Write down the old production path and the intended new production path.
4. Change the ownership boundary.
5. Route production execution through the new owner immediately.
6. Remove or quarantine obsolete ownership code.
7. Add tests or assertions for the new boundary.
8. Run the affected tests/build/checks.
9. Search the repository for stale legacy call sites.
10. Verify the acceptance criteria in the phase document.

Do not stop at "new code compiles".

## Legacy-path kill switch

When a phase introduces a replacement for an existing path, add a repository-local check where practical so accidental reintroduction is obvious.

Examples:

- compile-time feature gates,
- targeted grep/check scripts,
- assertions that only one runtime owner exists,
- integration tests proving route transitions use the new runtime,
- smoke tests proving old entry points are no longer invoked.

Prefer a failing check over a comment saying "legacy path should not be used."

## Testing

Prefer focused tests close to the feature and integration/smoke tests for lifecycle boundaries.

For runtime work, test at least:

- open -> use -> dispose,
- dispose during active async work,
- rapid open/close/reopen,
- route transition reader -> library,
- route transition library -> reader,
- multiple panes when split support is present,
- stale task/result rejection after disposal.

Run the project's existing checks before inventing new test infrastructure. Add new tooling only when it gives a repeatable signal for the requirement being implemented.

## Code quality

- Prefer clear names and small ownership-focused types.
- Comments should explain why, invariants, or non-obvious lifecycle behavior.
- Do not add comments that restate code.
- Do not add speculative abstractions "for future use" unless the current phase needs them.
- Do not make fields optional when the domain guarantees their presence.
- Prefer compiler-enforced invariants over defensive runtime branching.
- Do not swallow errors merely to keep an old path alive.
- Keep logging useful. Do not add per-frame, per-page, or per-scroll logs for ordinary operation.
- Do not add broad error context that only repeats the underlying error.

## Change discipline

Keep the implementation coherent inside the phase.

Do not create a large pile of dead scaffolding in anticipation of later phases. Create the contracts the current phase can actually exercise.

When a legacy module becomes obsolete because the new implementation is now authoritative, delete it or reduce it to a clearly time-bounded compatibility adapter. Do not leave an unused copy of the production implementation in the tree.

## Definition of done

A phase is done only when:

- the new path is exercised by production code,
- the old path is no longer the authority,
- the requested behavior still works,
- affected tests pass,
- stale call sites were searched for,
- lifecycle/ownership is documented,
- no unexplained fallback remains.

If the agent cannot satisfy a phase criterion, it must report the concrete blocker rather than silently reverting to the legacy implementation.

## Git Commit Message Standard

Every commit created by an agent MUST use the repository's standard commit-message format.

The purpose of this rule is to keep `git log`, GitHub commit lists, GitHub search results, changelogs, and review history readable without opening every commit.

### 1. Commit subject format

The first line of every commit is the **subject**.

Use Conventional Commits:

```text
<type>[optional scope]: <description>
```

Examples:

```text
fix(pdf): bound fast-scroll raster work
perf(virtualizer): cap concurrent page renders
test(lifecycle): add close-during-render coverage
refactor(reader): isolate virtualizer ownership
docs(phase-0): tighten memory baseline
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

The limit counts **every character**, including:

* letters;
* numbers;
* spaces;
* punctuation;
* parentheses;
* colon;
* slash;
* hyphen;
* underscores;
* scope characters.

For example:

```text
fix(pdf): cancel stale raster jobs
```

is measured as the complete line exactly as written, including every space and punctuation character.

Do NOT interpret the limit as "72 visible letters."

Do NOT count only the description.

Do NOT exclude the `type(scope): ` prefix.

The complete first line MUST be `<= 72` characters.

### 3. Preferred subject length

Although 72 characters is the hard ceiling, the normal target is:

```text
PREFERRED: <= 50 characters
HARD MAX: 72 characters
```

This follows the long-standing Git guidance to keep the summary around 50 characters while allowing the 72-character ceiling used in GitHub-oriented conventions.

When a subject naturally fits under 50 characters, do not stretch it toward 72 merely to provide more detail.

Prefer:

```text
fix(pdf): stop stale page renders
```

over a longer equivalent such as:

```text
fix(pdf): prevent stale page render requests from surviving scroll changes
```

when the shorter form communicates the change accurately.

### 4. What belongs in the subject

The subject answers:

> What changed?

It should NOT attempt to answer all of:

> Why did it change?
>
> How does it work?
>
> What tests were run?
>
> What files changed?
>
> What historical problem led to it?

Those belong in the body.

Good:

```text
perf(pdf): limit concurrent page renders
```

The body can explain that the previous implementation allowed too many full-page raster allocations during zoom and fast scrolling.

Bad:

```text
perf(pdf): limit concurrent page renders to reduce fast-scroll memory usage by changing the page queue, adding lifecycle counters, draining stale jobs, fixing close races, and preserving lookahead behavior
```

The subject is trying to become the entire commit description.

### 5. Use the body for important context

A commit body is OPTIONAL.

Use one when the implementation would otherwise be difficult to understand from the subject alone.

Format:

```text
type(scope): short summary

Why this changed.

Important implementation detail or behavioral constraint.

Tests/checks performed.
```

Keep body lines around **72 characters or fewer** for readability. GitHub recommends description lines under 72 characters.

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

Use these types consistently:

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

Use the most specific applicable type.

Examples:

```text
fix(pdf): release stale page surfaces
perf(pdf): cap raster concurrency
perf(virtualizer): reduce retained scroll items
refactor(reader): extract page lifecycle coordinator
test(lifecycle): cover rapid reopen cycles
docs(phase-0): record lifecycle ownership
ci: run browser memory baseline
```

Do not use vague subjects such as:

```text
update stuff
changes
fix things
improvements
work on phase 0
more changes
final changes
```

### 7. Scope rules

A scope is OPTIONAL.

Use it when it makes the history easier to search.

Prefer repository terminology already used by the codebase:

```text
pdf
virtualizer
reader
library
lifecycle
engine
appearance
ci
docs
```

Do not invent elaborate scopes merely to make the subject longer.

Prefer:

```text
fix(pdf): cancel stale renders
```

over:

```text
fix(pdf-renderer-lifecycle-management): cancel stale renders
```

The scope exists to improve classification and searchability, not to encode the entire implementation tree.

### 8. One commit should communicate one coherent change

Do not create a subject that lists several unrelated jobs.

Bad:

```text
fix(pdf): fix memory leak, update CI, refactor reader, clean docs
```

When changes are logically independent, create separate commits.

Good:

```text
fix(pdf): release stale page surfaces
```

followed by:

```text
ci: add lifecycle regression gate
```

and, when appropriate:

```text
docs(phase-0): record baseline results
```

A commit may contain multiple files and several internal edits when they form one coherent change. The rule is about **logical scope**, not file count.

### 9. Breaking changes

Use Conventional Commits' breaking-change syntax when the change intentionally breaks an interface:

```text
feat(engine)!: replace global PDF session API
```

or:

```text
feat(engine): replace global PDF session API

BREAKING CHANGE: callers must create an explicit engine session.
```

Conventional Commits defines `!` and `BREAKING CHANGE:` for this purpose.

Do not label a change as breaking merely because it touches many files.

### 10. Phase naming

When working through an architectural phase, DO NOT put the entire phase description into the commit subject.

Bad:

```text
feat(phase-0): implement all lifecycle diagnostics and memory measurement baseline with fast-scroll, zoom, lookahead, prefetch, search race, reopen and teardown validation
```

Better:

```text
test(lifecycle): add browser resource baseline
```

or:

```text
perf(pdf): bound fast-scroll raster work
```

or:

```text
fix(lifecycle): drain stale render work on close
```

The phase number may appear in the scope only when it helps history/search:

```text
docs(phase-0): record lifecycle baseline
```

Do not use phase names as a substitute for describing the actual change.

### 11. Do not use commit messages as progress reports

Do not create subjects like:

```text
Phase 0 implementation complete
Phase 0 almost done
Phase 0 final fixes
Phase 0 complete after lots of fixes
```

Those are temporary project-status statements, not useful technical history.

The commit should describe what changed:

```text
test(lifecycle): race close against active renders
```

The body can explain that this closes a Phase 0 acceptance gap.

### 12. Do not put implementation essays in the subject

These repository rules are especially strict because this branch's earlier subjects were too long to scan efficiently.

Do not use:

```text
Record the matrix baseline with the raced closes and empty lanes
```

even though it is informative.

Rewrite it as:

```text
test(lifecycle): record resource baseline
```

Then explain the raced closes and empty lanes in the body.

Similarly, instead of:

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

The check MUST count the complete subject, including spaces.

A simple Node-based check is preferred because it counts Unicode code points cleanly:

```bash
node -e '
const s = process.argv[1];
const n = [...s].length;
console.log(`${n}/72`);
if (n > 72) process.exit(1);
' "fix(pdf): cancel stale raster jobs"
```

The important invariant is:

```text
subject length <= 72
```

Do not rely on visual wrapping in the terminal.

Do not rely on GitHub truncation.

Do not manually estimate the length.

### 14. Verify the final commit after creation

After committing:

```bash
git log -1 --pretty=%s
```

Then verify the same exact subject again.

The commit must not be considered correctly formatted merely because the command that created it looked correct.

For multi-line commits:

```bash
git log -1 --pretty=format:%s
git log -1 --pretty=format:%B
```

The first command must produce one subject line of `<= 72` characters.

### 15. Never silently truncate the meaning

If the useful subject is longer than 72 characters:

1. rewrite it more precisely;
2. move explanatory detail into the body;
3. keep the subject focused on the primary change.

Do NOT blindly cut the string at character 72.

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

If a body exists, there MUST be exactly one blank line between the subject and body.

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

Git treats the text before the first blank line as the commit title, and that title is used throughout Git tooling.

### 17. Agent commit workflow

Before every commit:

```text
1. Inspect the staged diff.
2. Identify the single primary change.
3. Choose the most specific commit type.
4. Add a short scope when useful.
5. Write the subject in imperative language.
6. Count the COMPLETE subject, including spaces and punctuation.
7. Ensure length <= 72 characters.
8. Prefer <= 50 characters when practical.
9. Add a body only when context is useful.
10. Wrap body lines near 72 characters.
11. Verify the final commit with git log.
```

### 18. Required default style

When there is no strong reason otherwise, agents should prefer this shape:

```text
<type>(<scope>): <short imperative description>
```

Examples:

```text
fix(pdf): cancel stale page renders
perf(pdf): cap raster concurrency
fix(reader): dispose virtualizer bindings
test(lifecycle): cover rapid reopen
docs(phase-0): record baseline results
ci: run lifecycle regression suite
```

The default objective is:

```text
precise
+ searchable
+ imperative
+ technically descriptive
+ <= 72 characters
+ preferably <= 50 characters
```

The subject is the index entry for the change.

The body is where the explanation lives.

Note: 72 is the hard project rule, not a Git/GitHub protocol limit. Git's own guidance favors <= 50, GitHub documents 72 as the title maximum, and Conventional Commits defines the syntax rather than a subject-length cap.

References:

- Git commit documentation: https://git-scm.com/docs/git-commit
- GitHub contributing guide: https://docs.github.com/en/get-started/exploring-projects-on-github/contributing-to-open-source
- Conventional Commits: https://www.conventionalcommits.org/
