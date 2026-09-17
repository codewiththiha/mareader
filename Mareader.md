# Architecture

> Scope note: this document covers the design with the most subtle invariants —
> virtualization, motion, the format pipeline, the host protocol the AI layer finds a word
> through, and the library's addresses, shelves and rescan ledger. The library is most of it.
> For the feature tour, build setup and the crate map, see the README.

This repo now splits virtual scrolling into three layers.

## 1. `virtual-list`: pure geometry

`crates/virtual-list` owns the reusable math:

- list and grid layout contracts
- mounted-window selection from viewport + budget
- dominant-item selection
- offset and total-size queries
- anchor correction for measurement changes and uniform rescaling

It has no DOM, no framework coupling, and is the long-term public crate surface.

## 2. `virtual-list-leptos`: reactive adapter

`crates/virtual-list-leptos` wraps the geometry kernel in a Leptos-friendly adapter:

- `VirtualizerCore` is still pure and unit-testable
- `use_virtualizer` binds browser scroll containers and resize observers
- the public `Virtualizer` exposes reactive mounted items/rows, total size, dominant item, scroll offset and viewport, per-item offsets and sizes, per-item render state, the scroll-to APIs, and the measurement controls (report, suspend, resume, retention grace)

This layer is responsible for DOM measurement flow, scroll scheduling, and keeping the geometry authoritative.

It also owns the split between the MOUNT window and the RENDER band. The mount
window is what the geometry keeps warm; the render band — a tighter window
around the viewport, `VirtualizerOptions::render_band` — decides which of the
mounted items carry real content (`Active`) and which stand as placeholders at
the layout's own sizes (`Blank`). With no band (`render_screens == 0`) every
mounted item is `Active` — the pages mode, where everything the window mounts
renders fully. The stream pairs a wide budget with a band narrower than it, so
a fling slides cheap placeholders past the reader's eyes; every partly-visible
item is inside the band by construction, so nothing on screen is ever a
placeholder, and the band never changes what mounts or what the extent says.

## 3. Reader app: policy + rendering

The app uses the adapter and keeps only app-specific policy locally:

- view-mode state, plus the format-agnostic view policy (page gap, render
  budget, spread arithmetic) that lives in `crates/reader-core`'s `view` module
- toolbar inset
- page rendering, text/search overlays, and chrome
- measurement storage in `css_heights`

`css_heights` is the shared measurement store. It seeds the virtualizer, receives measured page heights, and is rescaled by the zoom actuator on every frame of a zoom. Geometry queries themselves go through the virtualizer and the layout APIs rather than through a parallel app-local model.

## Reader motion principles

1. Zoom animates the LAYOUT. Every frame of the tween rescales the strips
   through the zoom actuator, so the document genuinely resizes under the
   reader's eyes.
2. The actuator holds the document point under the viewport centre exactly
   where it is while it does so — computed gap-aware, because page heights
   scale and the gap between pages does not. Nothing is captured before a
   zoom and nothing is restored after it.
3. Zoom never scales a frozen surface with a CSS transform. A transform
   scales the page gaps along with the pages, the layout deliberately does
   not, and the whole accumulated difference lands at once when the transform
   is swapped for real geometry — which reads as the document jumping.
4. Page hosts stretch the bitmap they already hold. Nothing re-rasterises
   while the scale is moving; the crisp render is issued once, at the settled
   scale, when the transition commits. A container follow is what makes that
   rule non-trivial: a sidebar slide or a window drag relays the layout out on
   every frame and holds its COMMIT until the size has been quiet, so the burst
   costs one render pass rather than one per frame.
5. The virtualizer's window never drives the page number mid-zoom — and it
   catches the page up when the transaction lands, which is what keeps a long
   held follow from leaving the counter on a page the reader has scrolled past.
6. Recently evicted virtual items become zombies briefly (bounded set,
   grace longer than the tween), bridging the window across a zoom.
7. Zombie items never trigger a new PDF render; they keep their DOM and
   their last bitmap until their grace expires.
8. Page turns and reader surfaces (popovers, search, toasts) do not
   fade, slide or bounce in — document content appears instantly.
9. Layout chrome (the sidebar width, overlays) may use short STRUCTURAL CSS
   transitions; decorative entrance keyframes do not come back. Whether they run
   at all is the reader's call: Settings → Layout holds the master switch, and
   Settings → Animations holds one switch per motion the reader models. Both are
   projected into `state::reader::Motion` by the shell, which is what every gate
   reads — the master is applied once, there, and nothing downstream asks twice.
10. The page host is sized by its inline width/height, which ARE the page
    geometry, so it never lets the flex engine renegotiate them — a shrink
    takes the width while the height stays and the paper visibly squishes.
    There is no reflow exception for a resize, and none is needed: the layout
    follows the container on every frame of a slide or a drag, so the host is
    never left wider than its line for a frame the reader can see. A
    hand-picked zoom that overflows the window keeps overflowing — that is the
    page scrolling rather than squishing, exactly as a fit page in the
    horizontal strip does.
11. PDF renders happen only at transaction boundaries, never per frame.
12. A switched-off animation never skips the CHANGE, only the frames. Each gate
    sits where the interpolation happens, not where the change is decided:
    `animation.rs` declines to tween, `follow_watcher` drops the per-frame posts
    of a *gated* burst (a window drag) and lands the end frame once it is quiet,
    `scroll_mode` resolves to `Instant`, and the rail's class list gains
    `no-slide`. So a frozen reader is not a reader with fewer features — it is
    the same geometry, arriving at once. The one resize that is never gated is
    the follow of the sidebar's own width: there the frames ARE the correctness
    (a page may not be wider than the box the flex engine gave it), and landing
    in the frame the container was measured is what keeps a frozen slide to a
    single step instead of a step followed by a correction.
13. An animation that IS the information is not decoration, so the nets do not
    silence it. The loading mark keeps its loop under `prefers-reduced-motion`
    and under the master switch, as a fade in place: a spinner that stopped
    moving reports a hang, which is a different fact and a wrong one. The same
    rule decides how it moves — `transform` or `opacity`, and nothing else,
    because those are the two properties a compositor animates while the main
    thread is blocked, and the frames this mark must not miss are precisely the
    ones a document open takes. (It used to step three `background-position`
    gradients on one box; that is painted, so it froze during the parse it was
    on screen to cover.) Any animation that has to outlive the work it reports
    on is written the same way.

## Continuous reader flow

1. `ReaderPage` builds one `Virtualizer` for the continuous surface.
2. `ScrollShell` binds the scroll container and hands the mounted window to
   `UniversalStripHost`, which picks the format's strip — `PdfPageStrip` or the
   reflowable one. The strip renders `v.items()`, and the PDF's reports measured
   page heights back into both `css_heights` and the virtualizer; a page of type
   is A4 by definition, so there the cut publishes its sizes instead
   (`effects::reader::reflow_layout`).
3. Navigation sync uses the virtualizer for dominant-page tracking and page-to-scroll jumps.
4. Search reveal uses virtualizer offsets plus virtualizer scroll commands.
5. Zoom runs through one controller: commands resolve to a target, the tween relays the layout out through the actuator frame by frame — `css_heights`, both strips and the page hosts all follow the live display scale — and the render scale catches up once, at the end.

## Thumbnail panel flow

The thumbnail sidebar is a separate grid virtualizer:

- width-aware row windowing lives in `virtual-list`
- DOM/reactive wiring lives in `virtual-list-leptos`
- panel-specific constants stay in `src/components/shell/sidebar/panels/thumbnails`

That keeps list and grid virtualization on the same geometry stack while letting each surface keep its own rendering policy.

## The memory model: peaks, latches and the floor

The footprint the reader shows in Activity Monitor is three stacked facts, and
only two of them are the app's.

**The floor is the platform's.** WKWebView's memory number is a physical
footprint: private dirty pages, plus pages already freed but not yet reclaimed
(`MADV_FREE`, which a machine under no pressure never reclaims), plus
GPU-backed canvas surfaces. JSC's heap, bmalloc and the wasm linear memory
only grow — freed blocks go back to their own free lists, not to the OS. The
library after a read therefore never reads like the library cold, and no app
code can make it: only kernel pressure or the death of the process gives a
footprint back. Everything the app can do is keep the high-water mark low,
because the footprint latches onto the highest peak the session reached.

**The peaks are the app's.** Every transient full-page surface is a permanent
cost, and a zoom commit used to stack four of them per mounted page — the live
canvas, the snapshot mask, the unbaked raw and the bake output — with no bound
on how many pages rasterised at once, under a pixel ceiling that doubled to
32M (~128 MB per layer) on big-memory machines. What holds the peak down now:

- Full-size renders share one two-deep lane in `public/engine/renderer.ts`
  (the thumbnail lane's pattern), so a commit queues instead of stampeding.
- The pixel ceiling is 12M px everywhere (`public/engine/state.ts`): that
  still covers ~245% zoom at dpr 2 — past where anyone is inspecting rather
  than reading — and every transient a commit stacks is a quarter smaller
  than the 16M this used to be. The doubled ceiling before that bought only
  larger transients, not sharper pages.
- A bake retains its unbaked raw only while a tint scrub is plausible —
  inside 30s of the last scrub transition — and drops it at the bake
  otherwise; the scrub path re-renders on demand.
- A zoom stretch skips the snapshot mask when a render is queued for the same
  page (`src/components/formats/pdf/canvas_host.rs`): the mask exists to cover
  the frames until that render lands, which is not worth a third full-page
  layer.
- Where reading work ends — the zoom commit, the mode flip, the retention
  grace, the return to the shelf — the app sweeps the worker's caches and
  drops the masks any superseded render left behind (`sweep` /
  `sweepSnapshots` on the engine facade).
- A page a scroll fling sweeps into the mount window stays on its thumbnail
  underlay until the scroller settles — the virtualizer's scroll-end window,
  published as a signal (`crates/virtual-list-leptos/src/virtualizer.rs`) —
  and rasterises once, when the strip is quiet. A two-second fling through a
  long book costs the handful of pages it ends on, not an
  allocate/render/discard cycle per page flown past: the churn, not the
  mounted ceiling, is what drives the engine's resource cache to the mark
  the footprint latches onto.
- The full-text index builds on the first search, never at open
  (`src/effects/reader/search.rs`): extraction is the one wasm-side cost
  that scales with the BOOK — a worker round trip per page, landing in a
  heap that only grows — so an open-time build charged every book that
  ratchet whether or not anyone ever searched it. One build runs at a time;
  keystroke runs fired mid-build skip, and the builder queries the latest
  text when it lands.
- The two layers a render rebuilds REPLACE what is there instead of adding to
  it — the link layer swaps the old one out (`public/engine/links.ts`), the
  search boxes are cleared before they are repainted
  (`public/engine/highlights.ts`) — so a page rendered a hundred times carries
  one of each and not a hundred. Both run on the render path rather than the
  mount path, which is what makes them the one place a per-render leak could
  hide; a link build that outlives its host (every annotation await is a
  chance for the page to unmount or re-register elsewhere) drops its layer
  rather than parking it, with its listeners, on an element the engine has
  stopped tracking.

**The retentions were bugs.** Three teardown paths used to leave live
references behind, which is what read as hundreds of MB of private memory on
the empty shelf: the container's ResizeObserver now dies with its shell by
explicit contract instead of riding the disposal order of the install
effect's owner; a boot registration whose owner died before the microtask ran
no longer revives a dead canvas in the engine; and the full-text index is
keyed by the document's pdf.js fingerprint, so a reopen of the same book
adopts the retained index instead of re-extracting every page — each rebuild
ratcheted the wasm heap another step up — and the thumbnail lane's per-page
generation counters reset with the document they were issued for.

The heap is charted from inside, because from outside it is invisible: the
OS's number folds the wasm linear memory into the webview's total, where
canvas surfaces dominate. `src/memory.rs` logs the heap's byte length at
open, close, zoom commit, index build and the reload that resets it (`[mem]`
lines in the webview console), and the trace IS the leak-versus-latch test —
steps up once per book, flat across a session's zooms, never back down: that
is the ratchet working as the platform dictates. A climb per open/close cycle
would be a leak, and the log is where one shows up first.

What the trace is read against is a SHAPE and not a number: a fresh boot is
some floor X; reading is X plus the mounted canvases; idling on the page
stays at reading, which is the latch rather than a leak; the shelf after a
close stays there too, holding the retained index, the covers and whatever
the wasm arena grew into; and a reload is back to X. Five open/close cycles
that plateau are a latch. Five that climb are a leak, and these lines say
which before a profiler does.

The platform levers stay weighed and unpulled: a CPU-backed-canvas hint
(`willReadFrequently`) trades compositor speed for a smaller GPU cache and
wants an A/B measurement before it ships anywhere; cache-budget engine flags
are WebView2-only; and an AUTOMATIC pressure valve that recreates the webview
after very long sessions still trades reading continuity for a number the
next book latches right back. What shipped instead is the manual one: Reload
Window — a row in the reader's ⋯ menu and the shelf's — is the force-quit
minus the quit, offered rather than imposed. It pays what a quit pays first
(the resume point the progress effect is still debouncing, flushed through
`src/services/document/flush.rs`) and parks the address on the shelf before
it goes, so the boot does not mount a reader for a book that is not there.

## Formats: one host, one pipeline per family

The reader has two axes that must not multiply: how a document is *viewed* (single,
spread, two scroll modes) and what it *is* (PDF, plain text, Markdown). The UI is split
along the first axis and the crates along the second, and exactly one file joins them.

- `src/components/viewer/` is shape: the mode dispatch, the four layouts, the shells that
  hold the scroll container, and the reader's own controls (the bottom bar, the overlay
  scrollbar, the page indicator). A layout may not name a format; adding a view mode touches
  this directory and `reader-core`'s `view` module, and no format crate.
- `src/components/formats/` is substance: `pdf/`, `reflow/`, `txt/`, `md/`. Adding a format
  touches this directory, one parser crate, and one match arm in the open flow.
- `src/components/viewer/page_host.rs` is the seam, and the only file in the viewer layer
  allowed to ask which format is open. `UniversalPageHost` takes a page plus a `PageSlot`
  (single, spread left, spread right) and mounts either `PdfPageCanvas` or `ReflowPage`;
  `UniversalStripHost` does the same for the virtualized strip; `UniversalStreamHost`
  answers for continuous reading, the one case where the two pipelines disagree about the
  *surface* rather than the *content*. Both page components take the same props — page,
  scale, host id, `class` — read the page texture from context, and answer for exactly one
  page number; the PDF's extras (canvas id, gloss overlay, geometry callback) are built from
  the slot inside the host, so no layout ever passes a format-specific prop. The host also
  owns the DOM identity of a page (the `sp-`/`dp-`/`hp-`/`cont-` ids), which is why the
  floating chapter label and a selection anchor address a page of Markdown exactly as they
  address a page of pixels.

The state mirrors it: `state::reader::document` holds the document's identity (path, title,
format, page count, outline) and, beside it, a `DocumentContent` with two halves: `metrics`,
the page-size store both families write (`page1_size`, `intrinsic`, `css_heights`), and
`reflow`, the reflowable pipeline's own blocks, heights and current cut. A PDF fills the
metrics from the file and a reflowable document from its page cut, so the virtualizers, the
zoom coordinator and the progress chrome never ask who measured what.

## Text and Markdown pipeline

Plain text and Markdown share a pipeline that reuses the page machinery above the leaf
renderer. The split is deliberate: PDF pages are rasters the engine paints, text pages
are A4 hosts the reader lays out with real type — but both report the same per-page sizes into
the same virtualized strips, so view modes, zoom, navigation and search reveal stay
format-agnostic.

- `crates/reflow-core` is the shared half of that, pure (no DOM, no Leptos): the block shape
  and its splitting rules, the A4 page geometry and its spine sides, the block-granular page
  cutter, the height estimate, the typography resolution (schema lives in `reader-core`'s
  `settings::typography`) and the search over the blocks — which is a call into the scan
  `reader-core` lends the PDF's page-text index too, so an occurrence ordinal means the same
  thing in both families. `crates/txt-core` and
  `crates/md-core` sit on top of it with one parser each — normalising and paragraph-cutting
  for text, construct classification, prose subdivision, front-matter metadata and heading
  extraction for Markdown — so a format owns its syntax and nothing else. Everything is
  unit-testable on the host.
- On open, the file is parsed into blocks, oversized prose paragraphs are subdivided on line
  boundaries into continuation-flagged chunks (`subdivide`, five lines each), and an estimate cut
  is published immediately, so the reader is up the instant the bytes land. The subdivision is
  what lets the paginator pack pages tightly: no single block is taller than a few lines of type,
  so a page bottom never carries a blank band a pushed-over paragraph used to leave. The heights
  are then refined block by block as the reader's own rows render: each mounted row reports its
  measured scale-1 height into the shared store (`effects::reader::reflow_measure`), debounced so
  a fling costs one re-cut, not one per frame, and a typography or width-dial change re-runs the
  pure estimate instead of any DOM. Pagination is therefore measurement-true, and re-cuts whenever
  a typography knob moves — holding the reader on the block they were reading.
- `set_initial_heights` and `recut` are the two doors into the cut, its inverse block→page map
  and the per-page size bookkeeping, and both write the split and its map together, so a split can
  never disagree with its map.
- Zoom never re-paginates. The cut is computed at scale 1; a page host is sized `A4 × scale` and
  its type resolves through a scale-1 CSS variable times the host's own `--ts`, so the layout is
  identical at every scale and uniform scaling provably preserves the cut. During the tween the
  mounted pages reflow frame by frame at the live display scale — cheap, because the window is
  bounded — while the cut itself stays put.
- Vertical reading is the one deliberate deviation from "pages everywhere": a reflowable document
  in the vertical mode renders as the CONTINUOUS STREAM (`components::formats::reflow::stream`), which
  virtualizes the blocks themselves — not page-cut units — on the shared scroller id, with the
  window itself painted as the paper and the blocks flowing in a reading column narrowed by the
  page margin and positioned by the column-alignment setting. The page cut still backs the paged
  modes, the page bookkeeping and the resume flow; it simply is not what scrolls. The stream owns
  its zoom relayout (the page virtualizer is unbound there, and `navigation_sync`'s two page arms
  stand down), maps its dominant block back onto `viewer.page` for progress persistence, reveals
  search hits through its own virtualizer, and runs a render band wider than its visible window:
  rows inside the band carry type, rows the band has not reached are empty boxes at the
  virtualizer's own heights. What it renders is also what it measures: the mounted rows report
  their scale-1 heights into the shared store (`effects::reader::reflow_measure`), which debounces
  them into the page cut, so a column narrower than the page model still lays out truthfully.
  Single, spread and horizontal keep real A4 sheets.
- A Markdown document gets the sidebar's outline panel for real: `md_core::headings_of_blocks`
  finds the headings among the final blocks and `effects::reader::reflow_outline` projects them
  onto the live block→page table, so the chapter tree follows every re-cut instead of going
  stale. The tree lands in the same `document.outline` signal a PDF's `/Outlines` dictionary
  fills, in the same `reader_core::outline::OutlineNode` shape — the panel cannot tell the two
  apart, which is the point.
- Search hits are painted by the row that renders the block
  (`components::formats::reflow::highlight`), one layer per row, because in the continuous stream
  the row is the unit that mounts and unmounts — a page-level layer would have nothing to attach
  to, the same problem the stream's gloss layer solves by covering the whole column. The painter
  re-finds the query in the row's rendered text and covers each occurrence, so the boxes a text
  document shows are the boxes a PDF shows: one rule set styles both subtrees
  (`styles/components/search.css`), both cap what they paint at the same number, and both name the
  match the reader is on the same way — the engine pairs a page with a per-page ordinal, a
  reflowable match pairs a block with an occurrence (`reader_core::search::BlockHit`). The walk it
  measures with is the gloss projection's (`formats::reflow::spot`), so a hit and a stroke cannot
  disagree about where a character is, and the scan that finds the occurrences is
  `reader_core::search::occurrence_spans` — the one both search pipelines run — so the ordinal a
  match carries and the ordinal a box counts cannot disagree either.
- Format questions are asked once: `Format::is_reflowable` in `reader-core` is the predicate,
  `ReaderState::reflowable()` is the tracked read of it, and the UI never tests an extension
  or a document variant inline. The leaf renderer is the same deal one level down:
  `components::formats::block_render::BlockView` dispatches a block to the text or Markdown view
  from the document's format, so the page and the stream share one
  answer and none of them knows what Markdown is.
- Text never enters blend mode and never touches the paper session: a text page is recoloured by
  its own tokens, so the backdrop's colour machine is gated off for the format (the Theme tab
  hides the Paper section accordingly). Body ink has its own comfort dial —
  `ink_contrast`, a `color-mix` of the theme's ink toward its paper, exposed as the "Text ink
  intensity" slider while a text document is open. Search scans the blocks in-process (the
  document is its own index), mapping hits through the current page cut, and a reveal scrolls to
  the block the match names. The scan and the snippet window are `reader_core::search`'s, shared
  with the PDF index: one case-folding rule, one non-overlapping rule, one context window, and
  therefore one meaning for "the nth occurrence".
 Progress persistence
  saves the fractional stream position alongside the page, so a continuous read resumes where it
  stopped, not at a page top.

## How the AI layer finds a word: the host protocol and the spot

Selection, the Explain pill, the gloss card and the persisted highlights all have to
answer one question — *where in the document are these words?* — and the answer
used to be a PDF's: a page number and a rect in page space, measured against a
`.pdf-page`. Nothing else about the feature is PDF-specific, so the question was
generalised instead of the feature being forked per format.

- **The hosts declare themselves.** Every page host carries `data-reader-host`
  (the format family that painted it: `pdf` or `reflow`) and `data-host-page`
  (the 1-based page it shows), and every rendered block carries
  `data-block-index` plus the matching element id (`tx-block-<index>`, from
  `viewer::page_host::block_row_id`). The selection tracker (which lives in the
  reader bundle, not the engine's) and the app's capture find their host by
  asking for those attributes rather than for a class, so a format joins the AI
  feature by publishing two attributes and no selector anywhere grows a second
  name; the id is the lookup half of the same deal, because the gloss projection
  resolves a mark's block once per mark per scroll frame and an id read is the
  cheapest question the DOM answers. The surrounding sentence a selection
  reports is cut out of the same protocol — a PDF's text layer, or a reflowable
  document's block row, whichever the selection is inside — and a row is the
  better sentence anyway: a page of type is thousands of characters, and a word
  is disambiguated by its clause.
- **The event grew two optional fields.** `mareader:selection-detail` now
  carries `{ text, context, rect, host, spot }`. `host` says which family painted
  the selection, so the app decides the pipeline from the event rather than from
  the open document and a selection that outlives a document switch cannot be
  projected through the wrong format's maths; `spot` is a reflowable selection's
  durable identity. Both are `#[serde(default)]`, so a PDF's event — which
  carries neither — deserializes unchanged.
- **A reflowable mark remembers characters, not pixels.** Plain text and Markdown
  have no fixed page grid: the measure pass, a font-size change, a window resize
  and a column-alignment flip all re-cut the pages, and a page-space rect then
  points at whatever text moved under it. The identity that survives all of it is
  the block and the character range inside its rendered text
  (`ai_core::gloss::ReflowSpot`), persisted as a versioned envelope in
  `GlossMark.context` — `rf1:{"spot":{block,start,end},"text":"…"}`. The envelope
  keeps the stored schema the one `PageAnchor` shape, and it carries the sentence
  beside the spot because `context` is also what the model is handed when a mark
  is re-explained from storage, long after its selection is gone
  (`reflow_anchor::explain_context` reads the prose back out; a PDF's bare
  sentence passes through untouched).
- **Offsets are Unicode code points**, counted over the block's text nodes in
  document order with the stroke layer skipped (a mark's
  button carries the glossed word as its accessible name, and counting that would
  shift every offset after it). The conversion to the UTF-16 units a DOM `Range`
  speaks happens once, at `set_start`/`set_end`, so an emoji or a mathematical
  alphanumeric is one character on both sides of the wire — in the engine's
  TypeScript (`[…str].length`, `Range.toString`) and in the app's Rust
  (`.chars().count()`). For Markdown the offsets count the RENDERED text, which is
  why a heading's stroke survives its `#`s not being on screen — and why a search hit found in
  syntax the renderer drops has nothing to cover: the results list still counts it, and no box is
  painted for it.
- **The walk is shared.** `formats::reflow::spot` is the one module that answers "where is this
  character, in pixels": the text-node walk, the character→code-unit conversion, the `Range` over
  a span and that range's client rects. The gloss projection and the search-hit layer both call it,
  and the layers a walk must skip — a stroke's button, a hit's box — are one list in it rather
  than one per caller.
- **Two resolvers, one dispatch.** `anchor::anchor_resolver` answers in viewport
  space for the Explain pill and the gloss card; `anchor::stroke_resolver` answers in
  a stroke layer's own coordinates, relative to the element it measured. Neither
  knows a format: both build a `FormatAnchorBridge` per call, decided from the
  document that is open, and read the spot from the mark or the selection itself.
  A page number on a reflowable mark is a filter hint for its stroke layer, never
  an identity — `block_page` is what says where the words are now, and a re-cut
  moves a mark onto another page without touching the mark.
- **Unresolvable means hidden.** A spot whose block is virtualized away, orphaned
  by a re-parse, or written by an envelope version this build cannot read resolves
  to `None`, which the watchers already treat as "the origin left the viewport"
  and the stroke layer paints as nothing. Painting a stale capture-time box
  instead would highlight whatever words happen to be there now.
- **What makes a stroke look again.** Scale, for a PDF. For type: scale, scroll,
  container size, and `viewer::refresh::reflow_invalidation` — a fingerprint of the cut's
  block starts, the geometry it was cut with, the stream's extent and the view
  mode. It is a `u64` rather than the vectors so a re-measure that re-cut nothing
  costs one hash and wakes nobody, and the typography is deliberately not read:
  every knob that moves type moves the cut, and one that does not (the ink dial,
  the column's alignment) cannot move a mark. The search-hit layer reads the same
  fingerprint and the committed scale, but not scroll: its boxes live inside the row they cover,
  so a scroll carries them along rather than leaving them behind.
- **Three mounts.** A stroke layer is mounted by a PDF page, by a text page
  (`.tx-page`), and once for the whole reading column by the continuous stream —
  whose blocks are virtualized individually rather than paginated, so a per-page
  layer would have nothing to attach to and a per-block layer would drop every
  mark whose block scrolled out of the window. All three are the same
  `position:absolute; inset:0` box inside the element their resolver measured,
  which is why `styles/components/gloss.css` defines `.gloss-layer` once rather
  than under `.pdf-page`: nothing in it is about a raster. `mix-blend-mode:
  multiply` reads the same over ink on paper as over ink on a canvas, and the
  dark-theme `screen` swap is about the backdrop being dark, not about it being a
  bitmap.
- **Order on open.** A reflowable document's marks are loaded from storage before
  `apply_heights` publishes the block→page map, so the first page — or the first
  stream window — already paints them instead of gaining them a frame later. Dedup
  compares spots rather than pixels (`same_glossed_spot`), which is what stops a
  re-gloss after a scroll from stacking a second stroke on the same word.

## The library: addresses, shelves and the rescan ledger

The library's invariants are subtler than the reader's, because the thing it describes is a
filesystem the app does not own. Everything that decides anything is in `crates/library-core`,
which is pure — no filesystem, no wasm, no DOM — so the rules below are host tests rather than
behaviour you discover by pointing the app at a real folder.

### A book is an address

`library_core::book::Origin` has two variants and the first one is the app's whole history:

- `Origin::Linked { src }` — read in place. The book *is* that path. Nothing in the workspace
  moves, renames, copies or deletes it, and a path that stops resolving makes the book `missing`
  rather than gone: the row keeps its resume point and every shelf it is on, because a reader who
  moved a folder wants the page they were on back, not an empty shelf.
- `Origin::Stored { src, store }` — the app copied the bytes into its own data directory. `src`
  survives as provenance, which is what a relink offers to copy from again. `Origin::path` answers
  `store`, never `src`: repointing a stored book at its source would quietly turn "the app keeps
  its own copy" back into "the app reads your folder again". The store is ONE FOLDER PER BOOK,
  keyed by the id that never changes (`library_core::store`: `<Library>/items/<id>/source.<ext>`),
  so nothing on disk wears a title that can be renamed and two books both called `report.pdf`
  cannot collide — and nothing READS the store file as a name either: the display fallback for a
  copy is the stem of the source it was made from (`Book::title`'s `name_source`), because the
  address's own stem is the layout's word for the book, "source". A removal takes the folder with
  the file
  (`commands::library::delete_stored` sweeps the book's own item folder whole, and a directory of
  the old flat store only when the file was the last thing in it): a book that is gone leaving a
  directory behind is a directory no sweep would ever collect.

Which variant a book arrives as is the arrival's own fact. A folder import answers per its own
`in_place` option; a LOOSE file — picked from the dialog or dropped on the library — is always the
library's own stored copy (`import::land_stored_copy`, and the batch `run_files` lands), because a
loose file has no folder to rescan it and no ledger to answer for it, and a linked row no ledger
keeps is a row no rule can heal, tombstone or hand back. The copy takes the copy's own
measurement as its identity, which leaves the SOURCE file's fingerprint free for any folder that
reads it — the departure rule's arithmetic, on the import's side. One file is the question rather
than the copy: a loose file that sits inside a folder the library READS IN PLACE, where the book
that folder holds for it is alive and standing. A second linked row of one read-at-place file is
the duplicate the folder rule never makes, so the drop asks the covered-file question
(`conflict::Placement`, the covered pair): the library's own stored copy on this level, which then still walks
the level's name question, or the book the folder holds, gone to and lit. A file whose folder
never placed it — new since the last walk, or outside the folder's filters — is no question and
simply imports; the folder places its own linked book on the walk that finds it.

The corollary runs through every layer. A shelf holds book *ids* and nothing else, so a drag
between shelves edits an ordered list of ids and cannot touch a file the reader owns — which is
what makes filing a read-in-place book safe by construction rather than by care.
`services::library::arrange` is the only module that deletes a byte, and only one the app wrote.

One move DOES write a byte, and it is the departure: a read-at-place book leaving the ground that
made it — off its folder's shelf for another shelf, for the root, or through a collision answer
that seats it elsewhere — becomes the library's own stored copy on the way out
(`arrange::convert_to_stored`). The rule it serves is the read-at-place singular: one OS file is
one linked instance, so a book that has left its folder cannot stay one, or the folder's ledger,
its rescans and its re-imports would all keep answering for a book that is no longer theirs. The
copy takes the bytes into the store, the row takes the copy's own measurement as its identity —
which frees the ORIGINAL fingerprint, and that freedom is the point — the visible name moves into
`title`, the highlights move their key, and the folder takes a MOVED-OUT log
(`Tombstone.moved`). The ground is the rung the folder's own tree names for the file's address
(`library_core::folder::WatchedFolder::rungs_for`) and not the folder's shelf tree as a whole, so a
drag from one rung of a watched folder to another is a departure too: the book is no longer where
the ledger says it is, and a linked row wearing an address it has been dragged off is a row the next
import of that file collides with instead of coming home to. Only a re-order on the book's own rung
is no departure. A stored book is already the library's own and simply moves.

The SHELF departs the same way, and asks first — the book's rule read one level up
(`library_core::shelf::departs_on_move`). What ties a read-at-place rung to its folder is the seat
its directory stands on in the tree the folder's ledger names, so a hand taking the rung anywhere
else — another rung of the very same tree included — is a departure; only a re-order among its own
siblings, and a move back onto the seat, are not. The departure copies rather than converts one
thing at a time: the rung becomes a shelf of the reader's own — `ShelfKind::Departed`, which no
tree answers for any more, so it wears no watch dot, answers no seat and says on its card that the
library keeps the copies — every read-at-place book standing on the departing rungs goes through the
very `convert_to_stored` a book's departure rides — bytes, logs, gloss keys and all — the copy takes
the level's next free name so the folder's own name stays free, and the folder's `shelf_map` lets
the departed zone go. What rides along unconverted is what was
never the folder's to convert: the reader's own shelves inside the subtree, a book also shown on a
departing rung whose OWN rung still stands, and another folder's rung the subtree carries, which
keeps its disk knowledge under the hand's mark. It is a COST, so it is a question: the
shelf's own departure is one of the four gestures that go to a sheet BEFORE they happen
(`arrange::CopyAsk`), and the row promises the copies and the counter name before the button does.
Cancel leaves the shelf where the tree put it while the clean half of the gesture — the books and
the virtual shelves a mixed drag also carried — keeps the landing it already had.

One answer is the same for every copy the library makes, and that is the whole of
`arrange::asking`: a read-at-place book moving off its rung, a rung moving off its seat, a level
coming apart under the reader's hand, and a shelf coming off the list through the removal sheet are
four gestures with one cost, so one sheet names the action the reader is in the middle of and offers
the copy. It is the reader's answer that decides, not the path: `Copy` runs the copies and then
resumes the very gesture it interrupted (`arrange::moves::RowMove::resume`), and `WithoutCopies`
finishes the gesture the way it ran before the question existed — a return for a shelf with a way
home, and the removal sheet's own removal, which leaves the folder's ground to the next import.

The sheet has a third answer when the drop landed inside the mover's FAMILY — a rung of an
in-place tree whose root covers the ground the mover stands on (`arrange::shelf_departure::target_is_family`) —
because a read-at-place shelf lives on the seat its directory stands on, so a move inside the
tree it belongs to never has to cost a copy. *Put it back in its place* returns every mover that
has a way home (`arrange::shelf_departure::ReturnPath`): a displaced folder — one removed and imported on its own,
or born under an older build — folds into the family tree through the import's own
`reclaim_rung`, on the rung its directory names, and an off-seat rung reseats on the seat its
ledger names, which takes the hand's mark off on the way and gives the disk its place back. A
mover already on its seat has no way home to offer, because it is home.

The next import of the folder re-mints the original tree on the seats the disk names, the
moved-out logs are spent by their books coming back in their old names, and what a FOLDER import
reveals is the folder: its shelf lit on the level that holds it — nested inside the family, when
a nested rung is what left and its own subfolder is what was picked. The copy is never the
light's target: it is a book and a shelf of their own, bound to nothing. A shelf of a COPYING
folder is the library's own already and simply moves, as a stored book does.

### Identity is a fingerprint, not a path

`library_core::book::Fingerprint` is `{size, mtime_ms, head_hash}`: FNV-1a over the first 8 KiB
(`library_core::hash`), not over the file. Size and stamp are what a *move* preserves, so a file
dragged to another folder inside a watched tree still resolves to the book it already is; the head
hash separates the collision that matters, which is two different books of the same length touched
in the same millisecond. Reading 8 KiB rather than 2 GB is what makes a rescan on every window
focus affordable.

A row migrated from the previous schema carries no measurement, so it carries
`Fingerprint::placeholder` — derived from the address, so two migrated books can never share one —
and the `fp_pending` mark. `library_core::blob::LibraryBlob::awaiting_check` is the gate: a rescan
that diffed real fingerprints against placeholders would match nothing and add a second copy of
every book the folder already held.

The fingerprint is the identity, and one identity is normally one row — but not by force. Two rows
of one file are allowed, so the sanitizer dedupes by *id* rather than by fingerprint, and every
path-keyed writer treats the twins as the twins they are: a read and a path check update *all* the
rows at an address (the reading position is a fact about the file, not about the row), and a
removal sweeps the address's cover and store copy only when no remaining row reads from it
(`services::library::arrange`'s `sweep_book` — and the copy's deletion takes the book's own item
folder with it, because the folder is the book's and not the store's) — while the marks sit under
the ROW, so they go with the row the removal took and a twin keeps its own. The ledger's registry
is first-wins per fingerprint,
which is a safe answer while every row of one fingerprint reads one address — and the reason a
relink is dropped when the address it would write is one another row already reads, which is what
keeps it safe now that two folders can each hold a copy.

The list those rules run over is a list of ROWS, not of books. `book::Row` is either a `Book` or a
`Row::Link` — a name, a target and a stamp, and nothing else — and the split is what every rule in
this crate reads first. A content rule walks the book rows and steps over the links
(`book::book_rows`), because a pointer has no fingerprint to compare, no address to check and no
resume point to write; a place rule — a membership, a drag, a removal, the order a level renders
in — walks the rows, because a link is on a shelf exactly as a book is. A link is dropped by the
sanitizer when the book it points at is gone, and by a removal that takes that book, because the
one failure mode a pointer has is pointing at nothing.

Two rows can also stop being twins, and that is a mark on the row rather than a second kind of row.
`Book::independent` is what the conflict sheet's *as new* answer writes, and it opts the row out of
exactly half of the sharing above: its resume point becomes its own, and its highlights move to a
key carrying its id (`Book::gloss_key`), so no other row can name them and a removal of either row
takes nothing from the other. It does not opt out of the address's fate — whether the file resolves
is a fact about the file, so `book::apply_check` still writes every row at it, and the cover stays
the file's art. Which rows a read belongs to is one function (`book::rows_for_read`, indices so a
caller can hold the answer across the write it is about to make), and the three writers of a resume
point all read it: the open's record, the progress debounce and the close's flush. That is also why
an open carries the row it came from — `document::open_row` for a card, a list row or the menu's
Open, which opens a book and reveals the target of a link, and `document::open_path` for a drop, an
*open with* and a dialog — because an address cannot say which of two rows the reader clicked, and a
reader who asked for a book of its own is a reader who means that book. The rest of the library prefers a shared row wherever it resolves a content: the
ledger's registry indexes shared rows first and a private one only for a content nothing else
holds, and `book::add_book` resolves an import to a shared row and never to a private one.

### The ledger

`library_core::ledger::diff_folder` is the module the edge cases live in. It takes a folder's
ledger, the global fingerprint registry and the files a walk found, and answers one `ScanAction`
per file. The decision table is the test suite:

| Scan finds a fingerprint… | Book exists? | This folder placed it? | Removed from it? | Action |
|---|---|---|---|---|
| not seen before | – | – | – | `ScanAction::Add` |
| known, at the address already stored | yes | yes | – | `ScanAction::Skip` |
| known, at a different address | yes | yes | – | `ScanAction::Relink` |
| known, but the book is missing | yes | no | – | `ScanAction::Relink` |
| known, placed by another folder | yes | no | – | `ScanAction::Skip` — an explicit import's `Add` |
| seen here, but the row is gone | no | yes | – | `ScanAction::Skip` |
| anything | – | – | yes | `ScanAction::Skip` |

That last cell is the one row the two tables answer differently, and it is the row a reader meets
when they import a second folder holding a byte-identical copy of a book the first one placed. A
rescan stays quiet, because staying quiet is a rescan's whole job and the alternative is a book
reappearing on every window focus. An explicit import is a reader asking for *this* folder, and the
file in it is a file this folder has, so it is a book on this folder's shelf: handing back an empty
shelf for a folder the reader can see files in is the answer that reads as a broken import. The
address the library already holds is not a second book either way — that is the same file, and the
heal in `import::folder::run_folder` measures the row rather than adding one beside it.

Two rows of one fingerprint at two addresses are what that makes possible, and one guard keeps them
honest: the registry is first-wins per fingerprint, so it names one of the two, and a relink that
would point a book at an address another row already reads is a relink of the wrong row. The walk
drops it, and both folders' rescans stay quiet.

Row six is the rule the whole design exists for: a book the reader dragged off a folder's shelf is
still in the library, its fingerprint is still in `WatchedFolder::placed`, and the next rescan
leaves it where the reader put it. A departure that converted holds the original fingerprint in
`placed` AND writes a moved-out log beside it, so the file stays out of rescans by two independent
rows and the log can carry what `placed` cannot: a name, and — once the copy comes home — the row
that represents the file. Row seven is the tombstone in `WatchedFolder::ignored`, checked before
every other row, because a file the reader deleted from the library is still on disk and still
admitted by the folder's options. `library_core::ledger::tombstone` writes it only into the
folders that *placed* the book, so removing a hand-added book poisons no watched folder. A
moved-out log is a tombstone with `moved: true`, and the restore menu skips it: the book is not
gone, it left as a copy the library holds, and a restore would mint a linked second of it.

A relink rewrites the address and clears `missing`; it does not touch the id, the resume point or a
single shelf membership. That is what makes "the file moved" and "the book was re-filed" orthogonal.

And a relink is a question before it is a sentence the reader has to know to ask: a click on a
`missing` book raises the Find-again sheet instead of opening the reader onto an error, and its two
doors are the two ways a file is found again — pick the file itself, or name a FOLDER and let the
app walk it (the shell's own scan, every format, no size floor: a book the reader lost is not a file
to filter) looking for the book's own name, which is the shelf's display name, the stem of the dead
address, or that address's file name, case aside. A hit is re-measured before the row takes it; a
miss is one sentence; the cancel changes nothing, because a missing book is a book the library is
still holding a place for. What keeps `missing` honest between launches is the focus moment's other
half: both automatic passes — the startup's and every refocus — measure every address the library
holds BEFORE walking the watched folders, because a walk only ever sees what is still on disk, and
a book deleted or moved out since the last focus is the measure's finding, not the walk's.

A tombstone is a record and not a fingerprint, because it has a second job. Keeping a file out of every
later rescan needs one hash; offering the book back needs the name the shelf showed, the address it
lived at, the shelf it was filed on and when it went. Beside the tombstones each folder keeps what its
last scan saw, restricted to the fingerprints it placed — and `recoverables` answers both halves of
"what could this folder give back?" from those two lists plus the library's memberships, with no
filesystem involved. That is what lets the menu open on a click: a restore re-measures the single file
it is about to import, which is the only place freshness actually matters.

Two rules keep a restore from lying. It measures before it promises, and a measurement that comes back
empty leaves the tombstone exactly where it was — losing it would lose the only record the book was ever
there. And it takes the tombstone out without touching `placed`, which the import that follows writes
when the book actually lands: a fingerprint the ledger skips with no book behind it is the one state a
folder cannot recover from on its own.

The log also gives a removed book its third way back: an explicit import of the FILE — a folder run
that walks it, or the file dropped on a shelf by itself — spends the tombstone whichever folder wrote
it, because the match is the fingerprint and not the address: a file removed here, moved across the
disk and imported there is the file the log was written for. The row returns wearing the name the
shelf showed, and the run reveals it when it lands, so a book that reappeared is a book the reader
sees appear. WHERE it comes back is the log's answer and not the drop's, for a file an in-place tree
holds: the shelf the log remembers, then the folder's mapped rung for the file's subfolder, then the
folder's root shelf (`import::restore::restore_covered_file`), because a book that came back should not come
back somewhere new — and least of all on the level the file happened to be dropped on, which is a
level the folder does not own. A log of a COPYING folder, or of a file that has since left the tree,
has no folder place to come back to, and lands as the library's own copy on the level it was dropped
on, in the name the log remembered. A rescan, meanwhile, stays silent about the file, which is the log's whole point: the
removal was the reader's decision, a watchful folder does not overrule it, and only the reader's own
import does. For a MOVED-OUT log this is the whole design coming around: the copy that left has the
store's fingerprint, the original is free, and the import of the OS file brings the linked book back
beside it — two books of one content, each with one address, and the moved copy keeps its place
wherever the reader put it.

A moved-out log has one more state, and it is the return. Dragging the stored copy BACK onto a
shelf of the folder it left binds the log to the row (`Tombstone.returned_row`), and the condition
for the bind is the NAME: the log remembers the name the shelf showed, and a row wearing that exact
name is the book the reader moved back. From then on an import of the OS file does not mint a
linked neighbour beside the copy that came home — it succeeds by lighting that row up, which is
what the reader meant by importing a book they can already see. A row RENAMED since the move binds
nothing: the folder does not recognise it, and a later import simply brings the linked book back
and highlights it, which is the honest answer for a name the folder has never seen. A bind to a row
that later dies is checked against the library at import time and spent like any other log.

### When the level already holds the name

A placement — a drag, a lift out to the root, a bulk filing, a loose-file import of a file no
read-at-place tree holds — whose NAME the level it is going to already holds is a question, not a
skip. It used to be a skip, and the skip
was the bug the sheet exists for: the placement resolved the arrival to the row the library already
had, the level found that row already a member of itself, and nothing at all happened — which to
the reader was a book disappearing into the shelf it was dropped on.

The question is about a name on a level, and nothing else. `library_core::conflict` is the whole
rule and it is pure: `collide` compares the arrival's name against the BOOK rows of the target
level, case-insensitively, and answers with the row that holds it. A shelf's level is its own
member list; the root is a level too and its list is the unfiled rows the "All" view renders, so a
drop on Home beside an unfiled row of one name asks exactly as a drop on a shelf does. Four things
never ask, and each is a rule rather than a patch:

- a **link** row, on either side — it is not a book, so it never collides and never blocks, which
  is what lets a reader put a pointer on a shelf beside the book it points at;
- the row **being moved** itself, so a reorder and a duplicate filed on three shelves both stay
  quiet;
- a level that holds **no book of that name**, which includes an empty folder;
- a **counter** name, so `1_1` arriving beside `1` is the second book it already is and not a
  reason to ask again.

Names and not fingerprints, and the reason is what a shelf is. Two rows of one file were two books
the reader could not tell apart: same name, same cover, same resume point, same highlights, and a
removal of one that took the other's marks with it. A collision asked of a fingerprint could only
answer with row operations — keep both, replace, fold — and "keep both" meant two rows of one
address sharing everything an address holds. A collision asked of a name answers with a name: the
arrival becomes `1_1`, and the two rows are two books a reader can see are two books. Fingerprints
are not gone from the library, they are gone from *this* question — a watched folder's rescan
(`library_core::ledger`) and a path check (`book::apply_check`) still need one, because "is this
the file I already placed" is a question about bytes and only bytes can answer it.

The sheet (`features::library::conflict_modal`) offers three answers, and WHICH three is a fact
about the arrival rather than a setting on the sheet: a file arriving has no row of its own, and a
row being moved has two books in the question. `Arrival::is_import` is the whole of the branch, and
the answer sets are lists of one type (`conflict::Placement`), so a sheet cannot offer a file's
answer to a move or a move's to a file. The list the sheet renders and the list the apply accepts
come from the same function, which is what keeps a button from promising an answer nothing applies.

An **import** asks what to put on this level, and nothing it offers is destructive:

- **Already imported** places nothing and reveals the row that is already there
  (`services::library::reveal` — its shelf, then its card, lit). It is the answer that means *I did
  not intend to add anything*, and it is what the old silence should have been.
- **Add as new** places the arrival under the next free name (`conflict::next_name` — the name
  itself when this level holds nothing wearing it, the file manager's counter when it does, counted
  against that level's own names and promised on the row before the click) as the library's own
  stored copy — and as a second book of the address marked `Book::independent` when the address is
  one the library already reads, so its highlights and its resume point are its own rather than
  the first copy's. Because a stored copy is a book of its own bytes and never a second door on
  one linked file, no arrival has the row withheld. The compact folder-merge sheet below DOES
  still withhold its *As new* from the very file a read-at-place row reads, because that as-new
  lands as a LINKED row in the folder's own tree, and two linked rows of one file remain the
  duplicate the library does not make.
- **Make link** places a `Row::Link` instead of a copy: a row on this level with the book's name,
  no fingerprint, no page, no cover and no storage, which opens by revealing the book wherever it
  is filed. It is the answer for "I want it reachable from here" that used to be a second copy of a
  two-gigabyte file, or nothing.

A **move** asks which of two books this level keeps, and its survivor is always the row already
here — its id is what every shelf membership and every key in storage names, so a fold that moved
it would orphan both:

- **Merge** folds the moved row into it by `book::fold_books`: the further place in it wins (and on
  a page tie the deeper stream fraction, because a merge never sends a reader backwards), the page
  count is the best either row knew, names and authors fill gaps and never overwrite, the stamps
  keep the first join and the last read, a measurement beats a placeholder, and an address is dead
  only when both rows say so. The moved row's shelves become the survivor's — less the one level the
  move DEPARTED, which the arrival carries as `Arrival::from` and a filing, which departs nothing,
  carries as none: a merge that filed the survivor back on the shelf the book was lifted from would
  leave the move visibly undone, and the reader would have to drag the survivor off a second time to
  finish the first drag — and then it goes
  through `arrange::drop_row`, which is a removal without a tombstone, because the content stays in
  the library through the survivor and a tombstone for a fingerprint the library still holds is
  noise in a folder's restore menu — with one addition: when the survivor is the library's own
  stored copy of the dissolving row's very file (the provenance `src` is the check), the folders
  that placed the file take a moved-out log bound to the survivor, because here the library does
  NOT still hold the fingerprint, and an import of the file should light the copy up rather than
  mint a neighbour. Its highlights travel first, while both keys can still be read
  (`union_marks`, by `GlossMark::same_spot`, keeping their ids so the AI answers ride along): the
  sweep a removal rides takes the dissolving row's list with it, so a fold that ran afterwards
  would be a merge that deleted them.
- **Replace** sends the row that was here out of the library and seats the arrival in its SLOT —
  an overwrite stays where the thing it replaced was — and on every other shelf the displaced row
  was filed on, because a replace that quietly took a book off shelves the question never mentioned
  is a removal nobody asked for. This is the one destructive answer on either sheet, and it is the
  reason a row says what it takes before the click rather than the sheet asking twice afterwards:
  the name of the row going, and how many highlights leave with it.
- **As new** is the import's naming on a row that already exists: the moved row takes the next free
  name and lands beside the one it collided with. A read-at-place row landing beside a stored copy
  lands as a copy itself — the departure rule runs under the answer — so the two that stay are two
  stored books of one content, under two names, with nothing shared.
- **Make link** replaces *Replace* in the one shape where destruction has no side to stand on: the
  row being moved reads a file at its place and the row on the level is the library's own stored
  copy. The dragged row dissolves into a pointer at the copy — its highlights stay under the file's
  address, because the file is still the folder's and an import that brings the linked book back
  should bring its marks with it — and the folder takes a moved-out log bound to the copy, so that
  import lights the copy up. The file stays on disk, the copy stays in the store, and the level
  keeps one book and gains a way to reach it.

One question at a time, and a batch — a drag of four, an import of ten — lands its clean half at
once and queues the rest on `state::library::LibraryState::conflict_waiting`: answering pops the
next onto the screen, and Cancel drops them, which is what Cancel has always meant. There is no
"apply to all" and no queue bookkeeping beyond the list — with one exception, below, where the
questions are forty files of one folder rather than forty gestures.

A FOLDER has its own spelling of the question, asked before the walk rather than after it, and
before the question a GATE about read-at-place and its FAMILY: a shelf cut from a linked folder IS
the OS folder, so one folder is one shelf, and which tree a pick belongs to is a fact about the
ground rather than about the level the reader happens to be standing on. The gate
(`import::gate::covered_shelf`, which walks the in-place folders' `shelf_map`, and
`library_core::shelf::family_for`, which walks their roots for the rung a slot no shelf wears)
answers for READ-AT-PLACE picks alone; a stored pick walks straight to the level's name question,
its copies being the library's own second instance, unrelated to any tree:

- Ground the library already reads in place — the tree's OWN root re-picked, or any RUNG inside
  it — is not a note and not a sheet but a RECONCILIATION: the covering tree's walk runs as the
  explicit import it is, on the tree's own ledger and root, so a rung cannot mint a second
  instance of itself. New files join the tree as linked books, the logs a removal or a departure
  wrote are spent by their books coming back — wherever in the tree they stood, a book deleted
  inside a nested folder returns on a re-pick of the nested folder exactly as on a re-pick of
  the root — and the books the tree still holds but its shelves stopped holding come back as
  memberships of the rungs their directories name (`import::folder::returned_memberships`): a book
  the reader filed onto a shelf of their own and a book whose shelf they took apart are each a
  file the walk finds and the library holds, so each is a *skip* to both ledger tables and neither
  is a tombstone, which makes the merge the one half of a reconciliation no table can answer. It
  is a membership and not a move, so the shelf the reader carried a book to keeps it, and a RESCAN
  never asks — staying out of the reader's arrangement is the whole of a passive walk's job. Only
  a walk that found nothing new raises the note
  (`features::library::already_imported_modal`), saying so and lighting the shelf. What a FOLDER
  import reveals is the folder: the walk ends on the shelf the pick named — the tree's root
  shelf for its root, the rung's own shelf for a rung — lit on the level that holds it, while a
  FILE import reveals the book (`run_files`). Going inside the tree and lighting a book in it
  would be reporting the folder's contents as the folder.
- A pick whose rung the family's ledger names but no shelf wears — removed, or departed as a
  copy — imports BACK INTO THE FAMILY: the walk runs on the pick's own ledger — where the
  departed books' logs are, to be spent — and the run's last act folds the shelf it minted onto
  the rung its directory names (`RootPlan::fold`, `import::reclaim_rung`), the light landing on
  the shelf where it stands again. An import of a folder is the reader wanting it back, and back
  is inside the tree its ground belongs to, not a second shelf at the top of the library. A family
  answers only while the shelf the tree's own ROOT is kept on still stands: a reader who took the
  tree out emptied its ground, and a pick of a folder under it starts a tree of its own rather than
  minting the root shelf back to host it.
- The same fold runs at the end of any explicit walk whose tree has a MEMBER standing outside
  it (`import::gate::displaced_member`) — the rung removed and the subfolder imported on its own, or
  left standing by an older build. What "back" is, the tree's own SHAPE answers: a tree that cuts
  a rung per folder seeds the rungs the member already stands on into the run's map before the walk
  (`import::gate::seed_member_rungs`), so the books come back onto the shelf that already stands
  for the directory rather than onto a twin minted beside it, and the fold hangs that shelf on
  the rung the directory names; a tree that keeps the whole of its ground on ONE shelf seeds
  nothing, and the fold brings the member's books onto that shelf and takes the member's own
  shelves out (`import::reshape::flatten_rungs`), because the shape the reader imported with cuts
  no rungs. Either fold puts the member back, folds the folder that was reading it into the tree's
  ledger and retires it — the watch that row answered for its own root becoming the watch of the
  rung it becomes, since the row the answer was written on is the one the fold retires; for a
  one-shelf tree the answer that stands is the reader's own about its root — and the note that
  rises is the report of the shelf that went home (`NoteKind::Returned`). An import is an ask, and
  a member outside its family is an ask answered rather than a second question. The fold is into
  the row the run is walking, which no clone of the ledger can undo; a tree ANOTHER run is walking
  is the one fold refused.
- A folder's answers are the row's, and a re-import opens on them: `import::gate::ground_tracking`
  carries the row's own options beside the rung's tracking, so the sheet shows the shape and the
  filters the folder is in rather than the last import's, and the shelf-structure question is a
  question the reader can answer the same way by not touching it. Answering it the other way
  re-shapes the tree the row already reads (`import::reshape::reshape_the_tree`): the one-shelf
  answer brings the answered ground's books onto the rung that ground answers for and takes out the
  rungs it has no place for, and the shelf-per-folder answer re-files each of them onto the rung its
  own address names — both reusing the rungs that stand and minting none beside them, so the other
  shape can never grow a second tree next to the first. The answer stands for the ground the pick
  named, so the tree above it keeps the shape it stands on. A pick of a SUBFOLDER answers the
  shape for the tree it is about to become a rung of, so the re-shape runs on the row the fold took
  the pick into (`import::reshape::reshape_row`), once the fold has the pick's ground inside it: the
  books the one-shelf tree held spread come home to the rungs their own addresses name, and the run
  that found nothing new lights the shelf its books went back to rather than reporting nothing new.
  A rescan re-reads the row's answers, which is why only a re-import can move a tree this way.
- A tree whose root rung a hand took OUT — the shelf's own departure — arrives with no shelf
  and no family slot to name, and simply walks: the copy wears the level's counter name, so the
  folder's own name is free, the walk re-mints the tree on the seats the disk names, and the
  light lands on the shelf that came back.

A read-at-place tree has no *as new* and needs no *replace*: its re-import is a merge by nature,
because its books are the files themselves.

The watch itself is a TREE of decisions and has two doors, and both doors answer per rung
(`library_core::tracking`). A `TrackingTree` is a set of per-rung overrides keyed the way a folder's
`shelf_map` keys its rungs, and a rung nobody wrote inherits the nearest ancestor that has an
opinion — so the old root-level flag is exactly the tree's root rung, and "watch this subfolder but
not the tree it stands in" is a decision the data can finally hold.

The import sheet's switch is the first door. It is the reader's on every ground, and what changes is
which rung the answer lands on: ground an existing read-at-place tree already covers is a rung of
THAT tree, and the sheet opens SEEDED with the tree's own answer for the rung, so the switch shows
the state the ground is in and a click moves it — a switch that read the tree and wrote the options
was a switch that could be clicked all day without the knob moving, and the seed is what makes what
it shows the value that lands. A ground a tree has NOT taken in yet answers with the rung the run
folds the pick in as, so the switch and the walk agree about which rung the answer was about; ground
no tree answers for at all is a fresh folder's root. A run that COPIES
lands no tracking answer at all, because the sheet hides the row beside a copy: an `Off` written to
a standing tree's rung by a mode that never showed the switch would be a decision the reader was
never asked. And a re-pick of covered ground arrives as a CONTINUATION, which is the run that does
not own the tree's root: the switch answered for the rung the pick names, that answer was written
before the run (`import::gate::set_rung_tracking`), and the run re-mirrors the legacy flag from the
tree rather than writing the sheet's options over the root — a rung re-picked with the switch on is
a question about that subfolder, and the force that once answered it by turning the whole tree on
was the lock era's rule, from when the sheet had a lock to honour.

The second door is the shelf's own right-click (`services::library::set_shelf_watch`), and it
answers for the SEAT the shelf stands on: a rung's shelf turns that rung — an explicit decision at
that rung, with the tree above keeping its own — the root shelf turns the WHOLE tree, every rung's
separate decision swept with the root's, because a reader turning the folder back on is not asking
which subfolders a previous hand turned off, and a shelf a hand made inside a tree turns the
closest rung the disk named for it. The seat is one resolver's
answer (`library_core::governance::Governance::seat_of`), and the dot a card draws, the state the
menu row shows and the rung the toggle writes all read it, so the three cannot disagree. The row
names the ground it is about — the whole folder at the root, only the subfolder at a rung — because
"stop watching" from three shelves deep is a sentence about a tree the reader is not looking at.
Turning a watch on owes a walk and gets the rescan's, quietly: a hand that just asked the library to
look at a ground should not wait for a focus to see what it finds, and the rescan's table is the one
that honours a tombstone, so a watch turned on is not an ask for the books the reader took out.

The rescan reads the same tree both doors write. A focus walks every folder watched ANYWHERE — its
root or any rung a reader kept on under a root they turned off (`WatchedFolder::tracks_anything`) —
and the ledger's rescan table skips an unknown file whose rung resolves off
(`library_core::ledger::decide`), which is the quiet half of what a rung turned off means. An
EXPLICIT import is the loud half and adds what it finds whatever the tracking says: the reader asked
for that ground by name, and the watch is about the walks nobody asked for.

A STORED arrival's question is the level's name alone, and its three answers are the level's own:
*Show it* imports nothing and goes and looks — the shelf that is here lit where it stands, the
folder spelling of the book sheet's *already imported*. *Replace* sends the books the shelf that
is here holds out through the removal's sweep and seats the arriving copies on it — of the
folder's OWN read-at-place tree, the sweep is the import module's log-spending one
(`import::replace_folder_with_copies`, whose landed copies spend the logs the sweep wrote and
come back in the names the shelves showed); of any other shelf it is the removal's receipt over
the shelf's members and the merge's filing of the copies into it
(`import::replace_shelf_with_folder`). The row promises the count before the click either way.
*As new* mints the counter-named second shelf — and when the ground is one a read-at-place tree
still reads, the run that fills it is the UNBOUND one (`import::copies`): one directory is one
ledger row, so a copies run there cannot ride the bound walk, which would resolve onto the
standing tree's row, flip its mode and clear the map its seats hang on. The unbound run diffs
against the library instead of a ledger (`ledger::unbound_copies` — every file but the copy the
library already made, one book per fingerprint), and files the copies onto shelves of the
reader's own, `ShelfKind::Virtual` and bound to nothing, while the tree keeps its ledger, its
mode and its watch. The *replace* of a shelf the tree does not own — a second door wearing the
folder's name — files its copies into the swept shelf through the same walk, for the same reason.
And of ground the
library already reads in place, EVERY explicit copies run holds copies of its own — a nested
folder picked with the switch off while the outer tree stands is the shape this exists for: the
ledger's own table answers Skip for a file the library already holds, which is a rescan's answer
and not an import's, so the run's copy list (`ledger::copy_over_paths`) puts every file a linked
row reads back on the add list, and each lands as an independent book of its own bytes beside
the linked row the tree keeps, identified by its copy's own measurement — which is what leaves
the originals' fingerprints with the tree that reads them. A rescan never owes one: quiet about
ground another folder placed is a rescan's whole job. A per-file copy failure leaves that one
book reading in place: a folder with copies for some of its books is stable, and a re-import
mints the rest.

Past the gate, an import whose name the root level already holds
(`library_core::conflict::collide_shelf`, which counts the level's SHELF names and nothing else)
raises the folder sheet, because two doors of one name on one level are two doors a reader cannot
tell apart — which is what two books of one name are. Its answers are about the whole run rather
than one placement, and WHICH set of them is on offer is the arrival's mode: a stored arrival
gets the level's three above, and a READ-AT-PLACE arrival of a DIFFERENT folder's name gets two,
its *as new* withheld as the second instance the gate exists to prevent. *Make link* imports
nothing and leaves a pointer row instead — a `Row::Link` whose target is a SHELF id, which the
ids' first letters keep disjoint from a book's, and whose tap reveals the shelf it names
(`services::library::reveal_shelf`: its level, then its card, lit). *Merge into it* maps the
folder's root rung onto the shelf that is here — the
folder's `shelf_map` carries the promise, so every later rescan keeps it — and every file whose
NAME a STANDING rung holds goes to the compact per-file sheet: the root shelf the answer named,
and any subfolder shelf a previous run mapped, because a merge reuses the rungs it has rather than
minting fresh ones, and screening only the root would leave a folder of three subfolders asked
about one of them. The compact sheet's answers are Merge (the row stays and takes the file's
measurement), Replace, or As new, one at a time or, behind the apply-to-all switch, one answer for
the whole queue — and As new is withheld, here too, from a file whose very address a read-at-place
row reads, with the rule kept on the write side as well (`apply_folder_merge` folds such an answer
into Merge) because apply-to-all outruns the rows a single sheet showed.

A file nothing collides with simply goes in: new books in a merged folder are the default, not a
case — and "a file nothing collides with" is answered over EVERY file the walk found, not only the
ones the ledger marked new: a planned tree (a merge's, or an *as new* one) owes a membership of the
row the library holds for each file it already knows, because one content is one identity and one
identity is one row, and a second shelf of one folder is a second arrangement rather than a second
copy. A folder colliding with its OWN previous shelf asks too — a reader who picked a folder and
clicked Import asked for an answer, and a run that ends on "Imported 0 books" with no sheet in
between is the silent nothing the book collision used to be — and the sheet words it as the
continuation it is: a stored re-pick of one's own tree is the *show*, *replace* and *as new*
question about how the folder is held from here on. A read-at-place pick of one's own family
never reaches the sheet at all: the gate above answers it with the note, the reconciliation, or
the fold. A nesting still asks nothing at all, because a nesting writes a parent and not a
membership.

Nesting asks nothing at all, and that is the rule rather than an oversight: filing a folder inside
another writes no membership, so nothing arrives on the parent's level for a name to collide with.
A watched folder's own rescan never asks either — staying quiet is the ledger's job — and a folder
walk keeps the fingerprint dedupe it always had, because four hundred files are not four hundred
questions.

### Where the work happens

The split is IO on one side and decisions on the other, and the wire between them is declared once:

- `src-tauri/src/commands/library.rs` walks, measures, copies and deletes. It filters during the
  walk with `library_core::folder::FolderOpts::admits_file` so a folder of forty thousand
  screenshots never crosses the wire, refuses symlinks and hidden directories, caps the depth and
  the result count, and gates every path through the crate's existing document gate. Its delete
  command only removes a path that canonicalises inside the app's own store directory, and its
  relocation only moves between two such paths.
- `library_core::wire` holds the four types that cross (`ImportProgress`, `PathCheck`,
  `BookFileRequest`, `StoreResult`). Both sides depend on `library-core`, so there is one declaration
  and no contract test needed to prove the halves agree — which is an improvement on the AI chunk
  envelope, written twice and held together by a test.
- `services::library::import` runs the ledger and writes the answer. One walk per root, claimed
  synchronously and released by the run's own drop (`import::claim`), and **an ask outranks a
  rescan**: an import that finds its root being walked by a focus rescan waits out that walk rather
  than being refused by it, because the rescan honours the tombstones a removal wrote — which is its
  whole job — and the run that lifts them is the reader's. Waiting rather than cancelling, since the
  rescan's write is the ledger the import reads. A focus the app's own picker caused does not start
  a walk at all (`services::library::picker_focus`): a native dialog handing the window back is a
  focus event like any other, and the import it hands back with is a better walk of the same ground.
  Nothing is committed until the
  whole answer is known: the scan, the diff and the copies all run against local copies of the
  three lists, and the state is set once. A shelf that filled in file by file would repaint per
  file, and a failure half way through would leave the library holding books whose bytes never
  arrived. A copy failure is per-file, so one locked file costs the reader that file and not the
  batch.
- `effects::app::library` installs the three app-lifetime pieces: the sink that folds progress beats
  into `state::library`'s task list (a run outlives the page that started it, so a listener mounted
  on the page would stop counting at the route flip), the startup measurement pass, and the rescan
  on `tauri://focus` behind a cooldown.

### Order, in one place

`features::library::content` derives the visible list once — shelf narrows, sort orders, query
filters last — and provides it as `ShelfOrder`. A card cannot work out its own index from the DOM
without counting siblings, which would be a second definition of the order; a drop that lands
"before this card" therefore asks the same function the grid rendered from, and the two layouts
cannot disagree about where "here" is because neither of them owns the answer.

A drop is only given a position while the order is the manual one
(`library_core::view::LibraryView::drag_reorders`), because a shelf sorted by title re-sorts on the
next render and would undo the drop before the reader saw it land. A sorted shelf still accepts the
drop; it appends rather than promising a slot it cannot keep.

### One search, one rule

The bar's filter and the panel under it are one rule in `library_core::query`, spelled once: every
whitespace-separated term has to match one of a book's three fields — title, author, address —
either as a substring, the way search always worked, or as an in-order subsequence that scores
well enough, the fuzzy half. The score rewards the shapes readers type (consecutive runs, starts
of words, the start of a field) and charges the gaps between hits; a subsequence that scores under
two points a term character is too scattered to be a match, so fuzzy forgives a dropped vowel
without forgiving everything. The shelf filters on the rule, the suggestion panel ranks with the
same rule — title outweighing author outweighing address — and the matched character spans travel
with the score, which `features::library::search_suggest` lights so the fuzzy half shows its work.

There is no index, and the panel is no standing derivation. A few thousand one-pass string
comparisons are well inside a frame, and an index would be a second thing to keep in step with the
list it describes; the suggestions are computed at the keystroke, untracked, and stored, so
nothing re-ranks while the reader is not typing and a closed panel costs nothing at all.

### A shelf is a level, not a row

`Shelf::parent` makes the shelves a forest: the root level is the shelves with no parent, and a level
inside one is `library_core::shelf::children_of` on its id. `features::library::content` derives both
halves of the page once — `ShelfOrder` for the books and `FolderOrder` for the shelves at this level —
and provides them to both layouts, so the grid, the list, the selection bar's "All" and the drop that
lands among them are all counting the same level.

The grid renders a folder as a cell of the same grid the books are cells of, which is the whole of
what makes nesting drawable: the shelf tile this replaced spanned the grid to read as a row *of*
books, and a row cannot be inside a row. The dense list draws the same level as a tree: a shelf is a
row that unfolds in place — the shelves filed in it and its own books indenting under it, as deep as
the forest goes — while an Open on the row drills the breadcrumb route, because unfolding is a way
of looking and must not move the reader. The tree is `ShelfTree`, a plain prop bag over the same
rows, which is the component the reader sidebar's shelf tab will mount at its own density.

One relationship in this model can be wrong in a way no single row shows — a shelf filed inside
itself, or inside one of its own children, is a folder that renders on no level and can never be
opened again. So the graph is guarded twice, and the two guards are not redundant:
`library_core::shelf::can_nest` refuses the drop before it is written, and
`library_core::shelf::sanitize` cuts any cycle a restored or hand-edited blob carries, because a rule
enforced only on the way in is a rule one backup can break. Sanitising the shelves LAST in
`library_core::blob::sanitize` is what makes the second guard enough: a folder shelf whose watched
folder is gone is dropped there, and a shelf nested inside it would otherwise be left pointing at a
parent that no longer exists.

Removing a shelf lifts the shelves inside it to the level it was on (`shelf::lift_children`), for the
same reason its books stay in the library: a reader who took one folder apart did not ask to lose the
folders filed in it — and the level comes up to the nearest rung the folder's tree still stands on
rather than to the library's top level, because a rung below a hole is a level the folder's next scan
cannot see. What a level DOES take with it is the books it holds read in place: they leave the ground
that made them, so the removal sheet asks the same copy question a menu's *Take shelf apart* asks
(`arrange::asking`), and the reader who wants the folder to mint the level back instead picks the
answer that copies nothing.

That is the default and not the only answer, because "take this shelf apart" and "get rid of this
shelf and everything in it" are both things a reader means, and only the first of them was reachable.
`features::library::remove_modal` carries the second as a switch on the same receipt — the cascade
belongs in the sheet rather than in a second sheet, because it is a question about the SAME removal,
and what a shelf holds is part of what removing it costs.

Everything on that sheet is measured over the set the removal will actually take, not over what was
clicked: with the cascade on, the books row, the highlight and cover counts, the store copies and the
button's own wording all describe the asked books plus everything inside the asked shelves. A receipt
that itemised the selection and then removed the selection plus a folder's contents would be a receipt
for a different removal than the one it confirmed. A switch is offered only where there is
something for one to decide — an empty leaf shelf gets no cascade switch, and a removal of books
nobody wrote in gets no data switch — because a control that appears with nothing to decide is a
control the reader has to read and then ignore. Two things follow from the cascade being a
change of SET rather than of wording: the shelf rows switch from saying what survives to saying what
goes, since the same words would mean the opposite, and the deletes run deepest-first so
`lift_children` never moves a shelf to the level it was on moments before deleting it. The tree
arithmetic that decides which shelves those are (`subtree`, `deepest_first`) is pure over the shelf
list and host-tested, including the cycle a blob caught between two writes can still carry.

One row of that receipt is a question rather than a cost, and it is the only one. The app's own
copy goes with the book: a file nothing will read again is not worth a switch, and removing a book
has always meant the store losing it. What the READER wrote is theirs — the marks, the place they
stopped at and any name they gave the book — so it is stowed instead, in `mareader.kept.v1`
(`storage::kept`), keyed by the FILE rather than by the row that went. The switch drops it instead,
and the sheet says what keeping it means: the next import to land that file as a row of its own
puts it back (`import::kept`), matched by the file's name and by how much else agrees with the
record — its address, its bytes. Only a landing that MINTS a row reclaims: a row the library still
holds has reading data of its own, and an arrival that resolves to it is a second copy rather than
a book coming back. A watched folder's tombstone is the other way a book comes home, and the two
are independent on purpose — the log answers for a folder, and the kept store answers for a file
the reader brought back themselves.

Nesting is not `ShelfKind::Folder`'s `rel`. `rel` is a subfolder's address inside a watched
directory's tree — a rescan key, written by the filesystem's shape. For a FOLDER shelf, `parent`
starts as its projection: the tree on disk is the tree on the shelf, and every scan re-hangs the
folder's shelves on the rung their `rel` names. Which shelves a hand may simply take is decided by
how their folder is read (`library_core::shelf::departs_on_move`): a shelf of a COPYING folder is
the library's own already, so `library_core::shelf::reparent` accepts the move and marks the row
`Shelf::manual_parent` when the new parent is not the seat the folder's shelves name, and clears
the mark when a hand puts the shelf back on it — the re-hang passes a marked shelf by, so the move
is a promise the next scan KEEPS instead of one it breaks. A shelf of a READ-AT-PLACE folder is the
OS directory itself and never arrives at that reparent off its seat: the hand-move is the
departure, which asks and then lands a copy (`services::library::arrange`). The subtree still
travels with the shelf the hand named — a copy carries its folders and its books the way a moved
directory carries its tree — and a read-in-place book's address travels on the book, so tracking,
relink and the resume points never knew a move happened. For a VIRTUAL shelf `parent` is the
reader's from the start, and no scan ever writes it.

### One gesture, decided once

A card answers to three pointers that all arrive as the same `pointerdown`: a tap opens, a hold
starts a multi-select, a movement files the card somewhere else. Handled as three listeners they
race — the hold completes and the click it generates opens the book it was meant to select, or the
press drifts two pixels and cancels a gesture the reader was still making.

`components::primitives::interactions::draggable_item` decides instead. The mode is chosen once per
press and locked until the pointer is released: the hold timer firing wins, or the pointer
travelling past a 6px threshold wins, or neither happening before release means it was a tap.
Travelling past the threshold while nothing is draggable is its own fourth answer rather than a tap,
because on a touch surface that movement is a scroll and opening the card the reader scrolled past is
exactly the surprise the wrapper exists to prevent — which is why a finger is never a drag at all,
whatever the caller allows. `DraggableItemOptions` is the policy — which of the two a card allows,
and what each means — and `DraggableItemHandle` is the four pointer handlers, the one flag a card
paints itself from, and the one-shot probes that swallow the `click` and the synthetic `contextmenu`
a completed hold generates.

The wrapper decides *which* gesture a press was and stops there. What a movement then does belongs to
the caller, and on the shelf that is a session rather than a browser drag. The caller side is itself
one wiring rather than one per surface: `features::library::gestures` binds the wrapper, the session
handoff, the selection's enter/toggle, the keyboard's two halves and the right-click's ask once, and
a book card, a book row and a folder card hand it the three answers that are theirs — what the item
is called, whether a movement may lift it, and what "open" means for it.

### What a drop means

Nothing in the library rides the browser's own drag-and-drop, and the reason is not taste. Once the
engine takes a drag over, `pointerup` never reaches the element the press began on, so the card's own
"being held" flag had no release to clear it and the card stayed faded until a click somewhere else
dismissed the selection it had turned on. Two more things a browser drag cannot do at any price
decided it as well: it will not report how *long* a drag has hovered a target, which is the whole of
the fold gesture, and its image is one bitmap of the one element the press started on, so a set of
four books drags as whichever was pressed.

`features::library::dnd` is the replacement, in four pieces that each answer one question:

- `features::library::dnd::controller` owns the session: the `DragPayload` a press picked up, the
  pointer's coordinates, the target under it, and the single `end` that a release, a cancellation and
  an Escape all arrive at. A drag that cannot be stuck is a drag whose every exit goes through one
  function. Its listeners live on `window` and only while a session is live, which is what lets a
  card unmount mid-drag — a focus rescan filing it elsewhere — without taking the drag's release with
  it.
- `features::library::dnd::target` is the registry. Targets register themselves on mount and leave on
  unmount, and a move hit-tests the registry against coordinates instead of counting `dragenter` and
  `dragleave` boundaries. Boxes are read at hit-test time, so a shelf that scrolled between two moves
  is hit where it is rather than where it was.
- `features::library::dnd::effect` is the decision table: what is held, what is under the pointer,
  which part of the target's box the pointer is on, which shelf renders the target's row, and how
  long it has been there go in, and one `DropEffect` comes out. It has no DOM and no signals in it,
  which is why it is the piece with the unit tests and why a refusal is a value rather than an
  absence — a folder that would close a loop says no before the pointer arrives, so the ring that
  would have promised the drop is never drawn.
- `features::library::dnd::commit` is the only place an effect touches state, and it touches it
  through the services a menu row uses, so a dragged book persists and keeps its cover exactly as a
  filed one does. The services screen what arrives: a drop onto a shelf that already holds the very
  content goes to the conflict sheet instead of being skipped.

What a press picks up is the set's business rather than the gesture's: `features::library::selection`'s
`payload_for` answers "the whole selection when this card is in it, this card alone when it is not",
which is the rule that makes a bulk move one gesture. The visible half of that is a fade on every held
card and a ghost of their covers — four tiles at most, fanned, with a count badge for the rest — drawn
by `features::library::dnd::layer` above the content and below the sheets.

A list row is the same target with one fact and one question more. The fact is the shelf whose member
list renders the row, which the entry carries and the insertion names, so a drop inside an expanded
tree indexes that branch's own list instead of the flat order the page is showing — the seam under a
nested row used to land in the open level, silently. The lift carries the same fact back out as the
payload's source, so a reorder inside a branch takes its books off the branch and never off the
page's own shelf, which a book on both is a member of as well. The question is which PART of the row the pointer
is on: the bottom half of a book row lands the hold after its anchor, a shelf row's middle takes the
hold inside it, and its outer quarters reorder held folders beside their anchor in the level that
holds the anchor — the graph asked is the parent's own `can_nest`, and a folder asked to sibling
itself is refused. A hold with no books in it is refused by a book row at EVERY band: a row is a
seam between books and a shelf is not a book, so a folder over a book is neither a landing nor a
fold — it lands in a folder's mouth, beside its own kind on a shelf row's edge, on a crumb or on
the level's space. The band is computed once, in the session, from the row's own rectangle, so the
seam painted and the index committed cannot disagree; outside the list layout every band is the
middle one, and the grid keeps its whole-card answers without the table carrying a branch about
layouts. The tree adds the courtesy every file manager's tree gives a drag: a hold resting on a
collapsed shelf row opens it, so the way deeper is the way in. A shelf row is a LIFT as well as a
landing: a hold enters the selection with the shelf in it and a movement picks it up, by the same
wiring the folder card wears — a folder is draggable at both densities, watched or not (what the
drag MEANS is the services' answer: a membership edit for a shelf the reader or the store owns,
and the departure's ask for a rung a read-at-place folder named), and a set of books and folders is
one gesture in either.

Two dwells hang off the same target change, and they are NOT the same question at two depths — the
difference is the whole of the design. The sink belongs to the title bar alone: at 420ms over a crumb,
the ghost stops following the pointer and sits at a third of its size on that crumb's centre. A crumb
is the one target on the page smaller than the ghost hovering it, so it is the one place where a
full-size ghost covers the thing being aimed at — the name of the level the held items are about to go
to. The shrink is also the only thing that CAN keep it readable, and that is not a stylistic
preference: the bar and the fold menu are both in a lower lane than the drag overlay, so no z-step
puts the ghost behind a crumb without putting it behind the whole shelf. A third-size plate needs no
lane; it simply stops covering the label.

Nothing on the shelf itself sinks. A folder card does not need to: it already wears the loudest marker
in the shelf's vocabulary — the accent ring, the halo and the plate lifting — so a shrink on top of
that is a second, slower answer to a question the ring answered on the frame the pointer arrived, and
it takes the covers away from a reader at the moment they are checking what they are holding. A book
is not a container at all: it is a position, which the insertion line beside it already draws, or a
fold partner, which the plate draws instead of the ghost. And the level's empty space has a box the
size of the scroll container, so its centre is the middle of the screen — sinking there is the ghost
leaving the reader's hand for a place they are not pointing at.

At 650ms a hot book arms a fold and the ghost becomes a folder card's own plate filling in — one lit
cell per item the new shelf would hold and a `+` in the next — which outranks the sink, because a
plate shrunk to a third of itself inside the card it is offering to replace is a plate nobody can
read. The dwell is longer than the hold that starts a selection on purpose: a reader crossing a shelf
rests over cards, and a fold that armed at the hold's tuning would offer a new shelf on every drag
that happened to slow down.

Which book can be a partner is a separate question from whether the reader meant one, and the two are
answered by different things. Membership of the payload decides WHICH: a book the pointer is already
carrying is a position and never a partner, so dragging a book onto itself, or a selection onto one of
its own members, reorders instead of counting that book twice — the bug a self-counting target had.
The rest decides WHETHER, and it is not a refinement: without the dwell a reorder would be unreachable
at all, because every card a drag crossed would be offering a new shelf instead of a place to land.
With both, one book rested on another is a shelf of the two, which is the smallest shelf a drag can
make and the whole of what folding a pair means. The fold is BOOK over book in both halves: folders
and crumbs never brew a shelf whatever the rest, because a fold over a folder would be a nest and a
create at once — two answers to one release — and a hold with no books in it is refused by a book
row before the dwell is ever asked, so the plate and the ring appear only for the gesture that
exists: books brewing a shelf over a book. A mixed hold folds with everything it carries, the
folders included, because the new shelf is a shelf like any other and takes both kinds.

A sunk drag is a parked drag, and it is charged for nothing. The sink caches the crumb's box with the
spot, and while the pointer stays inside that box a `pointermove` does one comparison and returns:
no `getBoundingClientRect` per registered target, no signal write, no re-render. The cache is what
makes parking free and it is stale under a scroll, which is the same promise the captured spot
already makes — a sunk ghost says the pointer has stopped moving.

The transition is the sunk state and nothing else, on one signal rather than two. A `left`/`top`
transition left on for the follow would put every frame of it 200ms behind the hand AND cost a layout
per frame, which is the one thing a drag must not spend; a second "is animating" flag kept alive for a
grace beat after the sink lifts is exactly that, held for one beat too long. So the class comes off on
the same frame the sink lifts and the follow resumes 1:1, while the grow-back stays soft on transform
and opacity alone, which the compositor runs without touching layout. Both collapse under the app's
two motion nets, so a reader who asked for no motion gets a ghost that lands in one step rather than
gliding there.

### A bar of its own

The library's bar is the reader's shape — leading cluster, centred slot, trailing cluster, the
built-in pin — filled with the shelf's jobs, and the ways the two routes differ are one vocabulary
rather than scattered facts: `ChromeSurface` (`components::shell::controller`) names the route, and
every per-route rule reads that name. The pin is remembered per surface, in one settings field each,
because unhitching the reader's bar out of a document's way says nothing about the shelf's — and the
shelf's defaults to pinned, since its bar is how the reader moves. The appearance menu drops its
page-texture section off the reader surface (and on the reader too while a reflowable document
paints its own paper — the same two facts the settings modal's Paper section gates itself on), and
the settings gear does not mount: settings are the reader's, and a button that opens a modal with
nothing to say about the shelf is a button the reader has to read and then ignore.

### A bar that can go deep

A chain of levels has no end and a title bar does, so `features::library::breadcrumb` elides the
oldest crumbs behind an ellipsis — never just one, because a single elided level costs a hover to
reach and costs the bar the width showing it would have. Whether the bar folds at all is a depth
question first: a chain shallower than four nested folders never folds, however cramped the bar is —
its crumbs truncate against each other instead, because the smallest legal fold hides two levels and
below four that leaves one lonely crumb beside the ellipsis. Past that gate, how many fold is a width
question before it is a count: every crumb (plus the ellipsis itself) is measured in a hidden probe
against the cluster's own live box, which is observed rather than polled — so a chain that grew folds
on the same frame it gets cramped, and so does a cluster the window squeezed, with no resize handler
anywhere in the fold. The count rule — keep three — is the fallback for the frames before the first measurement.
The cluster itself is squeezable (`min-w-0`, not `shrink-0`): a bar whose left refuses to shrink
answers a long chain by overflowing over the search field, which is the overlap the fold exists to
prevent, and the shell already observes the cluster elements, so the centered slot follows every
squeeze without being told. The panel is the `MenuPopover` every
other anchored menu in the app uses, which matters more than it looks: that primitive is the one place
that knows the glass toolbar row's `backdrop-filter` makes it a containing block for `position: fixed`,
and a hand-rolled panel anchored in the bar would be positioned against the row and not the viewport.

The ellipsis is its own element and not an arrow on a crumb, and that is the whole of the fix for a
confusion worth naming. An arrow on the THIRD level whose panel lists the first and second reads as
"deeper than three", because a disclosure hangs below the thing it discloses — and the elided levels
are shallower. An affordance standing for them must not itself be a level, so it claims to be nothing
but a gap. Inside, the levels are drawn in the bar's own grammar rather than as a list of rows: name,
chevron, name. The chevron trails its crumb and shares its flex item, so a line ends on `4 >` and
the next begins on `5`; a leading chevron would put a stray `>` at the head of every line but the
first. The panel's width is measured rather than picked, and its chain is packed rather than
wrapped: the chain is drawn once more inside the panel as an invisible, unwrapped ruler, and on
open — and on every resize while the panel is open — the crumb boxes are laid greedily against a
budget of the whole window minus breathing room, because a chain the screen can hold on one line is
held on one line. The panel takes the widest packed row's width, and each row paints its own
surface, so a short second row is a short rectangle rather than a wide empty one dragging along
behind it. A crumb wider than the whole budget gets a row to itself: a level is never dropped, and
the panel never hangs off the screen.

It opens on hover and closes one beat after the pointer leaves, because a click is already taken by
the crumb it lands on. The beat is owned by an effect on "is the pointer over it" rather than by a
parked timer, so arming and cancelling the close are the same write and there is exactly one timer.
ArrowDown opens it too, which is not a nicety: the elided levels are on no other surface, so without a
keyboard path a chain deeper than the bar keeps would be navigable by mouse only.

Every crumb, elided ones included, is a drop target, which is the only way to reach a deep level with
a hand full of books — and the reason the panel has to be openable DURING a drag. A drag cannot raise
a `mouseenter`: the card the press began on holds the pointer capture, and a captured pointer reports
its boundary events to the capture target alone. So while a drag is live the ellipsis opens from the
session's hot target instead, which is the same geometry the drop is decided by and the one thing under
a capture that still tells the truth. The ellipsis is a target and not a drop: it stands for several
levels and names none of them, so resting on it opens the panel and releasing on it does nothing —
filing onto a level whose name the reader cannot see is a filing they cannot check.

### What a right-click is

A card used to answer a right-click with the removal receipt and nothing else, which is one row of a
menu wearing the whole gesture. `features::library::context_menu` is the shelf's answer now: one host
and one signal, asked by every surface that can be right-clicked — a book card, a list row, a folder,
the empty level — so four surfaces do not own four placements, four dismissals and four sets of rows
to keep in step. The payload says which menu, and it carries the facts rather than an id, because a
row that asked the library what it was pointing at would be reading a list a rescan can change between
the click and the row.

Two things it deliberately does not do. It does not start a drag: a menu row is clicked with a pointer
that has already been released, so a session begun from one would have no pointer to follow and no
release to end it, and the next click anywhere would be the drop. And it does not fork a second shelf
picker — "file these somewhere" is the selection bar's popover, which is on screen whenever a
selection is. The actions both surfaces offer are one function each in
`features::library::selection` for that reason: a bar and a menu that each minted a shelf would
eventually differ about whether to drill into it.

One row hands the reader out of the library and into the OS: *Reveal in folder*
(`services::library::reveal::reveal_in_folder`) opens the file manager on the item, selected
inside its folder. WHICH path a row reveals is the row's own fact, read at the build rather than
at the click — the store's copy for a book the library owns, because that copy is the file this
row reads; the file where it stands for a book read at its place; the target's own answer for a
link; the directory its tree cut it from for a watched folder's shelf (`path_of_row`,
`path_of_shelf`, over the one `Book::path` and `dir_of_rung` that already own those answers). A
shelf the reader owns has no ground and gets no row, a book whose address died gets a disabled
one, and the shell's verb is per platform — the item selected on macOS and Windows, the
containing folder on Linux, which has no standard select (`commands::library::reveal_in_folder`).

One right-click renames the thing under the pointer, whichever kind of thing it was: *Rename…*
opens the shelf's rename sheet (`features::library::rename_modal`), one field seeded with the name
the row or shelf shows, Enter commits and Escape leaves the name alone. The name is a DISPLAY name
and nothing else — a book's `title`, a link's own name, a shelf's `name` — and the write is the
primitives the rest of the app already had (`LibraryState::rename_row`,
`services::library::rename_shelf`): the row keeps its id, its address, its resume point and every
shelf it is filed on, and a file on disk keeps the name it has. A book's renamed title is LOCKED
(`library_core::book::Book::title_locked`), and the lock is a provenance line the load-time sweep
reads: the rule that drops a title shaped like a filename hunts download debris a document
supplied, and a name a person typed is not debris whatever it looks like. No collision question is
asked, and that is the level rule rather than an omission — the counter a collision mints answers
an ARRIVAL, and a rename is not an arrival; two rows of one name on one level are two books the
reader named that way.

One right-click makes a second instance of what was pointed at, whichever kind of thing it was:
*Duplicate* (`services::library::duplicate`). A book — whatever its origin — gets a second copy in
the library's own store through the store's own batch, named after the new row's id and stamped
with its own modification time, so the copy measures as a second book and not as the first one
twice and is known by the copy's own measurement; the source the original recorded stays the
provenance both wear. Nothing is written into the folder the original reads and the duplicate is
never linked, so the departure rule never sees it and there is no sibling on the reader's disk for
the library to orphan. The highlights ride along (`storage::copy_gloss`): the copy lands wearing
its own list under its own row id, every mark re-minted, so the two books' strokes never toggle,
evict or overwrite each other. A link at a book duplicates the same way — the copy is of what it
opens, named after the link and filed beside the link — while a link at a shelf stays a link,
because a level holds no bytes to store. Either way
the duplicate is a row
of its own: the counter name the level showing the original gives it
(`library_core::conflict::next_name` — the collision sheet's convention), filed right behind the
row the reader pointed at on every shelf that row is filed on, with a fresh resume point. A book
whose address died has nothing to copy and gets the disabled row, the Open row's own rule, and a
link whose target died answers a toast instead, because the menu cannot know a pointer's target
went; the selection's menu duplicates the set, one task and one report, because
two duplicates of one book asked in the same tick would race for one counter name.

A SHELF is the same answer at the tree's own scale: its duplicate is a second tree of the reader's
own holding fresh copies of the books inside it, with the whole subtree copied along — every
member through the one store batch, so one card carries the whole run and a member the store
refused is dropped, not shared; every shelf a fresh id, the sibling order the level reads kept,
the copy's root hung where the original hangs and spliced in right behind it, because the shelf
list IS the render order. The root wears the level's counter (`library_core::conflict::next_shelf_name`, the
shelf half of the same convention) and everything inside it keeps its own name, since the copy's
levels are fresh and hold nothing to collide with. Every copied shelf is `ShelfKind::Virtual`
whatever the original was, and that is a rule rather than a simplification: one directory is one
linked shelf, and a folder's `shelf_map` names one shelf per rung, so a second folder shelf of one
rung would be two doors to one directory with only one of them on the ledger — and a virtual shelf
is no scan's business, so the copy answers to no re-hang, no fold and no departure. The door is the
right-click's folder menu and the crumb's own popover, which is the second door to every other act
a shelf has.

A card's right-click is stopped before the hold's exhaust is even asked about. A completed hold
answers with a synthetic `contextmenu` on some platforms, and one that went on to bubble would open
the LEVEL's menu under the finger that was busy selecting.

The current shelf's crumb carries the shelf's own popover: a rename in place — the thing being
named is the thing being typed over, so no dialog has to describe a shelf the reader can already
see — a duplicate, and the removal, with the watched-folder note when one applies. It is the second
door to those acts beside the right-click's folder menu, and both renames — the crumb's field and
the sheet — commit through the one service, so two doors to one act cannot differ about what it
means.

## Known gaps: two gates deliberately not run

Both were built and tried. Both stay off for the same reason — a gate that is
red on the day it lands teaches people to read past red — and both belong to a
dedicated commit rather than to a check nobody can act on.

### cargo fmt --check

The codebase is hand-formatted in a style rustfmt >=1.9x would rewrite across
roughly forty files, so the gate would fail on pre-existing code. The one-time
formatting pass is the prerequisite, and it is a large mechanical diff that
should not share a commit with anything else.

### rustdoc's broken intra-doc links

The gate that was tried:

```
RUSTDOCFLAGS='-D rustdoc::broken_intra_doc_links' \
  cargo doc --no-deps --document-private-items --workspace --exclude mareader-shell
```

It works, and it is not a small find: 33 unresolved intra-doc links across the
three crates rustdoc reached before cargo stopped the build — app-chrome,
pdf-engine and the app itself — with the other eleven never getting far enough
to be counted. They are the same failure `check-doc-paths` exists for, in the
one form it cannot read: a `//!` that names a sibling module without `super::`,
a `search` that is both a function and a module, a `ScrollShell` left behind
when its shell moved module.

Not all of them can be fixed with a path. `GlossMark::context` points at a
struct field, and rustdoc has no link form for one at any prefix, so those are
prose rewrites — and the eleven undocumented crates cannot be enumerated short
of running rustdoc until they can.
