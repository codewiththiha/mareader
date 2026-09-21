# Memory baseline — Phase 0

*Status:* the instrumentation this document describes is in the tree —
`src/memory.rs` probes the three pools and `src/app/effects.rs` logs the boot
row — and the tables below are the recording template for the measurement
runs. The numbers are filled in from a desktop session (`tauri dev` for
iteration, a bundled release build for the row Phase 2's gate re-measures)
and committed in place; nothing here changes architecture, and no number is
written down before a run produced it.

## The two questions this phase answers

1. **How much RAM does each format actually hold, and where** — wasm linear
   memory, webview JS heap, DOM?
2. **How much of it survives `close` today?** That delta is the leak baseline
   every later phase is measured against, and Phase 2's gate is set from it.

## The pools

| Pool | What lives there in Mareader | How it is read |
|---|---|---|
| **WASM linear memory** | Leptos graph, reflow blocks/heights/cuts, the search index, gloss marks, library rows/covers | the `wasm` column of a `[mem]` line (`src/memory.rs`) — never shrinks within an instance; the ratchet this project exists to break |
| **JS heap** | pdf.js document, page render tasks, thumb LRU, canvas pool, DOM backing | the `js` column — `performance.memory.usedJSHeapSize`, Chromium/WebView2 only |
| **OS process RSS** | everything, plus webview overhead | Task Manager / Activity Monitor / `smem` — the fallback column where `performance.memory` is absent |
| Secondary | DOM node count, pdf.js engine stats | the `dom` column; `PDFReader.stats()` → `{ pages, thumbs, thumbLimit, thumbTasks }` |

## The `[mem]` line

```text
[mem] close: wasm 64.0 MB | js 210.3 MB | dom 1842 nodes
```

One line per event, logged by `src/memory.rs` at the points that move a pool
— or that must visibly not move one:

| Tag | Logged from | When |
|---|---|---|
| `boot` | `src/app/effects.rs` | once per webview, at effect install — before any document can open |
| `open` | `src/services/document/open/mod.rs` | the document is ready |
| `close` | `src/services/document/close.rs` | after engine destroy + sweeps |
| `search index` | `src/effects/reader/search.rs` | the full-text index finished building |
| `zoom commit` | `src/zoom/coordinator.rs` | a zoom gesture settled |
| `reload` | `src/services/reload.rs` | Reload Window reset the instance |

A pool a platform cannot answer is a dash, never a stall: `performance.memory`
is Chromium-family only, so on WKWebView and WebKitGTK the `js` column reads
`—` and RSS stands in for it by hand. Filter the console on `[mem]`.

## Corpus

```sh
node tools/gen-corpus.mjs        # writes corpus/big.txt + corpus/big.md (git-ignored)
```

| File | Target | Source |
|---|---|---|
| `big.pdf` | 1,000–3,000 pages | a large real book from the shelf — a generated PDF would measure the generator, not the reader |
| `big.txt` | 30 MB plain prose | `tools/gen-corpus.mjs` |
| `big.md` | 15 MB, a `# Chapter` heading every ~28 KB | `tools/gen-corpus.mjs` |

## Run protocol

Measure in the config Phase 2's gate will re-measure. `tauri dev` for
iteration; repeat the PDF run once in a bundled release build — the dev wasm
heap carries bookkeeping a release one does not, so the two are different
instruments, and each table row names the build it came from.

Per platform: on **Windows/WebView2** `performance.memory` works and the
DevTools Memory panel's GC button settles the heap after a close; on
**macOS/WebKitGTK** the `js` column reads `—` — record RSS from Activity
Monitor (or `smem`) instead, and let the wasm + dom columns carry the table.

Per format (PDF, TXT, MD):

1. Boot, wait 5 s → record `[mem] boot` + RSS. **(idle)**
2. Open the file, scroll/flip ~10 pages, zoom in once → record `[mem] open`
   + RSS. **(peak)** — for PDF also capture `PDFReader.stats()` here.
3. Click Library (close), wait 10 s, optionally force GC → record
   `[mem] close` + RSS. **(after-close)**
4. Wait 30 s more → record RSS. **(settled)**
5. The cycle test — reopen → close ×3, recording the wasm figure after each
   close. A curve that climbs per cycle is the headline: that is the leak,
   distinct from the latch.

## Recording table

*Fill from the runs; one table per build config if dev and release are both
recorded.*

| Scenario | wasm MB | JS MB | RSS MB | DOM nodes | thumbs |
|---|---|---|---|---|---|
| Boot / library idle | | | | | |
| PDF open (peak) | | | | | |
| PDF after-close (+10 s) | | | | | |
| PDF settled (+40 s) | | | | | |
| PDF close #2 / #3 / #4 (wasm) | | | | | |
| TXT open (peak) | | | | | |
| TXT after-close | | | | | |
| MD open (peak) | | | | | |
| MD after-close | | | | | |

## Findings

*Filled in once the runs are recorded — three answers, in this order:*

- **The leak baseline** — Δ(after-close − idle) per pool per format. The
  wasm figure is the number Phase 2 exists to drive to zero.
- **The dominant pool per format** — expected, from the code: PDF dominates
  in the JS heap (pdf.js canvases, render tasks, thumbs); TXT/MD dominate in
  wasm (blocks, heights, cuts, the search index). If the runs disagree, the
  plan follows the runs.
- **The ratchet** — whether the after-close wasm figure climbs across the
  four closes. Flat is the platform latch (expected, not a bug); climbing is
  a leak, and its per-cycle slope goes in the table.

What to expect so the numbers make sense: the wasm column will stay elevated
after close — `memory.grow` is one-way, and that is the finding, not a fault;
the JS heap should mostly recover (the destroy/sweep path and the thumb LRU
already exist — if it does not, that is a bug worth a fix regardless of the
split); the dom count should return to its library level (if it does not, a
listener or closure is pinning nodes, and the parked-closure suspects in
`src/services/tauri_listen.rs` are the first place to look).

## Phase 2's acceptance rule

Phase 2's gate is stated against this table, not against the previous
commit: **after the reader's teardown — the engine's `destroy()` and sweeps,
every Tauri listener unlistened, the Leptos root disposed, the iframe
removed — every pool's after-close reading must land within ±10 % of this
table's idle row, measured with the same protocol and corpus.** JS heap and
DOM must hit the band outright; the wasm column hits it only once the
reader's whole instance is destroyed, because linear memory returns with the
instance or not at all — which is precisely why the reader moves into one.

## Deliverables

- [x] `src/memory.rs` probes js heap + DOM alongside the wasm heap; the
      `Performance` feature is declared in `Cargo.toml`
- [x] `[mem] boot` fires exactly once per webview, before any open
- [x] Corpus generator (`tools/gen-corpus.mjs`)
- [ ] Protocol run for PDF / TXT / MD + the four-close cycle (desktop, not CI)
- [ ] Tables + findings committed in place
