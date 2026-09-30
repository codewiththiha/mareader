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

1. Read this file.
2. Read the branch's `docs/branch-state.md` if it exists. It records
   task state and wins over conversation memory and stale summaries.
3. Prefer the branch's docs over inference. When in doubt, inspect the raw
   source, tests, and exports rather than commit messages.

## Standing rules

These apply to every change on every branch.

1. **Teardown is a first-class concern.** Allocation, cancellation,
   clearing, worker termination, and frame disposal are reviewable parts of
   a change, not afterthoughts. Removing a document or closing a pane must
   release everything that page allocated.
2. **Read the memory rules before touching memory-sensitive code.**
   `docs/memory/rules.md` is binding: frame-scoped release, dwell before
   expensive work, bounded caches with drains, zero-and-remove for canvases,
   weak references from module state, observable teardown counters.
3. **Memory claims need measurements.** Record workload, metric,
   before/after values, and reporting limits. WebKit does not return
   process memory promptly; the replay harness scrolls once per pane, so
   motion-path changes need a scroll-heavy workload.
4. **No legacy path kept as production code.** Feature flags and
   compatibility switches are for migration periods only; they do not
   survive their milestone.
5. **No speculative scaffolding.** Do not add code for tasks not yet
   started. Do not add TODO/FIXME markers; if work is needed, do it or
   leave the code as-is.
6. **Do not weaken invariants.** Never relax test assertions to make them
   pass. If an invariant blocks the task, say so instead of changing it.
7. **UI changes need visible proof.** Implement and visually verify in a
   running browser (desktop-width and narrow), then describe what was
   checked. Text-only claims of "looks good" are not acceptable.
8. **Lazy loading is not fresh instantiation.** Reusing an already-loaded
   module is different from recreating it; do not regress one into the
   other.
9. **Do not reintroduce known bugs.** The codebase comments record past
   regressions (double filtering, raw-canvas lifetime, window-term caps,
   effect stretch). Read the surrounding comments before changing
   rendering paths.

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

## Commit standard

Commit messages use Conventional Commits in English. Commits are the
project's changelog, and subjects are the changelog title line.

### Subject line

- Type/scope prefix, then a summary: `feat(shelf): reclassify on close`.
- **Hard limit: 72 characters total.** Prefer ≤50.
- Verify before committing:
  `node -e "const s=\`SUBJECT\`; ..."` or
  `git log -1 --pretty=%s | awk '{print length, $0}'`.
- Imperative mood (`fix`, not `fixed`/`fixes`); no trailing period.
- Lowercase after the colon, except proper nouns and acronyms.
- The subject states the user-visible or system change. Do not embed
  internal step names, phase labels, task IDs, or branch names.
- One concern per commit. If the subject needs "and", split it.

### Types and scopes

Types: `feat`, `fix`, `docs`, `refactor`, `test`, `build`, `ci`, `chore`.

Scope is the user-facing or system area, not an internal module:
`shell`, `reader`, `pdf`, `epub`, `library`, `shelf`, `settings`,
`download`, `pwa`, `engine`, `wasm`, `assets`, `github-actions`.

### Body

- Separate from the subject with one blank line.
- Explain why and what changed, not how (the diff shows how).
- Wrap at ~72 characters. Bullet lists are fine; wrap each item.
- No AI attribution.
- If the commit fixes a numbered GitHub issue: `Fixes #123`.
- If it changes memory: state the baseline and post-change numbers.

### Hygiene

- Trivial fix commits (formatting, review nits) get squashed into the
  commit they fix before the final push; `git push --force-with-lease` is
  allowed for this on personal branches.
- Rebase onto the target branch when behind; do not merge the target
  branch in.

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
3. Commit subjects are ≤72 characters and conventional.
4. Docs updated: `docs/branch-state.md` reflects task state; memory-
   relevant changes cite measurements; new memory behaviour updates
   `docs/memory/`.
5. The summary reports what changed, what was verified, and any
   limitations.
