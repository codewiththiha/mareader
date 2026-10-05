# Split memory findings: experiments that did not hold

Two experiments aimed at memory after a split and smoothness while
scrolling. Neither is in the code. They are recorded so nobody repeats them
without the evidence below.

## Rebuilding the reader after a split closes

**Idea.** When a split closes back to one pane, the reader frame keeps the
footprint the split reached: webview surfaces and caches that grew for
several panes, and the WASM heap's high-water mark. Booting a fresh reader
frame for the remaining document and removing the old frame would return
that memory at once instead of when the reader next goes back to the
library.

**What was built.**

- A workspace setting, "Free memory when a split closes" (on by default).
- 1.5 s after a split closed to one pane (and was still one pane at that
  point), the host asked the Shell to rebuild the reader, passing the
  surviving document and its read point (page and stream fraction).
- The Shell retired the warm library slot to free a boot lane, booted a
  fresh reader in the warm slot, promoted it with the document, and forced
  the old frame to be removed rather than recycled.
- A lifecycle stage proved the swap mechanically: a new generation, one
  pane and one reader frame resident.

**Result: it did not work as intended.** In real use the rebuild did not
give the expected memory or experience:

- The swap is a full reader boot: the document reopens, pages re-render
  from nothing and the read point is restored from a summary. That is
  visible to the reader, and it happened on a timer they did not ask for.
- The memory it aimed at is mostly not held by the frame. The reader WASM
  high-water mark is about 2 MB. Closing a pane already releases everything
  its session owns (the lifecycle suite asserts the pane counters and
  `thumbnailRasterBytesEst` return to zero). The remainder is the webview
  returning memory lazily (WebKit in particular), and that does not change
  because a different frame now holds the document.
- It coupled the reader's pane lifecycle to the Shell's warm slot. A rebuild
  had to be serialized against starts and recycles and cost the warm
  library its slot.

**Conclusion.** Pane close is already the right release point. To find
retained memory after a split, compare `PDFReader.stats()` (bytes the
engine holds) with process memory: if the engine holds nothing, the
difference is the runtime's lazy release, and replacing the frame does not
fix it.

## A larger render budget

**Idea.** Mount and render more of the strip around the viewport so pages
arrive already painted. The budget is `RENDER_BUDGET`
(`crates/reader-runtime/src/features/virtualizers.rs`), in screenfuls
(fraction of the viewport, maximum pages); the shipped value is
`screenfuls(0.5, 3)`.

**Result.** `screenfuls(1.0, 5)` broke the browser lifecycle baseline's
look-ahead assertion ("the look-ahead was never observed active during
scroll (blend was on)"). The blend look-ahead
(`crates/pdf-engine/src/backdrop/lookahead.rs`, `lookahead_wants`) samples
paper colour for the next pages that do not yet have a palette. With a
five-page budget those pages had already rendered and carried a palette
before the strip reached them, so the look-ahead never had work to do and
was never observed active. `screenfuls(0.75, 4)` passed but traded memory
(more mounted full-page surfaces) for a gain readers did not see.

**Conclusion.** A bigger budget is the wrong lever. It costs surfaces for
every mounted page, and at 1.0/5 it makes the look-ahead dead code. Pages arrive
painted sooner when visible pages render immediately at reading speed (see
[fling-gate.md](fling-gate.md)), which costs only the pages actually on
screen. Any future change to the budget must keep the lifecycle look-ahead
assertion passing; do not relax that assertion to fit a budget.
