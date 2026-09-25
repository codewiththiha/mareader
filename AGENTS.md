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
