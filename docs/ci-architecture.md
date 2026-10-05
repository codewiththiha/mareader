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
browser         the one production frontend build, then the wasm app in a real
                browser: the boot contract stage (library at /, both runtime
                transitions with disposal order, /reader, and a missing
                artifact's error state), then the lifecycle/memory suite;
                the dist it measured is uploaded (3 days) pass or fail, so a
                run can be replayed or compared locally
tauri-smoke     the same production build, then the REAL native window under
                Xvfb: the Library runtime boots, the pixels are not one flat
                colour, and an OS document handoff boots the Reader

both deep lanes honour "[skip deep]" in the commit subject (never the cron)
```

- **One check, one reason.** No command runs twice across lanes; the engine
  smoke consumes the bundle the same job built from current source, so a
  smoke pass can never mean an old artifact passed.
- **Skippable where the answer cannot change.** `CI` ignores pushes that only
  touch `docs/**` — no lane reads those files as input (the contract scripts
  parse SOURCE comments, not the documents they point at), so there is
  nothing to compile. The filter drops a run only when EVERY changed path
  matches. `Deep CI`'s two 45-minute lanes honour `[skip deep]` in the
  commit subject, and a `workflow_dispatch` can narrow the run to one lane or
  override the marker. Neither escape is the last word: the nightly cron
  takes no notice of either, so whatever the day skipped is caught
  overnight.
- **Parallel, not serial.** The lanes are independent; the wall clock is the
  slowest lane, not the sum.
- **One cache key per lane** (`lint-cache`, `test-cache`, `macos-cache`):
  two lanes sharing a key race to write it, and neither carries the
  artifacts the other built. The format lane caches nothing because it
  compiles nothing. `target/` is never transferred between jobs — a cache
  miss that rebuilds locally is cheaper and more robust than artifact
  transfer.
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

<!-- // only the changed file was rewritten -->
