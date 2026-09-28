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

## Warm slot (why a route switch is no longer a boot)

`src/app/manager.rs::run_start` used to call `dispose_active().await` BEFORE
the replacement existed, and `start_serialized` blocked concurrent starts on
top of that: every transition was destroy-A → create-B → boot-B, so the user
paid a full artifact boot (fetch JS, fetch and instantiate WASM, mount) on
every click.

The manager now owns three slot states — `Active`, `Warm`, `Retiring`
(`src/app/frame.rs::FrameSlot`) — and keeps at most one `Warm` frame behind
whatever is on screen:

- 700ms after a runtime goes active (`WARM_DELAY_MS`), the Shell boots its
  counterpart into a hidden slot. The boot stops at `Ready`: **a warm boot
  never opens a document**, so the PDF machinery is never paid for twice and
  never paid for a book the user did not ask for.
- A navigation for a kind whose warm frame is ready is a **promotion, not a
  boot**: same element, same document, same realm, same WASM instance. The
  reveal is the frame's `data-mareader-slot` flipping to `active` (CSS
  `z-index`), which is why the frame painted while it waited — there is no
  cover to hold and nothing to await.
- A click that beats the warm boot waits out its REMAINDER
  (`wait_verdict()`), never a second boot of the same artifact.
- The runtime it displaced goes `Retiring` in the same synchronous block as
  the reveal and is disposed BEHIND it, off the critical path.
- A warm frame that died on the way up (`ready_outcome()` is an error or a
  timeout) is torn down and the transition falls back to `cold_start` — the
  warm slot must never cost the user the runtime.
- The library's heavy startup passes (migrate, measure, cover backfill,
  rescan) park in `DEFERRED` during a warm boot and run on reveal, so the
  shelf the user left is refreshed rather than replayed from boot time.

## Invariants (enforced by tests — never weaken them)

- Shell diagnostic counters are authoritative; runtime digests merge in only
  keys the shell does not already own (`src/diagnostics.rs`, unit-tested).
- Per kind, `created - completed == (active is X) + (warm-ready is X)`. The
  old `created == completed` form is only true when the warm slot is EMPTY,
  and it is never empty while the shell is warm — a warm runtime counts as
  created the moment it answers `Ready`.
- Leaving the reader cancels in-flight page renders synchronously with the
  click (`close_document` → `cancel_page_renders`) before the navigate
  command crosses the frame channel; the session destroy during disposal
  remains the single teardown path.
- Browser peaks: page hosts ≤ render window + zombie cap, active renders ≤
  page-lane slots, counters drain to zero at baseline.
- At most one frame is visible at any instant, and it is the `active` one. A
  hidden slot is `visibility: hidden` — **never `display: none`**, which
  starves the iframe of `requestAnimationFrame` and would therefore never
  produce `Painted`.
- Two frames may differ, two VISIBLE frames may not: hiding the outgoing
  frame is part of the reveal, not a follow-up task.
- Nothing is disposed while it is on screen (`run_retire` re-asserts this for
  any retirement that did not come from a reveal).
- Per kind, `created − completed == (active is X) + (warm-ready is X)`: the
  retired session's disposal still runs to completion, just not in front of
  the handoff.
- A warmed frame is the frame that gets revealed — `backSlots.active ===
  warmShelf.warm` — and the revealed generation is the warmed generation.
  Rebooting at promotion time is a failure, not an optimisation.
- Disposal epoch is frame-instance-local (1 at open, 2 at close); the
  reported runtime generation is Shell-owned — the reader-session count,
  advancing once per reader session across frames.

## CI is the only build

No Rust is compiled in the dev sandbox (disk limits). Push and let GitHub
Actions judge: `CI` (format, clippy+wasm check+dependency gate, `cargo test`,
web contracts, macOS shell) on every push; `Deep CI` (browser lifecycle
baseline + Tauri boot smoke) on pushes touching app/engine paths. Watch the
run's job logs (`ci_watch.py` at the workspace root polls them), fix, squash
fixups, force-push. No lane reads `docs/**`, so a docs-only push runs neither
workflow.

## CI is skippable where it is not needed

- `CI` ignores pushes that only touch `docs/**` — no lane reads those files
  as input (the contract scripts parse SOURCE comments, never the documents
  they point at). Any push touching code runs the whole matrix.
- `Deep CI`'s two 45-minute lanes honour `[skip deep]` in the commit subject;
  a `workflow_dispatch` can narrow the run to one lane or override the
  marker. The nightly cron ignores it, so a skip is never the last word.

## Measured, not assumed

The warm slot is proven by the browser lifecycle baseline, not by reasoning:
`rapidTransitions` reports `reusedWarmFrame: true` for all four back-to-back
handoffs, the host sampler's `peakFrames` is 2 (one on screen, one behind
it — never a third on screen), and every memory trend is unchanged
(`slope 0 B/cycle, drift 0 B` across normal, rapid and same-page cycles).
A click that outran the 700ms rearm boots the lane on demand instead of
falling back to a covered cold start, so the runtime the user is leaving
stays on screen for the boot either way.

## Known follow-ups (do not silently expand scope)

- `docs/runtime-split.md` still describes dynamic-import loading in places;
  production is frame-hosted (reconcile docs-only, do not change code back).
- Diagnostics hardening: a reader that existed but never reported a terminal
  digest should fail `atBaseline` closed (currently only "last digest says
  drained" is required).
