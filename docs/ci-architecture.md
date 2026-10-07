# MAReader CI architecture

Why the required workflow looks the way it does. The YAML is the
machine-readable checklist; the reasoning lives here.

## What CI answers, per run

1. Does the Rust workspace compile, lint and pass its tests?
2. Does the wasm32-only code the host builds never touch still compile?
3. Does the web/tooling side build from source and satisfy its
   cross-language contracts?
4. Does the macOS-only Tauri shell compile and test?
5. Is the tree rustfmt-clean?

## The lanes

```text
format          cargo fmt --all -- --check            (no cache: nothing is compiled)
lint            cargo metadata --locked (early) +
                clippy --workspace -D warnings +
                cargo check --target wasm32-unknown-unknown +
                cargo check -p app-chrome (standalone)
test            cargo test --workspace --exclude mareader-shell --locked
web             npm ci -> build:ts -> build:css -> contract checks -> engine smoke
macos-shell     clippy + test of mareader-shell, natively on macOS

deep (nightly / on demand / when the boot path itself changes):
frontend-build  one production frontend build; uploads frontend-dist (3 days)
browser         downloads frontend-dist, then runs the wasm app's browser
                lifecycle and boot-contract checks
memory-replay   downloads frontend-dist, then replays split memory in Chromium
                and WebKit
tauri-smoke     downloads frontend-dist, then boots the REAL native window
                under Xvfb and hands an OS document to the Reader

all four deep jobs honour "[skip deep]" in the commit subject (never the cron)
```

- **One check, one reason.** The production frontend builder runs once and
  uploads the bundle the browser, memory replay and Tauri smoke jobs consume;
  a smoke pass can never mean an old artifact passed.
- **Skippable where the answer cannot change.** `CI` ignores pushes that only
  touch `docs/**` — no lane reads those files as input (the contract scripts
  parse SOURCE comments, not the documents they point at), so there is
  nothing to compile. The filter drops a run only when EVERY changed path
  matches. `Deep CI` has one shared frontend build (45-minute cap); browser
  lifecycle (45) and native boot smoke (45) fan out after it, and memory replay
  (75) follows browser. Changed paths decide whether the workflow is in scope;
  `[skip deep]` in the SUBJECT of the
  push's last commit drops the frontend build and all three validation lanes —
  the marker job reads that one line and no body, so quoting it in prose cannot
  turn a gate off by accident. A `workflow_dispatch` can choose browser or Tauri
  validation, or override the marker; both lanes consume the shared build. Neither
  escape is the last word: the nightly cron takes no notice of either, so
  whatever the day skipped is caught overnight.
- **The skip is a policy, not a shortcut, and it lives in `AGENTS.md`.** What
  may carry the marker (presentation, prose, pure logic that `CI` already
  tests) and what may never (anything that allocates, retains, counts or
  releases) is decided by the lists there, because the judgement is about the
  change, not about the workflow. The marker also carries a second, narrower
  meaning: on a branch under repair it *defers* a lane to the round's last
  code-bearing commit rather than dropping it, since `CI` says whether the tree
  builds and no deep verdict is readable before that ("The CI loop" in
  `AGENTS.md` states the loop and its one rule that a deferral is never what a
  report calls done). The path filter stays wide on purpose: a
  narrow list plus a marker would let an owner change through with neither
  gate, and widening a filter costs a run while narrowing one costs a
  regression.
- **Build once, then fan out.** Browser and Tauri smoke run in parallel after
  the shared frontend build; memory replay follows the browser lane. The
  frontend is an artifact, not a second compilation in either smoke job.
- **One cache key per Rust lane** (`lint-cache`, `test-cache`, `macos-cache`,
  `deep-cache`, `tauri-smoke-cache`): two lanes sharing a key race to write it,
  and neither carries the artifacts the other built. The format lane caches
  nothing because it compiles nothing. `target/` is never transferred between
  jobs — a cache miss that rebuilds locally is cheaper and more robust than
  artifact transfer.
- **The shell crate is excluded from the Linux LINT lane and compiled on
  macOS** because its macOS-only branches (`set_traffic_lights`, objc2) do not
  compile on Linux. It IS built on Linux in the deep lane's native smoke job,
  where it links against the real frontend build — that build is what caught
  the platform stub for the traffic lights still being alive as dead code.

## Decisions worth remembering

- **`--locked` everywhere, no lockfile mutation.** `cargo metadata --locked
  --no-deps` is the cheap early failure; nothing in CI ever edits
  `Cargo.lock` just to discover the answer.
- **Feature selections are linted by the artifact build, not by Clippy.**
  `cargo clippy --workspace --all-targets` unifies the workspace's features,
  so a helper only a disabled format calls looks used and its dead-code
  warning never fires. The artifact builds (`cargo check --target
  wasm32-unknown-unknown --workspace`, and the per-route `reader` / `pdf` /
  `reflow` bins with their own feature sets) are where that code shows, so
  every runtime crate carries its own `[lints.rust]` table — the root
  package's table stops at the root package. A dead helper is deleted or
  used, and `allow(dead_code)` is not an answer (the workflow greps for it).
- **No README test-count gate.** The test runner is the authority; a number
  in prose is not a second database of executable tests.
- **rustfmt is required, permanently.** After the one intentional
  normalization commit, `cargo fmt --all -- --check` stays in the required
  path. The one-shot `.github/workflows/fmt.yml` exists only to produce that
  kind of commit (push a message containing `[fmt]`); it is a maintenance
  tool, not a gate.
- **The wasm check stays.** It is an architectural check, not a duplicate of
  host compilation: it is the only lane that sees `wasm32`-only code. The
  standalone `app-chrome` check stays with it because workspace feature
  unification hides missing direct dependencies.
- **Toolchain:** unpinned `stable`, deliberately — a moving target is what
  makes a nightly red informative. A pin is a decision with a date and a
  reason, not a cleanup: it lands as a `rust-toolchain.toml` naming a version
  the project has tested, in a PR that runs CI with it.
- **Actions:** current majors (`actions/checkout@v7`, `actions/setup-node@v7`,
  `Swatinem/rust-cache@v2`), updated through a PR so CI validates each bump.
- **Permissions:** `contents: read`; nothing in this workflow writes. The
  release workflow holds the only publishing token, isolated on tags.
- **Expensive checks live in `deep-ci.yml`:** the production frontend build,
  the browser lifecycle and memory replay, and the native boot smoke run
  nightly, on demand, or when the boot path itself changes — never in every
  PR, where a 45-minute lane would price a comment typo.

## What "healthy" means

The required workflow is fast enough to run constantly, every expensive
operation has one owner, smoke tests exercise the source they run next to,
failures name a real repository invariant, and no lane exists merely to keep
another lane green.
