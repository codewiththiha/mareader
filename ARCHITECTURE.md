# Architecture

> Scope note: this document covers the virtualization, motion and format-pipeline design — the
> parts of the reader with the most subtle invariants. For the feature tour, build setup and the
> crate map, see the README.

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
- the public `Virtualizer` exposes reactive mounted items/rows, total size, dominant item, scroll offset and viewport, per-item offsets, the scroll-to APIs, and the measurement controls (report, suspend, resume, retention grace)

This layer is responsible for DOM measurement flow, scroll scheduling, and keeping the geometry authoritative.

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
  so a page bottom never carries a blank band a pushed-over paragraph used to leave. A hidden
  measure column (carrying the page content's own `tx-content` rules, so it measures the reading
  face and not the browser's fallback) then renders every block once at scale 1 with the live
  typography, reads the true heights, and republishes the cut. Pagination is therefore
  measurement-true, and re-cuts whenever a typography knob moves — holding the reader on the
  block they were reading.
- `apply_heights` is the one place the cut, its inverse block→page map and the per-page size
  bookkeeping are written together, so a split can never disagree with its map.
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
  search hits through its own virtualizer, and reports its rendered item heights back so a column
  narrower than the page model still lays out truthfully. Single, spread and horizontal keep real
  A4 sheets.
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
  from the document's format, so the page, the stream and the measure column share one
  answer and none of them knows what Markdown is.
- Text never enters blend mode and never touches the paper session: a text page is recoloured by
  its own tokens, so the backdrop's colour machine is gated off for the format (the Theme tab
  hides the Paper and Rendering sections accordingly). Body ink has its own comfort dial —
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
- **The event grew two optional fields.** `pdfreader:selection-detail` now
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
  document order with the stroke layer and the measure column skipped (a mark's
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
  and the layers a walk must skip — a stroke's button, a hit's box, the measure column's twins —
  are one list in it rather than one per caller.
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
