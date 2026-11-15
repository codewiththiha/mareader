# EPUB pipeline

> **Status:** implemented · **Last updated:** 2026-10-31

EPUB replaces mareader's `.txt`/`.md` pipeline. Everything mareader's text
formats can do, an EPUB page must do too. The choice of stack is in
[ADR 0002](adr/0002-epub-via-parley.md).

## Pipeline

```
.epub ──rbook──▶ spine + ToC + resources
          │
          ▼  html5ever + CSS subset           (crate: epub, L2)
     reflow::Block[]  (heading, para, list, quote, pre, table, image, rule)
          │
          ▼  parley 0.12 + fontique            (crate: reflow, L1)
     lines → pages   (real line heights; never orphan a heading)
          │
          ▼  vello_cpu 0.3 (text as a coverage mask; colour in the page pass)
     page RGBA → texture → page pass (texture + grain; no filter)
```

- **Progressive opening.** A saved EPUB place stores its spine href and a
  block-relative offset, so a deep resume can parse a bounded window around
  the target without laying out the prefix. A fresh book previews its first
  spine item. The preview tries up to three spine items either side (seven
  total); if its markup or block budget is exceeded, it retries the previous
  two-item radius. One complete parse and layout then continues in bounded
  batches after the target raster arrives. Preview pages use local indices plus
  the saved global page offset and count. At completion, stable spine places map
  the view and reusable
  rasters into the full layout; there are no widening prefix snapshots. Older
  saves with only a global block anchor still load in one complete pass and
  retain their place. The counter shows `page / pages+`; its saved total is
  retained until the full count replaces it. The slider and progress line stay
  hidden until the full book is ready. PDF opening is unchanged.
- **Zoom never re-paginates.** Pages scale uniformly, as in mareader. A
  typography change re-paginates and keeps the anchor block in view.
- **Untrusted input.** Paths in the container resolve only inside it. XML depth,
  entity expansion, image dimensions and the total text size are capped. No
  network, ever: remote images and fonts are refused. Scripts are ignored.

## Typography knobs

`TextSettings` from `reader-core/src/settings/typography.rs` ports as-is,
serde names included, so mareader settings JSON loads. The defaults are
mareader's.

| Knob | Default | parley / reflow mapping |
|---|---|---|
| `font_size` | 17 px | `StyleProperty::FontSize` on the body; headings scale from it |
| `font_weight` | 400 | `FontWeight` (variable axis when the face has one) |
| `line_height` | 1.7 | `LineHeight` as a font-size multiple |
| `paragraph_margin` | 1.0 em | block spacing in `reflow` |
| `word_spacing` | 0 px | `WordSpacing` |
| `letter_spacing` | 0 em | `LetterSpacing` (× font size) |
| `text_indent` | 0 em | first-line indent in `reflow` |
| `justify` | off | `Alignment::Justify` (last line start-aligned) |
| `hyphenation` | off | `hypher` break points (ADR 0002, open question 1) |
| `default_font`, `serif_font`, `sans_font`, `mono_font` | Default stacks | fontique families: bundled OFL faces first, then system |
| `book_layout` | off | spread pages get a spine-side gutter |
| `column_align` | centre | where the column sits in the vertical stream |
| `ink_contrast` | mareader's default | mixes the ink token towards paper for body text only |

Publisher CSS supplies structure (emphasis, alignment, block margins). A knob
the user set always wins over it.

## View modes

- **Single and spread:** pages as above.
- **Horizontal strip:** page by page.
- **Vertical:** one continuous column with no visible page breaks (mareader's
  streaming mode), still rasterised in page-height bands so memory stays bounded.

## Features that ride on the layout

- **Search:** starts once the spine is complete, so a partial prefix is never
  reported as the whole book. Hits are cluster rectangles in page space, with
  the current hit in amber `#f59e0b`.
- **Selection and gloss:** hit-test pointer → cluster → word. The word and its
  sentence go to the study tools (`docs/study.md`).
- **Links:** internal hrefs map to spine item + anchor → page. External links
  open in the browser after the same scheme checks as mareader (`javascript:`
  and `file:` refused).
- **ToC:** from nav/NCX, flattened with depth. The active entry follows the
  reading position, and the deeper entry wins on shared pages.
- **Covers:** the OPF cover image, for the library.

## XHTML subset

What the epub crate reads, and what each element becomes:

| Markup | Block | Drawn as |
|---|---|---|
| `h1`–`h6`, `p`, `div` and other blocks | text | heading or body face; `h6` muted |
| `ul`, `ol` (`start`), `li`, `dd` | text, framed | 1.6 em indent per level; `•` `◦` `▪` or `N.` hung in the margin; 0.3 em between items |
| `blockquote` | text, framed | 1 em indent per level, a 3 px bar, softer ink |
| `pre` | preformatted | mono, spaces and tabs kept, long lines wrap; shaded box |
| `code`, `kbd`, `samp`, `tt` | inline | mono at 0.88 em on a light ground |
| `table`, `caption`, `th`, `thead` | table | equal columns, 1 px borders, head rows bold and shaded; splits between rows |
| a `table` holding pictures, `role="presentation"`, or with no heads, block cells and a blank spacer column | its cells' blocks | a layout table, e.g. a character page: each cell reads in order as blocks, pictures shown; columns blank in every row of a data table are dropped |
| `img` with a `width` (CSS em, rem, px, pt or %; or the attribute) | image | that width, with px and pt against the reader's text size; still shrunk to the column and page |
| `hr` | rule | a hairline |
| `img`, SVG `image` | image | its own size, shrunk to the column and the page; never split |
| the OPF cover | image | a page of its own, fitted and centred |

- **CSS:** inline `style`, `<style>` and linked stylesheets. Selectors `tag`,
  `.class` and `tag.class`; anything else, and every @-rule, is skipped.
  Properties: `font-style`, `font-weight`, `text-align`, `display: none`.
  `justify` reads as start until the typography slice.
- **Images:** read once per path, only from inside the archive. The header is
  probed when the book opens. Past 16 384 px a side, 50 MP or a 256 MiB
  budget per book, the alt text shows in italics. Pixels are decoded when a
  page needs them, at a power-of-two size near the drawn size, and kept in a
  64 MB cache. One that won't decode draws as a grey box.
- **Faces:** the mono face is JetBrains Mono (OFL). Every face falls back to
  Inter for symbols it lacks.

## Typography implementation

- **Settings to type.** `session::typography` turns `TextSettings` into
  `reflow::Typography`. The body takes the serif slot until the Font slot
  names a face; headings take the sans slot, code the mono slot.
- **Stacks.** A slot's pick comes first, then its bundled face (Literata,
  Inter, JetBrains Mono) as the last resort. System picks expand to their
  CSS stack; `serif`, `sans-serif` and `monospace` mean the machine's own
  face of that kind. fontique scans the machine once (fontconfig is opened
  at run time, so a box without it just has no system faces).
- **Hyphenation.** `hypher` marks break points with U+00AD in words of five
  letters or more, never inside inline code. The patterns follow the book's
  `dc:language`, falling back to English. parley breaks there but draws
  nothing, so the raster draws the hyphen at the line end.
- **Justify and indent.** `Alignment::Justify` leaves last lines ragged.
  The first-line indent applies to body paragraphs only; headings, quotes,
  list items, table cells and code start flush, as mareader's do.
- **Re-setting keeps the place.** A change re-sets each open book off the
  UI thread; the old pages stay up until the new ones arrive as
  `Event::Retypeset`. The view keeps the anchor of the page it showed and
  lands on the page holding it. Going back to the old type returns to the
  same page, since the stored anchor is reused, not re-derived.
- **Fonts tab.** Settings › Fonts: the four slots, size and weight
  steppers, spacing dials, and the justify and hyphenation switches.
  A system face that isn't installed shows as "Not installed".
- **Text for each pane (owner's extension).** Settings › Panes ›
  "Independent text for each pane" gives each split pane its own Fonts
  settings. The window's text mirrors the focused pane's, so the Fonts tab,
  the ink dial and shot keys edit that pane alone (`PaneTexts`). A new pane
  copies the focused one; turning the switch off or closing to one pane
  hands the focused pane's text to the window. Documents are per file and
  type: panes on one file share a parse while their type matches, and a
  re-set forks a new document in a per-pane slot, leaving the old one to
  any pane still on it.

## View modes implementation

- **One raster per page in every mode.** Single, spread, the horizontal
  strip and the stream all draw the same page textures, so switching modes
  draws nothing new and zoom never re-paginates.
- **Book layout.** The column moves toward the spine: 56 units on the
  outer edge, 92 on the spine side, so the column is 646 wide instead of
  650 and the book re-paginates once. Spreads pair pages from the first,
  so even pages (0-based) sit left with their gutter on the right. Single
  pages and the strip follow the same parity, unlike mareader, whose lone
  pages count the other way; one rule keeps one texture per page.
- **The stream.** The vertical strip of a book shows each page's band:
  its column, cut to the text plus the block gap its page cut swallowed
  (a chapter start gets a pad's worth). Bands are laid edge to edge with
  no gap or shadow, on the pages' own paper, so the pane reads as one
  column. Each page is drawn whole and clipped to its band (with 24 units
  of slack either side for overhanging glyphs), so pagination, anchors,
  page numbers and the page cache stay those of the paged modes.
- **Column alignment.** Left, centre or right across the pane; off the
  centre, the page margin keeps the column off the edge, at least 16 pt
  (mareader lets it touch at a zero margin).
- **Fit.** Fits measure whole pages, so the type is the same size in the
  stream as on the paged modes' pages.
- **Settings.** Book layout and column alignment sit in Settings › Fonts.

## Navigation

- **Places.** A live anchor is a block and byte offset, never a page, so
  it outlives re-pagination. Every element `id` (and old `<a name>`) marks
  one: mid-paragraph at its offset, otherwise at the next block. Each chapter
  file marks its first block. Durable resume adds the spine href and a
  block-relative offset, so a later open can find the chapter without parsing
  every earlier one.
- **Contents.** The EPUB 3 nav, else the EPUB 2 NCX, flattened with
  depth (capped at 20 000 entries and 5 levels deep). An entry whose
  target never resolves is dropped. A book that lists nothing gets its
  headings, all six levels, as mareader outlines its Markdown. Pages are
  worked out again from the places on every re-set, and the outline panel
  marks the active entry as it does for a PDF.
- **Links.** Linked words are underlined in the ink they're set in, not
  mareader's accent: pages are drawn in greys and themed afterwards.
  Each line a link crosses gets its own hit box, read from parley. Links
  inside the book open on the page holding their place; others go through
  the PDF path's scheme check, so `javascript:` and `file:` do nothing.
- **Streams.** A sheet is hit-tested only where its band shows, so a link
  near a band edge isn't taken by the neighbouring page's hidden part.
- **Resume.** A book read at exit saves its stable spine place, global page
  and page count; the next launch opens at that place, even in another type.
  Older saves with only a global block anchor remain supported.

## Selection and ink

- **Characters.** Each page's text comes from parley's clusters, line by
  line: one box per character, as wide as its share of the cluster and as
  tall as the line. Spaces are kept as characters, while soft hyphens and
  the hyphens drawn at line ends are not. List markers are left out, as a
  browser leaves out CSS markers. The page reaches the views as the same
  text layer a PDF's page gives, read on a worker, so every selection
  gesture works unchanged: drag, wheel and edge scroll, double click,
  ⌘C or Ctrl+C, Escape.
- **Copying.** A line that wrapped joins the next with nothing between
  them: the space it broke at stays on the line, and a hyphenated word
  rejoins. A line that ended (a new block, `<br>`, a line of `<pre>`)
  gives a line break. A page that opens mid-paragraph joins the page
  before in the same way.
- **Triple click.** It takes the paragraph's lines on that page, as a
  browser does; on a PDF, where no line wraps into the next, it still
  takes one line.
- **Ink dial.** The dial is mareader's "Text ink intensity", 10 to 100 %.
  It sits under the tint in the Appearance popover while the active pane
  shows a book, and in Settings › Fonts. The page pass mixes the drawn
  greys toward white before the theme, folded into the first matrix of
  the chain, so it costs nothing and moves with the slider. Everything
  drawn in ink lightens (headings, rules, borders and code grounds too, as
  mareader's tints derive from its ink). Pictures don't: each is drawn a
  second time over its own box in the pane's plain tone. Thumbnails keep
  full ink. Without a GPU, pages aren't themed and the dial does nothing.
  *Since the follow-ups, pictures keep their own colours unless Theme
  pictures is on (§ Layout and per-pane text follow-ups).*

## Hostile books

A book is untrusted input. Whatever its bytes, opening it ends in a book or
in one of three refusals (`OpenError`): `Invalid` (not a zip or no
package), `Empty` (nothing to read) and `TooLarge` (past a cap below). On
a refusal the pane closes and a toast says why, such as "the book is too
large to open safely". Nothing crashes.
`crates/epub/tests/hostile.rs` builds each attack and checks the outcome.

**Caps.** Each is far past any real book and stops one kind of attack.

| Where | Cap | Stops |
|---|---|---|
| Zip directory (`guard.rs`) | 10,000 files; 128 MiB a file; 512 MiB in all | zip bombs, before anything inflates |
| Inflating | the size the directory declares | a file lying about its size: zip stops at the declared size |
| Markup (`book.rs`) | 64 MiB of chapter text | books too big to lay out |
| Contents (`guard.rs`) | 64 levels of nav or NCX nesting | stack overflow in rbook, which builds and frees the tree recursively |
| Metadata (`guard.rs`) | 8 links in a `refines` chain | the same in rbook's recursive metadata |
| XHTML (`xhtml.rs`) | 256 open elements; 16 stylesheets a book | deep markup; one sheet re-linked by every chapter |
| CSS (`css.rs`) | 1 MiB a sheet; 10,000 distinct selectors | rule floods; a selector repeated merges into one rule |
| Pictures | header-probed sizes, as in slice 2 | a small file declaring 100,000 px a side shows its alt text |
| Layout (`book.rs`) | 1 GiB estimated | out-of-memory: see below |

**The guard.** Before rbook sees a book, `guard::open` checks the zip
directory. It then reads the book bare, without contents or metadata, and
scans the package file with quick-xml the way rbook reads it. A nav or NCX
nesting deeper than the cap is skipped and the headings stand in, as for a
book listing nothing. A refinement chain past the cap skips the metadata, so
the book opens untitled. If rbook still rejects the contents or metadata,
the book opens without them rather than not at all.

**Broken markup reads.** html5ever's tokenizer recovers from unclosed and
stray tags and unknown entities. A chapter whose bytes aren't all UTF-8 is
decoded lossily: the stray bytes become U+FFFD and the rest reads. rbook's
own reader drops such a chapter whole, so chapters are taken from the spine
directly. A spine entry naming no file, a picture path escaping the book
(`../`, `/`, `file:`) and a picture that won't decode are skipped or shown
as alt text.

**The layout budget.** The pipeline above plans lazy layout by chapter. As
built, a book is laid out whole when it opens, and every paragraph keeps its
parley layout, about 3,000 bytes plus 48 a byte of text (measured; see
`docs/memory.md`). A book whose estimate passes 1 GiB is refused before
layout. *War and Peace* comes to about 190 MB, the complete Shakespeare to
about 560 MB. Laying out on demand would lift the limit; it is a follow-up.
*Superseded: layouts are now laid on demand and blocks are capped instead
(§ Layout and per-pane text follow-ups).*

**EPUB 2.** Books with only an NCX, an OPF 2 cover (`<meta name="cover">`)
and HTML entities open as EPUB 3 books do. A real one is a test fixture:
Project Gutenberg's *The Raven* (`crates/epub/tests/books/`), with a nested
NCX, a page list and a cover wrapper page.

**Fuzzing, cheaply.** One test flips bytes in copies of the sample book 48
times, seeded, and opens each; every copy opens or is refused.

## Layout and per-pane text follow-ups

Post-implementation refinements to EPUB layout, pictures and per-pane typography.

- **Layout on demand.** Opening a book still lays every block out once, to
  measure its lines and cut exact pages, but keeps only the line boxes.
  A block's parley layouts are laid again when a page is drawn, its text is
  read or its links are asked for. The most recent are kept, up to 32 MiB by
  `Block::layout_cost` (`MAX_LAID`); past that the oldest go until half is
  left. Laying again uses the same blocks, type and fonts, so pages, places
  and selections are identical (`a_page_laid_again_draws_and_reads_the_same`).
  Only pieces in view are laid, so a tile of a page lays only its part. The
  fonts sit behind a lock, so workers lay one block at a time; drawing
  doesn't hold it.
- **Blocks capped, not layout.** With layouts no longer kept, the 1 GiB
  estimate is gone. What stays per block is its text and line boxes, about
  300 bytes for a one-word paragraph (`docs/memory.md`), so a book of more
  than 1,048,576 blocks (`MAX_BLOCKS`) is refused as too large.
- **Pictures keep their colours.** In every theme a book's pictures are
  drawn again over their boxes untoned, as mareader leaves them. Settings ›
  Fonts › Theme pictures tints them with the page instead (the ink dial
  still leaves them at full strength). PDF pages are themed whole, as before.
- **A book's own paper and ink.** As mareader's text pages, a book is not
  filtered like a PDF: its page is drawn clear and the page pass paints the
  theme's text palette under it and recolours the black text to its ink
  (light .98/.15, dark .24/.92, dim .22/.12 OKLCH lightness, chroma growing
  with the tint). The pane around it takes the same paper. With a blend
  colour or bands on, the paper is the PDF's dominant colour as before.
  Colour emoji (COLR, CBDT, sbix or SVG glyphs) are drawn again untoned like
  pictures; Settings › Fonts › Theme emoji tints them instead (off by
  default).
- **Thai, Burmese, Khmer and Lao lines.** These scripts write no spaces
  between words, so parley's `complex-scripts` feature brings ICU4X's
  dictionary segmenters (about 5 MB of data in the binary). Without them
  ICU4X printed "No segmentation model for complex script" on every
  layout and broke lines only at spaces.
- **Blend beside a PDF.** As mareader's shared paper, every pane shows the
  paper of the most recently focused pane that has one. A book has none, so
  focusing it keeps the last PDF's paper on it and on its neighbours; only
  when no PDF is open does a book show its own text paper. With independent
  themes each PDF keeps its own paper and each book its text paper.
- **Column Width.** mareader's dial, 60 to 140 %. The owner moved it from
  Layout to Settings › Fonts › Layout (2026-10-15), so it is a text setting
  and follows "Independent text for each pane"; settings v2 carry the old
  value over. The text column scales, never below 160
  units, and the page grows or shrinks with it, keeping its margins. Each
  step re-sets the book off the UI thread and keeps the reading place.
- **Kept as built:** one binding-side rule for every view (§ View modes
  implementation), and the line-end hyphen hanging just past the column's edge.

## Text files as EPUBs (owner request, 2026-10-14)

Markdown and plain text open by converting them to an EPUB in memory,
then opening that EPUB like any other book. Every EPUB setting applies,
with no second pipeline (`crates/epub/src/convert/`).

**What it does.** Plain text gets a paragraph style (a blank line, an
indent, or every line), hard-wrapped lines are unwrapped, and chapter
headings are found by heuristics. Markdown keeps footnotes and tables, a
chapter starts at each `#` heading, the contents come from headings to
`###`, the title and author come from front matter, and pictures are
packed. Legacy encodings are guessed.

- **Which files.** mareader's extensions: `.txt` and `.text` are plain
  text; `.md`, `.markdown` and `.mdown` are Markdown. They are told by
  name, because text has no signature; every other file is still told by
  its content. The open dialog offers them. A file is capped at 64 MiB
  (`MAX_TEXT`); past that it is refused as too large, like a large EPUB.
- **Encoding.** A byte order mark wins (UTF-8 or UTF-16), then valid
  UTF-8, then chardetng's guess (Windows-1252, GBK, Shift_JIS and so on)
  decoded by encoding_rs. mareader read everything as lossy UTF-8.
- **What gets built.** An EPUB 3 container with `content.opf`, a nav
  document and one XHTML file per chapter, stored uncompressed in memory.
  Nothing is written to disk. Each chapter starts a new page, as in any
  book.
- **Markdown.** pulldown-cmark with GitHub's flavour, as mareader rendered
  it: tables, task lists, strikethrough, footnotes, alerts, definition
  lists, and `{#id}` on headings.
  - **Chapters and contents.** Each `#` heading opens a chapter; text
    before the first one is a chapter of its own. Headings
    `#` to `###` make the contents.
  - **Ids and links.** Ids follow GitHub's slugs, so `[x](#a-heading)`
    lands even in another chapter. Footnote marks link to their notes,
    which sit where they were written, under a rule.
  - **Title and metadata.** The title comes from front matter, else the
    first heading to `###`, else the file name. The author and language
    come from front matter too; YAML or TOML, as mareader read it.
  - **Pictures.** Local pictures (relative or absolute paths, PNG, JPEG,
    GIF or WebP) are read from beside the file and packed: 32 MiB each,
    256 MiB in all. An `<img src>` in raw HTML, common in READMEs for its
    width, is packed the same way. Web pictures aren't fetched; their alt text shows.
  - **Shown as text.** Task boxes are drawn as □ and ■, which every
    bundled face has, in place of the bullet, as on GitHub: the XHTML
    reader now honours `list-style: none`, which EPUBs use too. Raw HTML passes through to the hostile-proof XHTML
    reader; it runs no scripts.
- **Plain text.** Markup is never read, as in mareader: `# x` is two
  characters.
  - **Paragraphs.** Decided by the text: blank lines if there
    is one per 20 lines or more; else an indent starts a paragraph if one
    line in 20 is indented; else every line is one.
  - **Wrapped lines.** A paragraph joins into one when at least four
    breaks in five fell where the next word wouldn't fit the file's width
    (its 95th-percentile line). Verse breaks where the next word would
    have fit, so it keeps its lines, and their indents. A word hyphenated
    at a wrap joins without a space.
  - **Headings.** One-line paragraphs: `Part`, `Book` or `Volume` with a
    number are level 1; `Chapter`, `Act`, `Section`, `Canto`, `Scene` or
    `Letter` with a number, a word such as `Prologue` or `Epilogue`, or
    `第…章` style lines are level 2. So are a lone numeral where blank
    lines are the paragraph breaks, and a line in capitals with two or
    more blank lines before it (`COMMENT ON THE POEM.`, but not a
    signature after one). Each heading opens a chapter. A file with none
    gets one entry, its title.
  - **Title.** The file name.
- **Not built.** The converted EPUB isn't saved: a library copy can come
  with M5 if wanted. Strikethrough shows as plain text, as `<del>` does
  in any EPUB here. The system isn't asked to open `.txt` or `.md` with
  mareader; mareader registered only `.pdf`.
- **Proof.** `crates/epub/tests/convert.rs` opens Project Gutenberg's
  *The Raven* as plain text (CRLF, wrapped prose, verse, headings in
  capitals) and `river.md`. Unit tests in `convert/tests.rs` cover
  encodings, paragraph styles, headings, front matter, chapters, links,
  footnotes and pictures. The session tests open both formats by name.

### Text file edge cases (2026-10-15)

- **Names that say more.** Chinese novels are named `《河流》作者：林.txt`,
  `《河流》[林].txt` or `【河流】作者:林.txt`: the title and author come
  from the name. A copy mark goes: `notes (1).md` is titled `notes`.
- **Labelled lines in the head.** Within its first 40 lines, a plain text's
  `Title:` and `Author:` lines (or `书名：`, `标题：`, `作者：`) win over
  the name, as front matter does in Markdown. Project Gutenberg's files
  carry them, so *The Raven* is titled "The Raven", by Edgar Allan Poe.
- **Chinese headings.** Numerals include the traditional and formal forms
  (`两`, `兩`, `萬`, `貳`, `拾` and kin). Units that are also measure words
  (`卷`, `部`, `集`, `本`, `册`) head a part only when a space or colon
  parts them from the title or nothing follows, so `第一本书记载着…` stays
  prose. A title runs 36 characters at most after its unit, and a line
  ending in `。`, `，` or `；` is a sentence.
- **A missed heading style costs only its own chapters.** Lines are judged
  one at a time, so an unknown style joins the chapter before it; the
  book's other headings stand.
- **No chunking of heading-less files.** A book is one list of blocks
  however many chapters it has, so splitting a long file into chapters of
  100 paragraphs would buy no speed and fill the contents with numbers.
- **Same text, same bytes.** The zip's dates and the package's modified
  date are fixed, so a file converts to identical bytes every time. The
  library relies on this to cache the result (`docs/library.md` § Text
  files in the library).

## Footnotes (owner request, 2026-10-18)

A click on a footnote's mark opens the note in a card on the mark, its
caret pointing there, instead of jumping away from the page.

- **Which links.** A link marked `epub:type="noteref"` or
  `role="doc-noteref"`; one to an element marked as a footnote, endnote
  or note (EPUB or ARIA); or a link whose words look like a mark ("3",
  "[12]", "*", "iv") pointing at an id. A mark-like link to a heading,
  or to more than three blocks, is a cross-reference and still jumps.
- **What shows.** The element the id is on: every block it holds, up to
  12 blocks and 2,000 characters. An id inside a paragraph shows that
  paragraph from the id on. Arrows back to the mark are dropped.
- **The card** reads at the page's size, scrolls when long, and has
  **Go to page N** for the note in its place. Escape, × or a press
  outside closes it; so does scrolling its mark off screen.
- Markdown files mark their footnotes this way when converted.

## Pictures opened large (owner request, 2026-10-18)

A double click on a book's picture opens it over the window, dimmed
behind, at its own pixels rather than the page's size for it.

- **Zoom and pan.** The wheel or a pinch zooms about the pointer; a drag
  pans once the picture outgrows the window. A double click switches
  between fitting the window and one pixel per point (or twice the fit,
  for a picture already enlarged). Fitting enlarges at most four times;
  zoom stops at eight points a pixel.
- **The bar** at the foot: Zoom out, the zoom (pressed, one pixel a
  point), Zoom in, Fit to window, Close. Keys: `+` and `-` zoom, `0` fits, Escape closes; a click
  beside the picture closes it too. The app's other shortcuts wait
  while it's open.
- **Decoding** runs on its own thread, up to 4096 pixels on the long
  edge (less if the graphics card's textures are smaller), so a large
  picture never stalls a frame; a spinner shows until it's ready.
- **PDFs.** The text layer also lists where a page draws its images. A
  double click on one, away from any word, draws that box of the page
  at up to eight pixels a point and opens it the same way. Where images
  overlap, the smallest is taken, so a figure on a scanned page opens
  rather than the whole scan.

## Revision history

| Date | Summary |
|---|---|
| 2026-10-13 | Initial EPUB pipeline plan |
| 2026-10-14 | XHTML subset implemented |
| 2026-10-14 | Typography knobs, hyphenation and Fonts tab implemented |
| 2026-10-14 | Reflow view modes (stream, single, spread, horizontal strip) implemented |
| 2026-10-14 | Navigation (ToC, internal/external links, position anchors) implemented |
| 2026-10-14 | Cluster selection, copy and text ink contrast implemented |
| 2026-10-14 | Hostile-book guards and EPUB 2 support implemented |
| 2026-10-14 | Follow-ups: layout on demand, pictures, Column Width |
| 2026-10-14 | Text files as EPUBs: Markdown and plain text converted on open |
| 2026-10-15 | Per-pane text settings; Column Width moved to the Fonts tab |
| 2026-10-15 | Text file edge cases: names, labelled heads, Chinese headings, repeatable bytes |
| 2026-10-18 | Footnotes shown in place |
| 2026-10-18 | Pictures opened large, and PDF figures |
| 2026-10-20 | Layout tables read as blocks, spacer columns dropped, image widths |
