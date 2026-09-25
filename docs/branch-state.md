# Branch state — `split-wasm-modules-t10`

Single source of truth for what this branch is and where it stands. Read this
before planning or editing; when it disagrees with memory, this file wins.

## What this branch is

Team 10's migration branch for the runtime split mandated by `AGENTS.md` and
`wasm-runtime-migration.md`. Base: `main`. Commits follow the conventional
format in `AGENTS.md` (subject ≤ 72 chars); author is the team identity.

## Migration status

| Phase | State |
| --- | --- |
| 0 — memory/lifecycle baseline | **done**: browser lifecycle suite (`tests/browser/lifecycle.mjs`), counters in `crates/reader-runtime/src/diagnostics.rs`, results in `docs/memory-baseline.md` |
| 1 — explicit runtime/session lifecycle | **done**: `ReaderRuntime` state machine, generations, resource registry, observable disposal (`crates/reader-runtime/src/runtime.rs`) |
| 2 — Shell / Library / Reader split | **done**: three WASM artifacts, shell-owned iframes with a `MessageChannel` handshake, `frame-transport` crate, dispose-as-a-unit |
| 3+ — Reader Host & panes, session-scoped engines, split mode, … | **not started** — waiting on the phase guide |

## Architecture as built (do not re-derive)

- Shell (`src/`) owns routing, the runtime manager, diagnostics, persistence.
- Reader and Library boot inside shell-owned iframes (`src/app/frame.rs`),
  each with its own document, JS realm and WASM instance; the Shell never
  imports their state, and disposal removes the frame as a unit.
- Frame roots must carry `h-full w-full`: a mount with `height: auto` gives
  the virtualizer an indefinite viewport and every page mounts at once
  (the "peak N page hosts" browser failure).
- The PDF engine is still a module-global under `PdfSessionHandle`; true
  session-scoped engines are Phase 4, not a defect to "fix" opportunistically.

## Invariants (enforced by tests — never weaken them)

- Shell diagnostic counters are authoritative; runtime digests merge in only
  keys the shell does not already own (`src/diagnostics.rs`, unit-tested).
- `readerSessionsCreated == readerDisposesCompleted` after every close, and
  `librarySessionsCreated == libraryDisposesCompleted` after every handback.
- Leaving the reader cancels in-flight page renders synchronously with the
  click (`close_document` → `cancel_page_renders`) before the navigate
  command crosses the frame channel; the session destroy during disposal
  remains the single teardown path.
- Browser peaks: page hosts ≤ render window + zombie cap, active renders ≤
  page-lane slots, counters drain to zero at baseline.
- Disposal epoch is frame-instance-local (1 at open, 2 at close); the
  reported runtime generation is Shell-owned — the reader-session count,
  advancing once per reader session across frames.

## CI is the only build

No Rust is compiled in the dev sandbox (disk limits). Push and let GitHub
Actions judge: `CI` (format, clippy+wasm check+dependency gate, `cargo test`,
web contracts, macOS shell) on every push; `Deep CI` (browser lifecycle
baseline + Tauri boot smoke) on pushes touching app/engine paths. Watch run
`361…` job logs, fix, squash fixups, force-push.

## Known follow-ups (do not silently expand scope)

- `docs/runtime-split.md` still describes dynamic-import loading in places;
  production is frame-hosted (reconcile docs-only, do not change code back).
- Diagnostics hardening: a reader that existed but never reported a terminal
  digest should fail `atBaseline` closed (currently only "last digest says
  drained" is required).
