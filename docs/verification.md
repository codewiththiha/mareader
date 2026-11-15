# Verification: local mode and CI mode

Every change is proven by the same gates (AGENTS.md, "Gates"): format,
clippy, tests, the xtask checks, and a look at the result. What differs
between machines is **where** they run. Neither mode is fixed: an agent
chooses per session, and the user can override the choice at any time.

| | Local mode | CI mode |
|---|---|---|
| Builds and tests | on this machine | on GitHub Actions |
| Verdict | the local gates | `python3 tools/ci.py watch` |
| Screenshots | `cargo run -p mareader-ui-egui --example shot` | `python3 tools/ci.py shots` |
| Browser preview | `tools/web.sh --serve` | `python3 tools/ci.py web` |
| Memory budgets | `cargo run -p mareader-ui-egui --example memory -- BOOK` | `python3 tools/ci.py memory --ref BRANCH` |
| Kokoro parity | none (needs the 650 MB model and builds) | `python3 tools/ci.py kokoro --ref BRANCH` |
| Piper parity | none (needs six voices and onnxruntime) | `python3 tools/ci.py piper --ref BRANCH` |
| Kitten parity | none (needs four builds and onnxruntime) | `python3 tools/ci.py kitten --ref BRANCH` |
| Other languages' phonemes | none (needs espeak-ng) | `python3 tools/ci.py ipa --ref BRANCH` |
| Chinese phonemes | none (needs misaki) | `python3 tools/ci.py hanzi --ref BRANCH` |
| System voices | none (Linux CI has no speech) | `python3 tools/ci.py system --ref BRANCH` |
| Installers | `cargo packager --release`, then `tools/check-installers.sh target/release` | `python3 tools/ci.py installers` |
| Polls CI | no | yes, until the verdict |

## Why two modes

A capable workstation builds the workspace in minutes and keeps its cache.
A sandbox does neither. The one this repository is developed in has 2 GB of
RAM and 2 cores, and its build cache is lost at every reset. Measured there
on 2026-10-14:

| | Time |
|---|---|
| Cold build of the gates, after a reset (one compiler at a time) | 30–35 min |
| CI, push to a green verdict (Rust / test median; 2026-11-10, see "CI time") | 15.2 min plus queue |
| CI, push to a red Gate job (median Gate time, before digest) | 15.5 s |
| `tools/ci.py shots`, dispatch to PNGs on disk | 70–85 s |
| `tools/ci.py web`, dispatch to the preview on disk (cold wasm build) | 2.5 min |
| `tools/ci.py cache restore` (lint builds, 1.2 GB at 7–17 MB/s, then unpack; 2026-10-27) | 2–3.5 min |
| The first local clippy after that restore (`-p mareader-ui-egui --all-targets`) | 28 s |
| The full local gates right after a restore (nothing compiles) | 11 s |
| `tools/ci.py cache build`, seeded from the previous cache | 2.5 min, plus changed crates |
| `tools/ci.py cache build --cold`, from nothing | 13.5 min |

So on such a machine the gates belong in CI, and on a workstation polling CI
is a waste: the same checks already passed locally.

## Choosing a mode

Highest precedence first:

1. **The user's words.** "Use CI", "use the workflow" or "don't build
   locally" mean CI mode, even on a fast machine. "Build locally" or "run the
   tests here" mean local mode, even on a slow one. The choice stands for the
   session, or until the user changes it.
2. **`MAREADER_VERIFY=local`, `cache` or `ci`** in the environment: a
   standing preference for a machine.
3. **Detection:** `python3 tools/ci.py mode` prints a recommendation, its
   reasons and the next commands. It picks local mode with at least 8 GiB of
   RAM (or cgroup limit), 4 cores and 10 GiB of free disk. Otherwise it picks
   cache mode when the sandbox cache fits: x86_64 Linux, a token, and 6 GiB
   free under `/home/user/.cache`. Failing that, CI mode.

State the mode and the reason in the first report of a session, so the user
can overrule it, e.g. "CI mode: 1.9 GiB RAM, 2 cores".

On a low-end machine, local mode still works; it is just slow. The first
build takes about half an hour. Run every cargo command through the
machine's memory cap, one compiler at a time. In the sandbox, a restored
cache cuts that first build down (see "The sandbox cache" below).

## Local mode

- Run the gates in AGENTS.md before each commit, and take shots with the
  `shot` example.
- Push when green. **Do not poll CI.** It runs the same checks, so its answer
  is almost always green, and waiting on it wastes the round.
- CI also builds on macOS and Windows, which a Linux machine cannot run
  locally. Glance at it with `python3 tools/ci.py watch --once` at the start
  of the next session, or before a release; a red result there is a real
  bug.

## CI mode

The loop, per change:

1. **Run the cheap checks locally.** They compile nothing:
   - `python3 tools/comment_check.py <files>`;
   - `python3 tools/commit_check.py --range origin/main..HEAD`;
   - `cargo fmt --all --check`, if a toolchain is installed (rustfmt only
     parses).
2. **Commit on a `<type>/<slug>` branch and push it.** CI runs on pushes to
   those branches (`feat/`, `fix/`, `perf/`, `refactor/`, `docs/`, `test/`,
   `build/`, `ci/`, `chore/`), so `main` only ever takes green commits.
3. **Watch the quick verdict:** run `python3 tools/ci.py watch`. A branch
   push runs one job, `Check` (see "Quick and full"). It stops at the
   first red job and prints the failing step: the command, its output and
   the error. Exit code 0 means green, 1 red, 2 still running. On a long
   wait, use `--detach` and read progress later with `watch --tail`; stop it
   with `watch --stop`.
4. **Red:** fix it in a new commit, push, and watch again. Never re-run a
   green run, and never re-run a red one hoping it turns green.
5. **Final:** when the slice is done, run `python3 tools/ci.py final`. It
   dispatches the full profile and waits for every job. Only a green final
   (or a green pull-request run) may merge.
6. **Green final:** merge into `main`, either by fast-forward
   (`git push origin HEAD:main`) or through a PR, then delete the branch. CI
   runs again on `main` for the same commit; there is no need to wait for it.
7. **Look at the result:** run `python3 tools/ci.py shots` for any visible
   change, at the four zooms ("Review every update").

What CI covers, against the local gates:

| Gate | CI job | Profile |
|---|---|---|
| `cargo fmt --all --check`, shape, comments, commit subjects | Check | quick and full |
| `cargo clippy --workspace --all-targets -D warnings` | Check | quick and full |
| `cargo test --workspace --no-fail-fast` (lavapipe for GPU tests) | Check | quick and full |
| `cargo xtask layers`, `trapped`, `parity`, `assets`, `unsafe` | Check | quick and full |
| `cargo xtask wasm` | Rust / wasm | full |
| every commit builds alone (`--locked`) | Build / each commit | full |
| clippy on macOS, with and without default features | Build / macos-latest | full |
| native macOS and Windows builds | Build / macos-latest, windows-latest | full |

**Hybrid.** In CI mode on a machine whose cache is warm, one targeted local
command is fine for a tight loop (for example
`cargo test -p mareader-ui-egui some_test`, under the memory cap). The gates
still come from CI. A restored sandbox cache holds lint builds only, so a
local test first needs `cache build --full`.

**Superseded runs.** A newer push to the same branch cancels the older run.
`watch` reports that run as superseded, not red; watch the branch head
instead.

## Quick and full (2026-11-11)

The dev loop waits on one job; the merge waits on all of them.

| | Quick | Full |
|---|---|---|
| Runs on | a push to a `<type>/<slug>` branch | pull requests, `main`, the schedule, `[ci full]` in the head commit, `ci.py final` |
| Jobs | `Plan / profile`, `Check / fmt, clippy, test` | those, plus `Rust / wasm`, `Build / each commit`, `Build / macos-latest`, `Build / windows-latest` |
| Stops | when Check ends; every step runs past a failure | when every job ends |
| Command | `python3 tools/ci.py watch` | `python3 tools/ci.py final` |

Why the loop was slow, measured from the GitHub API on 2026-11-11 over
the 25 runs on `refactor/engine-g2-reducer`:

- **No Rust cache existed.** `GET /actions/caches?key=v0-rust` returned 0
  entries; the repository held 15,316 caches using 10.74 GB of its 10 GB.
  15,310 were `sccache/*` entries from the deleted
  `refactor/ipa-rule-engine` branch. They evicted main's caches (LRU). Work
  branches could not save their own (`save-if` on main only), and main's
  env differed anyway. So every job compiled all ~300 dependencies at
  `opt-level = 2`: `rust-cache` restored in 0–8 s (a miss), Windows
  `cargo build` took 24 min, `cargo test` 13.7 min, and green runs took
  19–25 min from push to verdict.
- **Cache GC never ran.** `delete` and `schedule` triggers fire only from
  the default branch, and `cache-gc.yml` existed only on work branches.
  Its `--limit 1000` would also have left 14,000 entries.
- **The sandbox cache trailed the work.** It was rebuilt only after a green
  push to `main`, last on 2026-11-07 at `bf2cd85`, 32 commits behind the
  branch, which is past `restore`'s 30-commit staleness line.
- **One failure per round.** `cargo test` stopped at the first failing
  binary, and lint and test were separate jobs, each stopping at its first
  failing step.

The fixes: one quick job whose steps all run; Check saves its cache on
pushes; Cache GC runs from `main`; and the sandbox cache follows every green
push.

First measurements on the branch, 2026-11-12: the cold quick gate (its first
push, seeding `linux`) took 20.4 min; the next push, warm, took **5.0 min**,
inside the 6–9 min estimate. The sandbox cache job took 11.7 min seeded and
published a 1.61 GiB artifact; restoring it here took 3.9 min (1652 MB at
8.1 MB/s) and the first scoped clippy after it took 1.1 min. A final on a
warm Windows cache is still unmeasured.

## Cache budget

GitHub keeps 10 GB of Actions caches per repository and evicts the least
recently used beyond that; an entry unused for 7 days expires. What CI
needs, roughly: `linux` (Check, main and one or two branches), `wasm`,
macOS and Windows (main only), apt `.deb`s, and the voice-model caches of
the Remote tasks. Cache GC deletes caches of deleted branches and closed
pull requests at once, and sweeps dead refs and any `sccache/*` key
daily, 700 deletions per run (GITHUB_TOKEN allows 1,000 requests an hour).
Until GC runs from `main`, the owner can clear the leftovers once with
`gh cache delete --all -R codewiththiha/mareader2`; the next push to
`main` rebuilds the caches CI needs.

## CI time (before 2026-11-11)

Measured on 2026-11-10 from the **20 most recent completed CI workflow runs**,
then available from 2026-11-10 04:05 through 12:50 UTC. Each cell is the median
of that job's executed attempts; skipped and cancelled attempts are excluded,
so `n` varies. These are job durations, not push-to-verdict promises: queueing,
cache state and parallel jobs still determine wall-clock time. The current
critical path is `Rust / test`, with a 15.2-minute median.

| Job | Median | Executed attempts |
|---|---:|---:|
| Gate / no compiler | 15.5 s | 18 |
| Rust / lint | 5 min 14 s | 7 |
| Rust / test | 15 min 11 s | 7 |
| Build / macos-latest | 16 min 41 s | 5 |
| Build / windows-latest | 24 min 12 s | 5 |
| Gate / shape + fmt + changed crates (former job) | 3 min 38 s | 2 |
| Check / macos-latest (former job) | 5 min 8 s | 2 |
| Check / windows-latest (former job) | 7 min 7 s | 2 |

What made a change take an hour or more, and the fix for each:

- **The Ubuntu mirror stalled.** In 7 of the 16 runs, `apt-get update &&
  install` hung for 4–29 min in one job, and that job set the verdict
  (15–36 min). Shots stalled the same way. **Fix:** `.github/actions/apt`
  installs cached `.deb` files, keyed on the runner image and the package
  list. It asks the mirror only on a miss: once per new image, with
  timeouts and retries. Every apt install in `ci.yml` and `remote.yml`
  goes through it. Add a package through its `packages` input, never with
  a bare `apt-get`.
- **The sandbox cache was stale.** It was refreshed only by hand, and had
  fallen 612 commits behind. After each reset, the first local clippy took
  12–14 min instead of under one. **Fix:** CI's last job,
  `Cache / sandbox`, starts a cache build after every green push. It takes
  2–8 min on a runner, and nobody waits for it: `watch` gives its verdict
  without that job. The cache holds lint builds only (1.2 GB, not 5.6), so
  a reset now costs a 2–3.5 min restore and a 28 s first clippy. The cache job then deletes all but the newest
  two caches. `cache restore` warns when the cache it unpacked is more than
  30 commits behind. The cache job's wasm lint only warns: the browser
  preview is best-effort, and it once failed every refresh unnoticed.
- **Full debug info.** Every test binary linked full DWARF for the
  workspace crates. CI now builds with
  `CARGO_PROFILE_DEV_DEBUG=line-tables-only`, and `cargo test` fell from
  6.6 min to 4.3–5.5 min. A panic's backtrace still names files and
  lines. The shots and memory jobs set the same, so they
  still reuse the test job's build cache.
- **Prose-only pushes.** Changes to `docs/` and the top-level `*.md` files
  (AGENTS.md, README.md) skip CI.

Checked, and left alone:

- **Other workflows.** `Remote` and `Installers` run only when dispatched.
  They never queue ahead of CI, and a newer push cancels an older CI run.
- **The macOS and Windows builds.** They run in parallel and finish before
  the test job, so they add no time.
- **Testing only the crates that changed.** Almost every change touches
  `mareader-ui-egui`, the top crate, which nearly every test builds.
- **More sharing of the GitHub cache.** Workspace crates still compile in
  each job, because a fresh checkout gives every source file a new time.
  Only the dependencies come from the cache, and they restore in under a
  minute.

The fastest loop, per change:

1. **Lint locally only with a warm cache.** After a restore that is not
   stale, `cargo clippy` takes about 30 s. With a stale cache, or none,
   skip it: CI's lint job answers in about 2 min.
2. **Push, then start `watch` and the change's `shots` together.** Shots
   build from the same commit and do not wait for CI.
3. **A red job ends the watch at once,** with its digest. Fix it, push, and
   watch again.

Expect about 7–9 min per cycle, from push to a green verdict and shots.
Changing the cache key (the toolchain, `Cargo.lock`, or a `CARGO_*`
variable in a workflow) makes the next run rebuild every dependency once,
taking about 20 min.

## Screenshots from CI

```sh
python3 tools/ci.py shots \
  "grid pdf=sample:20 workers=0 texture=grid gap=false scroll=900" \
  "dark pdf=sample:20 workers=0 theme=dark panes=2"
```

- **Format.** Each argument is one shot: a file name, then the `shot`
  example's `key=value` options (see `crates/ui-egui/examples/shot.rs`).
  `workers=0` renders pages synchronously, so page content appears in the
  shot; without it, the shot may show blank pages.
- **Any setting (2026-10-22).** `set=<dotted.path>:<json>` writes one
  settings field through `Settings::set_path`, then sanitizes, for
  example `set=layout.progress_bar:false`. Every switch can be
  shot on and off without a new key; an unknown path fails the shot.
- **What happens.** The `Remote` workflow renders on lavapipe, the same
  software GPU the tests use. It reuses Check's `linux` build cache and
  uploads only the PNGs, kept for 3 days.
- **Where they land.** In `target/remote/shots/<name>.png`; open them with
  `read_file`.
- **The commit must be pushed.** Shots render a pushed commit (`--rev`,
  default `HEAD`), so push the work branch first. Uncommitted edits are not
  in the shot.

This beats building the browser preview and screenshotting it in a local
Chromium. The native renderer is the product; a wasm build here costs a
second cold build for another target, a wasm-bindgen install, and a
~150 MB browser plus its system libraries.

## The browser preview from CI

`python3 tools/ci.py web` builds `tools/web.sh`'s output in CI, with the
wasm-bindgen version that `Cargo.lock` pins. It also tries a headless-Chrome
screenshot, `preview.png`; that part is best-effort. Everything lands in
`target/remote/web/` (or `--out DIR`). To show the user the live preview:

```sh
python3 -m http.server -d target/remote/web 8000 --bind 0.0.0.0
```

Use it only to check the browser preview itself. Desktop changes are
checked with shots.

## Memory from CI

`python3 tools/ci.py memory --ref BRANCH --rev SHA` runs the Remote memory
task: two PDFs and two EPUBs read in rounds, a 3,000-PDF library and 20,000
books without files, then the EPUB layout test. It takes about 15 minutes.

- **Where it lands.** `target/remote/memory/memory.md` (each workload's
  rows and budgets) and `layout.txt`; the run's summary shows the same.
- **The verdict.** The command returns 1 when any budget reads `**no**`.
  Piped through `sed` or `grep`, that status is lost, so read the file.
- **What the numbers mean.** `docs/memory.md`, including why RSS on CI's
  software GPU counts what a real GPU keeps in VRAM.

## Kokoro parity from CI

`python3 tools/ci.py kokoro --ref BRANCH --rev SHA` runs `tools/kokoro-parity`,
which loads the pinned Kokoro model twice: once through `mareader-kokoro`, once
through the reference Rust build it was ported from. Both get the same phonemes,
voice and frame timing.

- **Activation.** The reference runs ALBERT's feed-forward with exact GELU.
  PyTorch's Kokoro, left at ALBERT's default, and Kitten's exports use its
  tanh form, as the app does; here ours is set to exact GELU to compare alike.
- **Exact stages.** Phoneme encoder, projection, duration encoder, raw and
  rounded durations, pitch and energy curves, text encoder. Each is "same"
  under 1e-3 relative error, "close" under 1e-2, else it fails.
- **Audio.** The noise source is seeded here but random there, so waveforms
  can't match sample for sample. Takes must agree in length (within 2%), RMS
  (within 0.9 to 1.1 times), 10 ms loudness curve and waveform (correlation
  above 0.98 each; two reference runs reach about 0.994). This holds for the
  decoder alone and for whole texts, chunks and pauses included.
- **Generator steps.** The tool carries a copy of the reference decoder that
  draws noise as ours does. Fed the same seed, every generator step is listed
  side by side, and the final audio must correlate above 0.99. Noise
  diagnostics show how far reseeding or silencing the noise moves each take.
- **G2P.** `mareader-g2p` against `loqui-g2p` 0.4.0, the crate it ports, in
  both dialects: prose, tricky sentences, thousands of numbers, years, money,
  a third of each lexicon with cased and suffixed forms, and made-up words for
  the neural model. Every input must give the same phonemes and tokens. The
  G2P files download with the model; `kokoro-parity <dir> <out> g2p` runs
  this part alone.
- **Builds.** Each smaller build (16-bit and 8-bit GGUF, 8-bit ONNX) speaks
  the same phonemes as the full weights, noise seeded alike. Rounded durations
  can shift a frame, so the gates are loudness curve, length and RMS: Half
  above 0.98 within 2% and 10%, Small above 0.95 and Tiny above 0.8, both
  within 5% and 15%. Waveform correlation and matching durations are listed
  too. Tiny's coarser 8-bit file drifts as far under onnxruntime, so it is
  also run against the reference loaded with its own weights: every stage
  must agree as Full's do, and the audio on equal noise above 0.99.
  `kokoro-parity <dir> <out> builds` runs this with the G2P part.
- **Where it lands.** `target/remote/kokoro/summary.md`, plus each take as
  WAV to listen to. Speed rows give real-time factors on CI's CPU.
- **The model.** Fetched from its pinned revision, checked by SHA-256 and kept
  in the Actions cache.

## Piper parity from CI

`python3 tools/ci.py piper --ref BRANCH --rev SHA` runs `tools/piper-parity`
on six pinned voices: low, medium and high quality, American and British, one
with 109 speakers. Our G2P writes each sentence's phonemes and ids once; both
sides speak those same ids.

- **Noiseless.** With both noise scales at 0 the model is deterministic, so
  `mareader-vits` must match onnxruntime running the voice's own ONNX file:
  the same length to the sample (in at least 80% of sentences, since a
  duration can round across a frame) and waveform correlation of 0.999 or more.
- **With noise.** The noise differs, so totals are compared: length within
  15% and RMS within 25% of the reference.
- **Phonemes.** Our G2P's phonemes, respelled for the voice's espeak style,
  against espeak's own for the same text; the error rate must stay under 25%.
  espeak and onnxruntime come from `piper-tts` in this job only; neither is
  linked into or shipped with the app.
- **Where it lands.** `target/remote/piper/summary.md`, with load time and
  real-time factors for both sides, and the first takes as WAV.
- **The voices.** Fetched from their pinned revision, checked by SHA-256 and
  kept in the Actions cache.
- **The engine.** The app's own Piper engine then speaks two sentences from
  two voices laid out as downloads leave them, and the summary gains when the
  first sound was ready. The kokoro task does the same for Kokoro.

## Kitten parity from CI

`python3 tools/ci.py kitten --ref BRANCH --rev SHA` runs `tools/kitten-parity`
on Kitten's four pinned builds: Nano in full precision and 8-bit, Micro and
Mini. Our G2P writes ten sentences' phonemes and ids once, the voice rotating
through all eight; both sides speak those same ids at speed 1.

- **Seeded alike.** The reference graph's random nodes are swapped for our
  seeded noise and fixed phases, so `mareader-kokoro` must match onnxruntime
  on full-precision Nano: equal durations and length in at least 80% of
  sentences and waveform correlation of 0.99 or more. Silenced noise would not
  do: unvoiced stretches then turn constant, and their spectrum's empty bins
  get phases from rounding alone.
- **Weights by name.** Every build must yield each weight full-precision Nano
  does, since a missing norm or bias would otherwise load as its default. An
  export that merged identical norms drops their nodes; those count apart, as
  Mini's norms are all 1 and 0.
- **Every build.** Total length within 10% and RMS within 25%. The 8-bit
  builds round activations as onnxruntime runs them, so their waves differ.
- **Phonemes.** Ours against espeak's, as for Piper, under 25% error. espeak
  and onnxruntime are installed in this job only.
- **Where it lands.** `target/remote/kitten/summary.md`, with the sizes read
  from each file, load time, real-time factors and the first takes as WAV.
- **The engine.** The app's own Kitten engine then speaks two sentences from
  a Nano build laid out as downloads leave it.

## Other languages' phonemes from CI

`python3 tools/ci.py ipa --ref BRANCH --rev SHA` builds `mareader-ipa`'s
`ipa` example and runs `tools/ipa-parity/check.py`: 500 public-domain
sentences a language, our phonemes against espeak-ng's, which Kokoro's Spanish,
Italian, Portuguese, French and Hindi voices were trained on.

- **As the voices read them.** espeak's side takes misaki's steps for Kokoro,
  ties written as single letters; both sides keep only Kokoro's vocabulary.
- **The gate.** Phoneme error at most 0.5% for Spanish, whose spelling gives
  its sounds and stress, and 5% for Italian, where stress and open vowels are
  partly a matter of the word. Portuguese and French may err 2%: open vowels
  and links between words follow the word there too. Hindi may err 2%, where
  the unsaid `a` and stress follow the shape of the word.
- **The danda.** espeak says nothing for `।`; our rules read it as a full
  stop, so Kokoro pauses there. The check hands espeak a full stop instead.
- **Where it lands.** `target/remote/ipa/summary.md`, with the most frequent
  words that differ. espeak-ng and phonemizer are installed in this job only.

## Chinese phonemes from CI

`python3 tools/ci.py hanzi --ref BRANCH --rev SHA` builds `mareader-hanzi`'s
`hanzi` example and runs `tools/hanzi-parity/check.py`: 500 sentences, most
from two public-domain novels, the rest numbers, punctuation and mixed
scripts, our phonemes against misaki's `ZHG2P()`, the reader Kokoro-82M runs.

- **The same data.** The check fetches the dictionaries from the commits the
  app pins, sizes and checksums checked; misaki runs with jieba, pypinyin and
  cn2an at those versions.
- **The gate.** Phoneme error at most 0.1%: this is a port, not rules of our
  own, so it should agree word for word, spaces included.
- **Where it lands.** `target/remote/hanzi/summary.md`. misaki and its
  Chinese tools are installed in this job only.

## System voices from CI

`python3 tools/ci.py system --ref BRANCH --rev SHA` runs `mareader-speech`'s
system-voice tests on a macOS and a Windows runner, where the system's own
speech works; the Linux test job has none.

- **Opened again.** Read aloud stopped and started, or the settings tab
  reopened, opens the system's voices anew. macOS lets its speech open only
  once a run, so every system voicer shares that one.
- **Side by side.** The settings tab can sample a voice while a book is read:
  each keeps its own voice and tone, and stopping one never stops the other.
- **Where it lands.** `target/remote/system/summary.md`, a verdict per system.

## The sandbox cache

After a sandbox reset, local mode would start with a 30-minute cold build.
`python3 tools/ci.py cache restore` instead downloads a build that CI made
and unpacks it here. The build covers what the sandbox runs locally:
clippy on the workspace and on wasm, plus the xtask gates. It ships the
toolchain too. `python3 tools/ci.py cache build` refreshes it, and
`--full` adds the test binaries and the `shot` example for local mode.

**Use it only when it is needed.** Each step costs minutes:

- **Restore** only on a cold machine (after a reset) when local work is
  wanted: local mode, or CI mode's hybrid loop. In plain CI mode nothing
  compiles locally, so skip it. A warm machine never needs it.
- **Build** rarely by hand. CI's `Cache / sandbox` job already starts a
  cache build after every green push, and keeps the newest two caches
  ("CI time"). Build by hand only when `restore` finds no live cache, or
  when a commit that never went green must be cached. An older cache is
  still worth restoring: dependencies are reused, and only crates edited
  since then recompile.
- **`--cold`** only to move to a new toolchain or to drop accumulated
  leftovers. The cache ships its own compiler, so it keeps the one it was
  first built with until then.

- **What is inside.** The `rustup` toolchain (with the wasm and macOS
  targets, clippy and rustfmt), the cargo registry and the `target-mr`
  directory, packed with zstd. The artifact is kept for 30 days. With
  `--full`, `target-mr` holds every test binary: 23 GB unpacked and
  5.6 GB to download (2026-10-27), against 7 GB and 1.7 GB on 2026-10-14.
  That is why the sandbox's default cache builds lint only.
- **Why a build takes minutes, not 13.** Normal CI jobs start from
  GitHub's saved build and only compile our crates. That saved build sits
  at other paths with other settings, so cargo cannot reuse it here. A
  cold cache build therefore compiles all ~300 crates on one machine. A
  normal cache build instead downloads the previous cache inside GitHub
  (100 MB/s, about 30 s), compiles only what changed, and repacks.
- **Why `target-mr` sometimes starts afresh.** Cargo never deletes a stale
  artifact. It names a dependency's files by version, so a lock change
  leaves the old files behind: one seed across 612 commits carried 1.1 GB
  of them. The seed step therefore keeps the toolchain and the registry,
  but drops `target-mr` when `Cargo.lock` or the build set (lint or full)
  differs from the cached one. That build takes 12–17 min on a runner,
  and nobody waits for it. A workspace crate keeps its file names across
  edits, so the other refreshes do not grow the cache.
- **Why it can be reused.** Cargo keys its cache on absolute paths, the
  compiler version and the build profile. So the CI job clones the repo to
  `/home/user/mareader2` and uses `/home/user/.cache/{rustup,cargo,target-mr}`
  with `capped.sh`'s profile (`debug = 0`). It builds with the toolchain it
  then ships.
- **Incremental compilation stays on.** It is part of the profile hash, and
  the runner turns it off (`CARGO_INCREMENTAL=0`). With it off, every
  workspace crate rebuilt after a restore, though no dependency did. The
  job sets `CARGO_INCREMENTAL=1` to match cargo's default here.
- **Why nothing recompiles.** Cargo also compares file times. The cache
  records every tracked file's time. `restore`, and the next cache build's
  seed step, give each file that is unchanged since the cached commit its
  recorded time back. So only crates edited since then recompile.
- **When to use it.** A fresh sandbox where the user wants local mode, or
  CI mode with a warm cache for the hybrid loop. A stale cache still works:
  only what changed recompiles. Past about 30 commits, though, that is
  most of the workspace, and `restore` says so. Lint in CI then.
- **Limits.**
  - It fits only a machine with this exact layout (x86_64 Linux, the paths
    above); anywhere else, cargo would rebuild from scratch.
  - It refuses to overwrite an existing cache unless you pass `--force`.
  - It needs about 4× the archive size in free disk while unpacking.
  - It installs `zstd` with `sudo apt-get` if it is missing.

GitHub's own build caches (`actions/cache`, `Swatinem/rust-cache`) cannot
be downloaded outside a runner; `gh` has no command for it. That is why the
cache travels as a workflow artifact.

## The token

- **Reads need no token.** This repository is public, so `watch` and
  `mode` work without one, at 60 API requests an hour.
- **Other commands need one.** Dispatching (`shots`, `web`, `cache build`)
  and downloading artifacts need a token with Actions read and write and
  Contents read.
- **How to pass it.** Set `$GITHUB_TOKEN` or `$GH_TOKEN`, or pass
  `--token-file PATH` before the subcommand. The tool never writes the token
  anywhere. Never commit it, and mask it in any output you paste.

## Troubleshooting

- **"is not on GitHub: push it first."** Shots, web and cache work from
  pushed commits only. Push the work branch first.
- **"no workflow runs appeared."** The push may have touched only `docs/`,
  which CI ignores, or the branch name may not match a `<type>/` prefix.
- **HTTP 403 when dispatching.** The token lacks Actions write, or it has
  expired.
- **`--offline` says "failed to download" after a restore.** The cache
  leaves out the `.crate` archives, and cargo wants them even with the
  sources unpacked. Run `cargo fetch` once, which takes about 3 s. A
  private git dependency needs the token for that fetch.
- **Restore says files "already exist."** The machine is already warm. Pass
  `--force` only if the cache there is broken.
- **Crates recompile after a restore.** Run `cargo build -v`. A "Dirty"
  line names a stale file or setting. A crate compiling with no "Dirty"
  line has a new hash: compare its JSON under `target-mr/debug/.fingerprint/`
  with the restored one. A differing `profile` field means a profile
  setting differs, as `CARGO_INCREMENTAL` once did. If all dependencies
  recompile instead, the paths or the toolchain differ; `restore` prints
  the cached `rustc`.

## What only a real machine can check

Headless tests prove Mareader asks for the right thing: a caption click,
a drag on the empty bar, a press on a frameless edge each send their
window command (`crates/ui-egui/tests/chrome.rs`). Whether the system then
moves, resizes or closes the window is eframe's and the OS's, so the
owner checks these on real machines before 1.0:

| Check | Where | How |
| --- | --- | --- |
| Drag the empty title bar; double-click it | every OS | the window follows; it maximises and restores |
| Resize from each edge and corner | Windows; Linux on Wayland and X11 | the cursor changes, the window resizes |
| Caption buttons | Windows; GNOME | minimise, maximise and restore, close |
| Traffic lights | macOS | sit in the bar at x 20, slide with the sidebar |
| Open from the file manager | every OS, installed | the book opens in the running window |

**2026-10-17, macOS:** the owner ran the Mac rows on a real Mac with an
installed build: the bar drags and double-clicks, the traffic lights sit
in the bar and slide with the sidebar, and books open from Finder into
the running window. All passed. Each installer's launch is also checked
in CI on every OS (`docs/packaging.md` § Installers from CI).


### Review batch transition

The S0 branch proposes a pinned toolchain, a no-compiler gate, main-only build
cache writes and cache garbage collection. Timings and cache behavior are not
verified until the owner authorizes the final merged CI run. The review's
one-time cache purge is an owner operation and has not been performed here.
