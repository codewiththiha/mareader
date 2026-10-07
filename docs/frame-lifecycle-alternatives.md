# Frame lifecycle: three designs, measured

Three designs answer the same question — *one WASM per route, mounted and
unmounted instantly, with the other route's memory gone*. The first was
correct about memory and too slow to use; the second was instant and kept
every byte; the third is instant and lets the memory go, and its frame
boundary and reveal sequence are what the app runs today. This document
records what each design is, why the first two fail, and what the numbers
say, so that nobody rebuilds them.

Policy moved on after the third design was drawn, and `docs/runtime-split.md`
is the authority on what the app does now: no intent warming, no warm shelf
slot, no `Rearm`. What this document keeps is the part that did not move —
where the boundary goes, and what a baseline has to be able to say. Every
figure below was measured on a CI-built `dist/` artifact replayed in a local
browser, or read off a Deep CI run; a measurement names its run and an
inference from code says so.

## Version 1 — dispose, then boot

### What it was

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
- **Shell‑owned iframes** (`3a87309 feat: host runtimes in shell-owned
  frames`, `d1f06e8` transport, `1850048 refactor: delete the same-window
  runtime transport`): each runtime boots in its own iframe
  (`/reader.html?hosted=1&g=<generation>&n=<nonce>`), pairs with the Shell
  over a `MessageChannel`, and is disposed as a unit by removing the frame.
  This is the boundary all later versions keep, and it is the right one:
  a removed frame's realm — wasm instance, JS heap, pdf.js worker, DOM — is
  garbage.

The route switch in that design (`run_start`, `src/app/manager.rs`):

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

### Why it was too slow

Measured by replaying the CI artifact of Deep CI #193 (`0159f09`, the last
1b build with an artifact) in headless Chromium, warm HTTP cache, 2 vCPU —
a floor, not a desktop number (see [Method](#method) for how):

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

### What it got right

Memory. With the frame gone the renderer returns to its shelf baseline
within seconds ([Results](#results): 232 → 134 MB PSS after a forced GC for the text
book, 368 → 162 MB for the scanned one, and `frames: [library]` only).
Every later version had to reproduce this property, and version 2 lost it.

## Version 2 — warm slot and recycle

### What it was

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

### Why it had no memory benefit

Read from `src/app/manager.rs` in that design and confirmed in the replay:

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
   `['library:active', 'reader:warm']` ([Results](#results)), and its footprint is the
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
forced GC ([Results](#results)).

## Version 3 — intent warming, idle eviction, a transient bake page

### What changed

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

### Why it works

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

### Trade‑offs, stated

- A click that lands faster than a reader boot after the *first* shelf
  intent waits for that boot (the boot is ~300 ms on the replay machine).
  Every later click is a reveal, as in version 2.
- After a read, the reader's memory is gone within the idle window plus a
  dispose beat, or at once past the heap ceiling. Continuous pointer
  movement over the shelf keeps the warm reader alive — that is intent, by
  design.
- Warming the shelf behind the reader looked free, because a close goes there.
  It is not policy: a route that is not on screen owns no frame, so the shelf
  is created on the return exactly as the reader is on the way in.
  `docs/runtime-split.md` carries the rule; the trade-off is recorded here
  because it looks free until someone has to measure it.

## Measurements

### Method

Each design's production build was taken from the `dist` artifact its own Deep
CI run uploaded (v1: run #193, commit `0159f09`; v2: #231, `0eb673d`; v3: #244,
`8ae1782`) — the browser lane uploads its `dist` on every run, pass or fail,
which is what makes a comparison across designs possible at all. That build was
served by `tests/browser/server.mjs` and driven by a replay script of the same
shape as today's `tools/measure-split-return.mjs`, in headless Chromium
(Playwright's `chromium-headless-shell`, Linux, 2 vCPU, 2 GB). Same scenario
for all three:

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

### Results

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
  version 2's switch times ([Why it was too slow](#why-it-was-too-slow)).

The same runs' latencies are under [Why it was too slow](#why-it-was-too-slow). Deep CI's own numbers for the
three eras (its lifecycle summary): v1 (Deep CI #186) `rapidTransitions`
booted a new library *and* reader session per handoff (library sessions
3→6, reader 2→5); v2 (Deep CI #217) `reusedWarmFrame` 4/4 with library
sessions constant at 2 and `coverRelay {covers: 1, ms: 1}`; v3 (Deep CI
#242) `reusedWarmFrame` 4/4, `coverBake {covers: 1, ms: 5064}`, eviction
2,019 ms.

## Comparison

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
| who bakes a shelf cover | the Shell, with `pdf-engine` linked into its artifact | the warm reader, over the wire | the shelf itself, in a JS-only bake document |

## What these designs rule out

1. **Separate the boundary from the policy.** The iframe boundary was right
   from the first design. Both failures were policies on top of it: "dispose
   before boot" and "never dispose". Memory and latency are decided by
   *when* a frame exists, and that is a small state machine in the Shell,
   not a rewrite.
2. **Measure the claim, not the proxy.** `reusedWarmFrame: true` measures
   reuse; the complaint was retention. A 1 ms cover "relay" cannot be a
   render, and a baseline that cannot be false while a reader is resident is
   not a baseline. The stages that guard the boundary therefore assert the
   user‑visible property instead — frames resident, eviction time, a cover
   that could only have come from the bake document — and the Shell refuses a
   bake from a frame that is not a shelf, so no other runtime can produce a
   cover at all.
3. **Latent bugs live where nobody is looking.** A cold-shelf bake that had
   been broken for a week, and a document-less dispose that waited out its
   timeout on every eviction, were both invisible until a test refused to be
   satisfied by a side effect of another assertion. An assertion earns its
   place by being able to fail.

