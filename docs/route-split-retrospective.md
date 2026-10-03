# The route split, three times — a retrospective

Three designs for "one WASM per route, mounted and unmounted instantly, with
the other route's memory gone" were built in turn. The first was
correct about memory and too slow to use; the second was instant and kept
every byte; the third is instant and lets the memory go. This document
records what each one was, what it cost to build, why it failed or works,
and what the numbers say — so the next person does not rebuild the first
two.

Everything here was reconstructed from the git history (`git log`,
the diffs and the docs at each stage), from the GitHub Actions run and job
records (509 runs), and from the CI-built `dist/` artifacts
of each era replayed in a local browser. Where a number is measured, the
source is named; where it is an inference from code, it says so.

## 1. Timeline

| | Version 1 — dispose, then boot | Version 2 — warm slot + recycle | Version 3 — intent warming + eviction |
| --- | --- | --- | --- |
| Commits | `9a9381d` … `e6b7a65` (reader runtime, the split, frames, transport, build contract, dev flow) | `861d518` … `da06bff` (`perf(shell): keep a runtime warm`, `perf(shell): recycle frames and relay cover bakes`, then zoom/noise polish) | `b737644` … `a2aa19a` (`refactor(shell): bake covers in a shell frame, warm reader on intent` and seven follow-ups) |
| CI window (UTC) | 2026‑09‑24 10:34 → 09‑26 06:50 | 2026‑09‑28 00:46 → 08:52 | 2026‑09‑28 11:20 → 12:04 |
| Route switch | cover the host, dispose the outgoing frame, await it, create the next frame, boot, open | reveal a frame that booted behind the screen; the one just left is kept and re-armed in place | same reveal, but the reader boots on shelf intent, is evicted after an idle window, and covers never touch it |
| Memory after a read | freed with the frame, at once | never freed: the reader frame lives as long as the app | freed with the frame, ≤ idle window (60 s) or at once past a heap ceiling |
| What was wrong | every switch paid a page load + wasm instantiation + Leptos mount + pdf.js, behind two visible loading states | acted like the unified app: the reader realm, its wasm linear memory and pdf.js stayed resident; a reader was booted 700 ms after every library paint even if no book was ever opened; covers were baked in that reader | — (trade‑offs in §4.4) |

Five earlier attempts at the same goal (2026‑09‑20 → 09‑23) ran 361 CI
runs, 257 of them failures, 1,016 wall‑minutes, none of it merged. They are
not analysed here.

## 2. Version 1 — dispose, then boot

### 2.1 What it was

Two sub‑stages, both "one runtime alive at a time":

- **1a, same window** (`8bbda0a feat(runtime): split shell, library, and
  reader runtimes`): the Shell dynamic‑imported `/reader.js` or
  `/library.js` into its own window (`dyn_import`), called the module's
  exported `…_start(host, launch_json)` / `…_dispose(session_id)`, and
  talked to runtimes through `window.__mareaderShell`. The document written
  for it (`docs/runtime-split.md` at that commit) already admits the flaw:
  *"A module's linear memory stays reserved after its session ends — that
  is the compiled‑code cache."* In one realm a wasm instance is never
  collectible while its module namespace is reachable, so this design could
  not free memory by construction. It lasted about a day.
- **1b, shell‑owned iframes** (`3a87309 feat: host runtimes in shell-owned
  frames`, `d1f06e8` transport, `1850048 refactor: delete the same-window
  runtime transport`): each runtime boots in its own iframe
  (`/reader.html?hosted=1&g=<generation>&n=<nonce>`), pairs with the Shell
  over a `MessageChannel`, and is disposed as a unit by removing the frame.
  This is the boundary all later versions keep, and it is the right one:
  a removed frame's realm — wasm instance, JS heap, pdf.js worker, DOM — is
  garbage.

The route switch in 1b (`run_start`, `src/app/manager.rs` at `e6b7a65`):

```text
paint the loading card → dispose_active().await   (Dispose → DisposeComplete round trip)
→ clear host → new iframe → module load → wasm instantiate → Leptos mount
→ Ready → (reader) pdf.js load → document open → Painted lifts the card
```

Nothing overlapped. The user saw the Shell's loading card, then the
reader's own "Opening" state: the "two loading stages". The Shell also
carried `pdf-engine` itself to bake shelf covers (`bake_for_library`
called `pdf_engine::api::cover_data_url` in the Shell page), so the
"no‑engine Shell" of later versions did not exist yet.

### 2.2 Why it was too slow

Measured by replaying the CI artifact of Deep CI #193 (`0159f09`, the last
1b build with an artifact) in headless Chromium, warm HTTP cache, 2 vCPU —
a floor, not a desktop number (see §5 for the method):

| switch | v1 | v2 | v3 |
| --- | --- | --- | --- |
| click on the shelf → reader on screen | **490 ms** | 128 ms | 158 ms |
| click → document rendered (text PDF, 430 pages) | **968 ms** | 648 ms | 675 ms |
| click → document rendered (scanned PDF, 65 MB) | **888 ms** (1,753 ms in a memory‑starved run) | 580 ms | 657 ms |
| close → library on screen | **423 ms** | 14 ms | 12 ms |

Half a second before anything is on screen and a third of a second to get
back to a shelf that was there a moment ago, on a machine with the artifact
in cache and nothing else running. In a WebView on a laptop, with a cold
disk cache and a real book, it is the "too much time" the user reported.
The cost is structural, not a bug: a disposal awaited before a boot, and a
boot that is a page load.

### 2.3 What it got right

Memory. With the frame gone the renderer returns to its shelf baseline
within seconds (§5.2: 232 → 134 MB PSS after a forced GC for the text
book, 368 → 162 MB for the scanned one, and `frames: [library]` only).
Every later version had to reproduce this property, and version 2 lost it.

### 2.4 What it cost to build

From the Actions records, 09‑24 10:34 → 09‑26 06:50:

| | v1 |
| --- | --- |
| workflow runs | 308 (96 success, 162 failure, 50 cancelled) |
| distinct pushed commits | 156, of which **25 came out green** |
| longest run of consecutive red pushes | **44** |
| wall‑clock minutes on runners | 889 (1,402 job‑minutes) |
| job failures by lane | browser lifecycle 83 · clippy 72 (+11 macOS) · tests 52 · rustfmt 42 · Tauri smoke 34 · web contracts 12 |
| median gap between pushes | 5.2 min; 123 of 155 pushes landed within 10 min of the previous one |

Two patterns account for most of it:

1. **CI as the compiler.** 42 pushes failed on `cargo fmt --check` and 83
   on clippy; the same subject *"feat(runtime): split shell, library, and
   reader runtimes"* was force‑pushed **47 times** (54 failed runs) as one
   amended commit. Between 14:38 and 16:05 on 09‑25, `feat: host the
   runtimes in shell-owned frames` was followed by 22 consecutive
   one‑line `fix:` pushes, each fixing the one lint the previous run
   reported (`fix: drop the offer's unused window binding`, `fix: call the
   port start without a unit binding`, `fix: close the lane without a unit
   binding`, …), all 23 of them red. Nothing was compiled or formatted
   before pushing.
2. **The blank window.** `trunk build` alone emitted the Shell page and left
   the runtime pages to 404, and the packaged app shipped a native window
   with an empty host (`40b5719 build: boot runtimes from one dist build`,
   `Keep the window covered until the runtime paints`, `Cover the host
   across the whole handover…`). The build contract
   (`tools/build-dist.sh`, `tools/check-runtime-artifacts.mjs`, the boot
   placeholder) was invented under that incident, one push at a time.

The lifecycle suite was also being written during the same window, so a
share of the 83 browser failures is the suite finding real races
(close‑during‑render, stale geometry on dispose) — that part was work, not
waste. The rest was the absence of a local check.

## 3. Version 2 — warm slot and recycle

### 3.1 What it was

`861d518 perf(shell): keep a runtime warm for instant route swaps` inverted
the order: the *other* runtime boots into a hidden warm slot behind the one
on screen, and a route change is a reveal:

```text
click → reveal the warm frame → Launch{document} / Refresh → retire the one just left (off the critical path)
```

`e8b1d18 perf(shell): recycle frames and relay cover bakes` then stopped
retiring: the runtime the user left becomes the warm counterpart *in
place* — kept intact for `RECYCLE_DELAY_MS` (1,200 ms, a straight return
gets the same session back), then its session is disposed through the full
exchange and a fresh, document‑less session is mounted in the same document
via `ShellFrame::Rearm`. After the first two boots a route change never
loads a page, fetches or compiles wasm, or loads pdf.js. The Shell dropped
`pdf-engine`; the shelf's cover bakes were queued by the Shell and run in
the warm **reader** frame (`ShellFrame::BakeCover` → `RuntimeFrame::CoverReady`).

This is the version the user described as "fast but no memory benefit", and
the one version 3 was built on top of.

### 3.2 Why it had no memory benefit

Read from `src/app/manager.rs` at `da06bff` and confirmed in the replay:

1. **The reader frame was never removed.** `retire_or_recycle` recycled;
   `run_recycle` disposed the *session* and re‑armed the *frame*. There was
   no idle timer. The only retirement was a heap ceiling
   (`READER_RECYCLE_HEAP_MAX`, 320 MiB) — below it, the realm lived for the
   life of the app. What a realm keeps when its session is gone: the wasm
   instance and its **linear memory at high‑water** (wasm memory never
   shrinks; `heapHighWaterBytes` is exactly that number), the compiled
   module, pdf.js and its worker glue, the Leptos runtime, per‑document
   memos outside the session (`PARSED_SPOTS` — fixed in v3 as `dd760a3`),
   plus whatever V8 has not compacted.
2. **A reader was booted whether or not it was wanted.** `schedule_warm`
   armed a 700 ms timer after every library paint and booted a reader
   behind the shelf. Opening the app to look at the shelf cost a reader
   realm. In the replay the shelf "at rest" already holds
   `['library:active', 'reader:warm']` (§5.2), and its footprint is the
   same as version 3's shelf *plus* a booted reader.
3. **The shelf depended on the reader.** Covers for the library were baked
   *in the warm reader*. The reader was therefore a dependency of the
   library route — the coupling the user asked to remove — and a bake in
   flight in that reader is work the user's own open has to share the
   frame with when the click lands.
4. **The gauges did not see it.** `atBaseline` read the reader's *digest*
   ("the last session says it drained") and was `true` with a full reader
   realm resident. The lifecycle suite asserted `reusedWarmFrame: true` —
   i.e. it asserted the reader was *kept* — and its "cover relay" stage was
   satisfied by the cover the reader's own open had already written
   (Deep CI #217 reported `coverRelay: {covers: 1, ms: 1}` — a real bake
   takes seconds; version 3 measures 5,064 ms). The relay's ask was in fact
   dropped at `dispatch_boundary`'s registry lookup, because a cold shelf
   asks before its Ready verdict admits it. Nobody could see that either.

So after a read the numbers looked like the unified app's: the session's
canvases and decoded images went (the document *was* closed), and
everything that is the reader stayed. `frames` after a return, sampled for
70 s: `['library:active', 'reader:warm']` at every sample, unchanged by a
forced GC (§5.2).

### 3.3 What it cost to build

09‑28 00:46 → 08:52 (this includes the zoom/noise fixes pushed on the same
day, which share the CI record):

| | v2 |
| --- | --- |
| workflow runs | 74 (35 success, 25 failure, 13 cancelled) |
| distinct pushed commits | 36, of which **6 came out green** |
| longest run of consecutive red pushes | 13 |
| wall‑clock minutes on runners | 241 (431 job‑minutes) |
| job failures by lane | browser lifecycle 24 · clippy 8 · tests 4 · Tauri smoke 4 · rustfmt 1 |

Better hygiene than v1 (one rustfmt failure, and a `ci: let a push skip the
work it cannot change` commit), but the same loop for the browser lane: the
two `perf(shell)` subjects were pushed 5 and 6 times each, and the untweened
zoom work that followed pushed `fixup!` commits ten times against a red
lifecycle lane. The measurements written into `docs/architecture.md`
("Measured, not assumed") were taken from the suite's own summary without
asking whether the suite measured the right thing — which is how a 1 ms
"relay" and a `true` baseline with a resident reader got recorded as proof.

## 4. Version 3 — intent warming, idle eviction, a transient bake page

### 4.1 What changed

The frame boundary and the reveal are version 2's. What changed is Shell
*policy* — when a frame exists — and one coupling:

| | change | commit |
| --- | --- | --- |
| R1 | no reader is warmed on library paint; the shelf sends `ExpectReader` when the pointer enters it (`#library-level` pointer‑over), and only then does a reader boot behind it | `b737644` |
| R2 | a warm or recycled reader that stays idle for `warmReaderIdleMs` (60 s; `?warmIdleMs=` for the suite) is retired: its session disposed through the normal exchange and **its frame removed**; renewed intent boots a fresh generation | `b737644` |
| R3 | a reader whose heap high‑water passed 320 MiB is retired at once instead of recycled (linear memory never shrinks, so re‑arming that realm keeps the high‑water) | `b737644` |
| R4 | covers are baked by the Shell's own hidden `bake.html` (`public/coverBake.ts`: pdf.js and the engine's cover render, no wasm, no runtime), one ask at a time, page removed 5 s after the queue drains; the reader is never booted for a cover, and the dependency gate forbids the reader set for the Shell and the library | `b737644`, `4f8f0e7` |
| R5 | `PARSED_SPOTS` (a per‑document memo outside the session) is forgotten on dispose, so a recycled frame does not carry it | `dd760a3` |
| R6 | the grain animation is paused in hidden (warm/retiring) frames | `9922fef` |
| R7 | pdf.js loads on the first document open, so a warm reader holds no engine | `335c0f7` |
| R8 | `atBaseline` is `false` while any reader frame is resident; `readerFramesResident`, `warmReaderEvictions`, `bakeFrameResident`, `coversAnswered` are probe fields the suite asserts | `b737644`, `49342ba` |

Two latent bugs surfaced only because the new stages are strict, and both
were reproduced against the CI `dist/` in a local browser before they were
fixed:

- **A cold shelf's cover ask was dropped** (since `da06bff`): the shelf
  sends `bakeCover` from inside its mount, before `ready`; the Shell admits
  a cold frame to its registry only on the Ready verdict, so the lookup in
  `dispatch_boundary` threw the ask away. The hosted backfill had never
  worked. The baker now keys the ask on the frame generation, boots the page
  at once, starts the bake on admission (`frame::register` →
  `bake::frame_registered`) and prunes it on teardown (`src/app/bake.rs`).
- **A document‑less reader never answered its dispose**: the promise was
  resolved only by the document‑close tail, so every idle eviction waited
  out the Shell's 8 s forced‑removal timeout (Deep CI #241: 10,105 ms for a
  2,000 ms window). `1fe32d2` resolves it at the end of every tail; the
  suite bounds the eviction to the window plus a dispose beat and refuses a
  forced removal (Deep CI #242: 2,019 ms).

### 4.2 Why it works

- **The unit of memory is the frame, and the frame is now time‑bound.**
  Version 1 proved a removed frame frees everything; version 2 proved a
  revealed frame is instant. Version 3 keeps the frame exactly as long as
  it is likely to be revealed: from intent until an idle window after the
  user left. Both earlier properties hold at once because they were never
  in conflict — only the policy "never remove" was.
- **Warming follows intent, not paint.** A user who reads warms a reader
  by moving toward the shelf; a user who only looks at the shelf never
  pays for one. The click still reveals a booted frame in the common case
  (the intent precedes the click by more than a boot).
- **The library owes the reader nothing.** Covers are the one PDF job the
  shelf needs and they are done by a page that is neither runtime; the
  dependency gate makes the separation a build failure, not a review note.
  The two routes share appearance and settings through persisted storage
  and nothing else.
- **The gauges tell the truth.** `atBaseline` cannot be `true` with a
  reader resident; the suite clears the covers before a cold shelf boot so
  the only way a cover can exist is the bake page; an eviction that takes
  the forced‑removal timeout fails the run. A wrong policy now fails CI
  instead of being recorded as measured.

CI evidence, Deep CI #242 on `a2aa19a`: `coverBake: {covers: 1,
sawBakeFrame: true, coversAnswered: 1}` with `readerFramesResident 0`;
idle eviction `2,019 ms` for a 2,000 ms window, 0 forced removals;
`reusedWarmFrame` 4/4 handoffs, `peakFrames` 2, host never empty; reopen
heap slope 0 B/cycle; `samePageRecycledOpens` 9/10.

### 4.3 What it cost to build

| | v3 |
| --- | --- |
| workflow runs | 14 (7 success, 4 failure, 3 cancelled) |
| distinct pushed commits | 7, of which 2 fully green (`3643dbf`, `a2aa19a`) |
| longest run of consecutive red pushes | 4 |
| wall‑clock minutes on runners | 51 (89 job‑minutes) |
| red pushes, each a distinct root cause | `7ab4edd` host stub missing `expect_reader` + clippy `question_mark`; `ab54a4e` `web_sys::ScrollBehavior` feature only came transitively; `bb859b3` the suite cleared the covers 2 ms before the reader's late cover write landed; `fd5851a` the bake‑before‑registration bug above |

Four red pushes is not zero; two of them were compile errors a local
`cargo clippy --target wasm32-unknown-unknown` would have caught, and the
sandbox this was built in has no Rust toolchain (a deliberate constraint of
the session, recorded in `docs/architecture.md`). The cleanup pass that
produced this document added one more (`cb97379`: dropping the workspace
root's dev‑dependency on `library-core` also dropped the `test-util`
feature it had been enabling for two other crates' tests as a side effect;
each now declares it). The difference from the
earlier eras is what happened after a red run: the CI `dist/` was
downloaded and served locally, Playwright drove the real build, the
`MessagePort` traffic was traced, and the fix addressed the cause — instead
of pushing the next guess.

### 4.4 Trade‑offs, stated

- A click that lands faster than a reader boot after the *first* shelf
  intent waits for that boot (the boot is ~300 ms on the replay machine).
  Every later click is a reveal, as in version 2.
- After a read, the reader's memory is gone within the idle window plus a
  dispose beat, or at once past the heap ceiling. Continuous pointer
  movement over the shelf keeps the warm reader alive — that is intent, by
  design.
- The library is still warmed behind the reader (it is cheap and it is
  where every close goes). It is the reader, not the shelf, that is
  time‑bound.

## 5. Measurements

### 5.1 Method

Each era's production build was taken from the `dist` artifact its own
Deep CI run uploaded (v1: Deep CI #193, `0159f09`, the last 1b build with
an artifact; v2: Deep CI #231, `0eb673d`, the last build before `da06bff`;
v3: Deep CI #244, `8ae1782`, the final build — the browser lane uploads
its dist on every run now, which is what made this comparison possible),
served by `tests/browser/server.mjs` and driven by
`tools/measure-route-switch.mjs` (removed with the reader and library
frames it measured; it lives in the history) in headless Chromium (Playwright's
`chromium-headless-shell`, Linux, 2 vCPU, 2 GB). Same scenario for all:

```text
seed one read book (?open=) → fresh library boot → 1.5 s of shelf intent (pointer-over)
→ click the book → flick through ~40 screens → close → hands off for 70 s → forced GC
```

Memory is the renderer processes' proportional set size (`Pss` from
`/proc/<pid>/smaps_rollup`, all `--type=renderer` processes of the
browser), which counts wasm linear memory, the V8 heaps, canvases and
decoded images. Absolute values vary by ±20 MB between runs (V8 and the code
cache are not deterministic); the deltas within a run and the frame lists
are the evidence. Two fixtures: a text PDF (the CSS 2.1 specification,
3.3 MB, 430 pages) and a generated scanned book (60 JPEG pages, 65 MB).

### 5.2 Results

Text PDF, PSS in MB and the frames in the host (`kind:slot`):

| moment | v1 | v2 | v3 |
| --- | --- | --- | --- |
| library at rest, before any read | 141 · `[library]` | 143 · `[library, reader:warm]` | 138 · `[library]` |
| after 1.5 s of shelf intent | 141 · `[library]` | 143 · `[library, reader:warm]` | 139 · `[library, reader:warm]` |
| reading | 232 · `[reader]` | 247 · `[library:warm, reader:active]` | 248 · `[library:warm, reader:active]` |
| +2 s after the return | 191 · `[library]` | 195 · `[library, reader:warm]` | 199 · `[library, reader:warm]` |
| +20 s | 138 | 160 | 161 |
| +62 s | 136 · `[library]` | 160 · `[library, reader:warm]` | 160 · `[library]` (evicted at the idle window) |
| after a forced GC | **134** · `[library]` | **150** · `[library, reader:warm]` | **136** · `[library]` |

Scanned book:

| moment | v1 | v2 | v3 |
| --- | --- | --- | --- |
| library at rest | 202 · `[library]` | 250 · `[library, reader:warm]` | 227 · `[library]` |
| reading (at the close click) | 369 | 354 | 349 |
| +2 s after the return | 188 | 197 | 191 |
| +62 s | 165 · `[library]` | 188 · `[library, reader:warm]` | 182 · `[library]` |
| after a forced GC | **162** | **179** | **161** |

What the tables say:

- All three versions release the *document* on close (the +2 s drop is the
  engine's destroy: decoded images, canvases, the worker). That is the runtime lifecycle's
  work and it was never the problem.
- Version 2 ends every run with a reader frame resident and 14–18 MB above
  version 3 after a GC. That gap is the fixed cost of a reader realm with a
  **tiny** heap: these fixtures leave the wasm heap at 1.6–5.3 MB. A real
  read — reflowed text, a search index, hundreds of pages — drives the
  linear memory to its high‑water and version 2 keeps all of it, because
  linear memory never shrinks and the frame never goes. That is the "RAM
  stays high for minutes" the user saw; the fixture shows the mechanism,
  not the size.
- Version 3 returns to the shelf's footprint once the frame is evicted
  (136 vs 134, 161 vs 162 — within noise of version 1) while keeping
  version 2's switch times (§2.2).

Latencies for the same runs are in §2.2. Deep CI's own numbers for the
three eras (its lifecycle summary): v1 (Deep CI #186) `rapidTransitions`
booted a new library *and* reader session per handoff (library sessions
3→6, reader 2→5); v2 (Deep CI #217) `reusedWarmFrame` 4/4 with library
sessions constant at 2 and `coverRelay {covers: 1, ms: 1}`; v3 (Deep CI
#242) `reusedWarmFrame` 4/4, `coverBake {covers: 1, ms: 5064}`, eviction
2,019 ms.

## 6. Comparison

| | v1 | v2 | v3 |
| --- | --- | --- | --- |
| route boundary | iframe per runtime (1b) | same | same |
| library → reader | cold boot, awaited disposal first | reveal | reveal (intent‑warmed) |
| reader → library | cold boot of the shelf | reveal of a warm shelf | reveal of a warm shelf |
| reader after a close | frame removed | frame kept for the app's life (rearmed) | frame kept ≤ 60 s idle, then removed; removed at once past 320 MiB |
| reader booted without intent | never | 700 ms after every library paint | never |
| who bakes shelf covers | the Shell, with `pdf-engine` in the Shell artifact | the warm reader (library → Shell → reader → Shell → library) | the Shell's transient `bake.html` (no wasm, no runtime) |
| library ↔ reader coupling | Shell carries the engine | reader is a dependency of the shelf | none beyond persisted settings/appearance; gate‑enforced |
| `atBaseline` honesty | frame gone ⇒ true | true with a reader resident | false while any reader frame is resident |
| switch to reader on screen (replay) | 490 ms | 128 ms | 158 ms |
| back to the shelf (replay) | 423 ms | 14 ms | 12 ms |
| memory after a read | baseline in seconds | baseline + a reader realm, forever | baseline within the idle window |
| CI: pushes / green / longest red streak | 156 / 25 / 44 | 36 / 6 / 13 | 7 / 2 / 4 |
| CI: runner wall‑minutes | 889 | 241 | 51 |

## 7. What to keep from this

1. **Separate the boundary from the policy.** The iframe boundary was right
   from 1b onward. Both failures were policies on top of it: "dispose
   before boot" and "never dispose". Memory and latency are decided by
   *when* a frame exists, and that is a small state machine in the Shell,
   not a rewrite.
2. **Measure the claim, not the proxy.** `reusedWarmFrame: true` measures
   reuse; the user's complaint was retention. A 1 ms cover "relay" cannot
   be a render. A baseline that cannot be false with a reader resident is
   not a baseline. Every version 3 stage asserts the user‑visible property
   (frames resident, eviction time, a cover that could only come from the
   bake page).
3. **Compile before you push, or replay before you push again.** 42
   rustfmt and 83 clippy failures are a local pre‑push hook; 44 red pushes
   in a row is a signal to stop and reproduce. The CI `dist/` artifact is
   downloadable and serves locally in one command — now on every browser
   run, pass or fail — and Playwright against it finds in minutes what a
   push‑and‑wait loop finds in hours.
4. **Latent bugs live where nobody is looking.** The cold‑shelf bake had
   been broken since `da06bff`; the document‑less dispose had waited out
   its timeout on every eviction. Both were invisible until a test refused
   to be satisfied by a side effect.

## 8. Legacy removed in this pass, and what was deliberately kept

Removed (nothing referenced them after version 3):

- `ShellApi::save_library` and `ShellApi::save_covers`, with
  `RuntimeFrame::SaveLibrary`/`SaveCovers`, the Shell's
  `FrameVocabulary` arms and `services::save_library`, the port and
  standalone implementations in both runtimes, and the recorder's
  `library_calls`. The shelf writes its blob and its cover cache itself,
  through the origin's one store; nothing ever sent these over the wire.
  With them gone `runtime-contract`, `frame-transport`, `reader-runtime`
  and the Shell no longer depend on `library-core` directly (it reaches
  them only through `storage`), and the Shell artifact lost 48 KB
  (`mareader_bg.wasm` 648,701 → 600,698 bytes between Deep CI #240 and
  #244).
- `app_state::memory::set_heap_sample_sink` and its thread‑local: a hook
  the split introduced for the reader diagnostics to register, which
  nothing ever registered; the high‑water mark is sampled by the
  diagnostics themselves.
- `library_runtime::standalone_settings` (never called).
- The reader's `ApiHandle::bake_cover` no longer forwards to the port — the
  reader never asks for a bake and the Shell refuses one from any frame
  that is not a shelf.

Both follow-up orphans from the split are closed. `DragOverlay` had no
caller and is deleted: the Shell paints the drop hint itself
(`data-import-drop` in `src/app/mod.rs`) from the signal
`install_import_drop` returns, and the component's stylesheet block went
with it. `install_window_state_bridge` was repaired rather than kept — the
live `AppTitleBar` installs its own `window_state` module, so the frameless
caption's maximize/restore glyph follows the window again.

## 9. Sources

- History: `git log --format='%h %ad %s' --date=short`; the v1 designs from `git show
  8bbda0a:docs/runtime-split.md`, `git show e6b7a65:docs/runtime-split.md`
  and `git show e6b7a65:src/app/manager.rs`; the v2 design from `861d518`,
  `e8b1d18`, `c722bd7`, `a3e3c8f` and `src/app/manager.rs` at `da06bff`.
- CI: the GitHub Actions REST API, all runs created since 2026‑09‑20 with
  their jobs and failed steps; eras are split by run creation time.
- Replays: the `dist` artifacts named in §5.1; `tools/measure-route-switch.mjs`
  (in the history).
