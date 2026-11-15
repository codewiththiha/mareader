# Search and keyboard (M7)

Finding text in the open document, then running the reader from the
keyboard alone. The parity entries are `search.document`,
`search.lazy-index`, `a11y.keyboard-only` and `pdf.auto-scroll` in
`parity/mareader-features.toml`.

## What mareader does

- **Matching.** Case-insensitive and non-overlapping, one page at a time.
  Each hit keeps 32 characters either side as its snippet.
- **Index.** Built on the first search, not when the file opens.
- **Bar.** Top right of the reader, at most 560 px wide (90% of a narrow
  window): an icon, the field, a count, previous and next, close, and a
  results toggle. Typing waits 180 ms. Enter goes to the next hit, Shift+Enter
  to the previous; both wrap. Escape or a press outside closes it and keeps
  the query.
- **Count.** Empty with no query, "0/0" with no hits, "n" before a step,
  "i/n" after one.
- **Results.** One row per hit: a "p. N" badge over the snippet, the
  current row tinted, "No results" when there are none.
- **Hits.** Every hit is the accent at 30% (26% on dark pages), radius 2.
  The current one is amber (#f59e0b, #fbbf24 on dark) at 58% (46%) with a
  1.5 px ring at 85%.
- **Reveal.** A hit already in view stays put, inside the insets plus a
  24 px margin (48 px sideways). Otherwise it scrolls into view. Paged
  modes turn to its page.

## What we do differently

- **Across lines.** A query matches across line breaks and words hyphenated
  at a line's end ("docu-" "ment" finds "document").
- **Folding.** Accents, curly quotes and ligatures fold, so "cafe" finds
  "café" and "fi" finds the "ﬁ" ligature, which is spelled out when its
  page is read.
- **From where you read.** The first Enter goes to the first hit on or after
  the page read, not the first in the document. It waits for that page to be
  indexed, or for the whole document.
- **While indexing.** Hits arrive as pages are read; the count ends in "…"
  until every page is in. Indices never shift, as later pages only append.
- **A cap.** At most 10,000 hits are kept; the count ends in "+" past it.

## Where it lives

| Piece | Home | Why |
|---|---|---|
| Folding, a page's searchable text, matching, snippets, stepping and the reveal rule | `crates/search` (L0) | pure, no dependencies, tested alone; the library's search folds with it too |
| Per-document indexes and their threads | `crates/engine/src/search.rs` (L5) | owns the `Document`, asks for a frame through a `Waker` |
| Hit painting | `crates/ui-egui/src/search.rs` | egui |
| The bar, count, results list, debounce and steps | `crates/ui-egui/src/find_bar.rs` | egui |
| Revealing a hit | `PdfView::seek`, `Documents::reveal_hit` | the view knows its layout and scroll |

## The index

`Search` keeps one job per open document, keyed by the `Document` it reads,
so a re-typeset book starts again. The three most recently searched are
kept; dropping one ends its thread.

The thread reads pages in order, turning each text layer into a `Page`:
folded characters with a line number per glyph and a map back to the
layer's characters. A new query answers at once over the pages read, then
keeps reading, sending each page's hits as they turn up. Only the newest of
several queued queries is answered. Progress is reported every 16 pages, so
idle frames aren't woken for each page.

Tests and the browser run inline (`workers=0` and wasm): the first query
reads every page on the spot.

## The bar

Cmd/Ctrl+F opens the bar over the focused pane; with nothing open it
searches the library instead. Its hits paint only in that pane, and only
while it is open. A step sets the current hit and asks its pane to show it,
retrying for up to 90 frames while a page's text is still being read.

The reveal leaves 72 px clear at the top for the bar, and 24 px at the
other edges. It moves along both axes, so a zoomed-in page also scrolls
sideways to the hit.

## Keyboard

mareader gets keyboard use from the browser: Tab order, a 2 px accent
ring on `:focus-visible`, Space and Enter on buttons, and focus trapped
in modals. egui moves focus and fakes clicks but draws no ring, so
`crates/ui-egui/src/focus.rs` adds what is missing.

- **The ring.** 2 px accent, inside a control's edge. Cards ring their
  padded backing; links, marks and words ring outside, and a link is also
  tinted 22%, as in mareader. It shows after Tab and hides at the next
  pointer press. Text fields keep their own focus look.
- **Hidden chrome.** Tab brings the auto-hidden title bar and the focused
  pane's page bar up, so their buttons join the order. Either stays up
  while the ring is in it, and so does the floating sidebar. mareader's
  bar stayed hidden, so Tab reached buttons no one could see.
- **The page.** Links, marks, and shaded words when a click explains them,
  become Tab stops in reading order (`crates/ui-egui/src/reach.rs`). Only what
  shows whole, clear of the bars Tab brings up, takes part, so Tab never
  scrolls the page or lands under a bar. Enter or Space
  does what a click there does. They are focus-only widgets, so clicks,
  drags and hover still reach the page beneath.
- **Not stops.** The page's text surface, drag bands, dividers, the
  library's background and popups' own frames. Each senses clicks
  without `FOCUSABLE`.
- **Menus.** Shift+F10 opens a focused card's or mark's right-click menu
  under it, with its first row focused (`widgets::context_menu`). Enter
  or Space on a row closes the menu, as a click does.
- **Library.** Enter opens a card, Shift+Enter starts picking, and Enter
  toggles while picking, as in mareader. The Library panel's shelves fold
  with Left and Right.
- **Modals.** Settings and the library's sheets are egui modals. Nothing
  behind them takes focus, so Tab stays inside and comes round.
- **Panes.** F6 and Shift+F6 move between panes. Tabbing into a pane
  focuses it, as mareader's `focusin` did.
- **Find.** F3 and Cmd/Ctrl+G step through matches, adding Shift to step
  back; with the bar shut they open it.
- **Keys and focus.** Only a text field takes typing keys. A focused
  button takes Space and the arrows, which move focus as egui does;
  zoom, page keys, Home and End still reach the reader.
- **Auto-scroll.** Shift+A, or the row under zoom in the View menu,
  drifts the focused pane down the strip at mareader's 72 px a second,
  in the scrolling views only; in a paged view a toast says so. The step
  follows the real frame time, so the pace holds at any refresh rate,
  where mareader rounded each frame's step to whole pixels. Keys and the
  wheel add to the drift, a held press (as a selection's drag) pauses
  it, and it stops at the strip's end, on Escape, on Shift+A, on a paged
  view, or when another pane takes focus. Its speed may become a setting.


| # | Slice | Check |
|---|---|---|
| 1 | Document search: the `search` crate, threaded lazy index, bar with count, steps and results, hit painting, reveal | `crates/search` unit tests; `crates/ui-egui/tests/find.rs` |
| 2 | Keyboard-only operation and focus rings | `crates/ui-egui/tests/focus.rs`; `reach.rs` unit test |
| 3 | Auto-scroll: Shift+A, the View menu row, Escape, stops at the end | `crates/ui-egui/tests/auto_scroll.rs` |
