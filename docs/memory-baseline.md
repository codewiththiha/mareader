# Memory baseline — Phase 0

The load from which every later phase is measured: a filled table of where the
reader's RAM lives, and how much of it survives a close today. No architecture
changes in this phase — the point is to have a number to beat.

## What is measured, and why three columns

One number cannot answer both questions, so the probe logs three pools per
line, form `[mem] <tag>: wasm <x> MB | js <y> MB | dom <n> nodes`:

| Pool | What lives there | How it is read |
|---|---|---|
| **wasm** | Leptos signal graph, reflow blocks/heights/cuts, `SearchIndex`, gloss marks, library rows/covers | `crates/app-chrome/src/memory.rs::wasm_heap_bytes` — `memory.grow` is monotonic, so this column is a ratchet: freed Rust only punches holes inside the arena. |
| **js** | pdf.js document, page render tasks, thumb LRU, canvas pool, DOM backing | `performance.memory.usedJSHeapSize` via `crates/app-chrome/src/memory.rs::js_heap_bytes` — Chromium/WebView2 only; prints `—` on WebKitGTK and WKWebView, where the name is `undefined`. |
| **dom** | Every node the webview holds | `crates/app-chrome/src/memory.rs::dom_node_count` — `getElementsByTagName("*").length`. The cheapest leak detector: a close that leaves the tree at reader size has left nodes pinned by something. |
| **rss** (outside) | Everything above plus webview + canvases | Activity Monitor / Task Manager — the column the app cannot read about itself; recorded by hand. |

## Instrumentation landed this phase

- `crates/app-chrome/src/memory.rs` widens `log_heap` from one column (`wasm`) to three
  (`wasm | js | dom`), joining `js_heap_bytes` and `dom_node_count` to the
  existing `wasm_heap_bytes`. Off wasm the probe stays inert, as it was.
- `"Performance"` added to the root `web-sys` feature list for the js probe.
- One new call site: `log_heap("boot")` at the end of
  `install_app_effects` (`src/app/effects.rs`), firing exactly once per
  launch — after every app-lifetime effect is installed, before any document
  can open. That is the **idle row**.
- Existing call sites, unchanged: `open` (document ready), `close` (after
  `destroy`/`sweep`), `search index` (first index build), `zoom commit`,
  `reload`.

## Test corpus

`tools/gen-corpus.ts` generates the two reflowable files (run
`npm run build:ts && node scripts/gen-corpus.js`; output lands in `corpus/`,
which is gitignored):

| File | Target size | Why |
|---|---|---|
| `corpus/big.txt` | 30 MB plain text | Reflow blocks/heights/cuts — wasm growth, JS stays quiet. |
| `corpus/big.md` | 15 MB with a heading every ~30 paragraphs | Same reflow cost plus parser work. |
| `big.pdf` | any real book, 1,000–3,000 pages | Not generated — no pdf writer ships here, and a synthetic one-pager would not exercise pdf.js. Record its page count beside the row. |

## Run protocol

Build config: measure in the config Phase 2 will be re-measured in — `npm run
dev` for iteration; repeat the PDF run once from a release bundle if
convenient (`[profile.release]` shrinks code, but the runtime heap shape holds).

Platform notes:

- **Windows (WebView2):** DevTools → Console, filter `[mem]`. Chrome/Edge
  expose `performance.memory`, so all three columns read. Use the DevTools
  Memory panel's GC button after each close to settle the JS heap.
- **macOS (WKWebView) and Linux (WebKitGTK):** `performance.memory` is
  `undefined`, so the `js` column prints `—`. Read RSS from Activity Monitor
  / `smem` instead — no code for this; it is the fallback column.

Per format (PDF, TXT, MD), in one session:

1. Boot → wait 5 s → record `[mem] boot` + RSS. **(idle)**
2. Open the file → scroll ~10 pages (flip a few) → zoom once → record
   `[mem] open` + RSS. **(peak)**
3. PDF only: also run `window.PDFReader.stats()` from the console —
   `{ pages, thumbs, thumbLimit, thumbTasks }`.
4. Click Library (close) → wait 10 s → optionally force GC → record
   `[mem] close` + RSS. **(after-close)**
5. Wait 30 s more → record RSS. **(settled)**

Then the cycle test — the number this whole project exists to justify:

6. Same PDF, reopen → close × 3 (four closes total). Record the wasm column
   after each close. A monotonic climb is the ratchet captured as a curve;
   that curve is the headline evidence.

## Recording template

| Scenario | wasm MB | js MB | RSS MB | DOM nodes | thumbs |
|---|---|---|---|---|---|
| Boot / library idle | | | | | |
| PDF open (peak) — N pages | | | | | |
| PDF after-close (+10 s) | | | | | |
| PDF settled (+40 s) | | | | | |
| PDF close #2 / #3 / #4 (wasm) | | | | | |
| TXT open (peak) | | | | | |
| TXT after-close | | | | | |
| MD open (peak) | | | | | |
| MD after-close | | | | | |

## What the numbers should say (interpretation guide)

Read **shape, not level**, and only against the same build — a dev wasm heap
carries bookkeeping a release one does not. Three expected shapes:

- **PDF → js heap dominates.** pdf.js documents, render tasks and the thumb
  LRU are JavaScript; the wasm column barely moves past `open`. The teardown
  already calls `engine::destroy()` + `sweep()` + `sweep_snapshots()`, so the
  js column should recover toward `boot` after close. If it does not, that is
  a teardown bug worth fixing separately from the split.
- **TXT/MD → wasm dominates.** Blocks, heights, cuts and the retained index
  land in the arena. Expect the wasm column to stay elevated after close —
  that is not a leak; `memory.grow` is one-way, and instance destruction in
  Phase 2 is exactly what breaks that latch.
- **DOM returns to library level after close.** If it does not, some listener
  or closure is pinning nodes (the parked `tauri_listen` closures are the
  prime suspect) and the `dom` column is where that shows up first.

The cycle test distinguishes the two cases the platform makes indistinguishable
from outside: closes that **plateau** are the ratchet working as dictated (each
book steps the heap up once, then flat); closes that **climb** per open/close
cycle are a leak, and the `close #2/#3/#4` row is where one shows up.

## Phase 2 acceptance rule

After reader teardown (the phase-2 `destroy()` protocol: flush read point and
gloss, `PDFReader.destroy()`, unlisten everything, dispose the Leptos root,
remove the DOM), every pool in the `after-close` row must return to within
±10 % of the `boot` row from this baseline — wasm, js (where the platform
reads it), DOM nodes, and RSS — and the cycle test's `close #2/#3/#4` column
must plateau rather than climb. A number outside that band on any pool is a
teardown bug to fix before the split is called done.
