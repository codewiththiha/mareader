# EPUB layout: the book's own design

Designed pages (colour plates, character pages, title pages, boxed notes)
are drawn as the book lays them out: its grids, boxes, rules, type sizes,
spacing and embedded fonts. Prose chapters keep the book's type (sizes,
weights, indents, alignment, margins) moved by the reader's settings, and
read in one column. Colours always follow the reader's theme; pictures
keep their own.

## Decisions (2026-10-20)

| Question | Decision |
| --- | --- |
| Engine | Our own reflow engine grows a CSS box model; no web engine. |
| Colours | Book layout, theme paper and ink. Book text colours show, adapted to the theme (request #5). |
| When | Automatically per chapter; a setting can force it on or off. |

## Model

The stream stays flat: every paragraph is still a top-level block, so
anchors, highlights, search, CEFR marks, notes and page AI keep working.
Structure rides on each block's frame:

- **Look**: the computed text style of a designed block (size, line
  height, letter and word spacing, indent, weight, faces, justification)
  and the space above it, margins collapsed as CSS does.
- **Boxes** (L2): the styled boxes around a block, outermost first:
  margins, borders, padding, width and auto-margin centring. A box spans
  every block inside it and splits across pages.
- **Cells** (L3): a block's place in a grid (table or flex row). A grid
  row's blocks are laid side by side and paginate as one unit.
- **Float** (L4): the floated element a block is in. Its blocks shrink to
  their widest line at one side; the text after it breaks line by line
  past it, and pictures or other boxes start below it.

Boxes and rows are shared by their blocks, so a box is known by its
address. Pagination keeps the boxes open on the page; a page break cuts
a box's border there and reopens it overleaf. Borders are drawn in the
theme's ink, or the book's border colours adapted (see Text colours). A
row taller than a page reads down the page instead.

Backgrounds are drawn in the theme's ink too, as strong as the book's
colour contrasts with its own page: a white line on a black page is a
full-ink line on ours. Under text a background stays faint, so the text
reads. Background pictures keep their colours, with `background-size`,
`-position` and `-repeat`, clipped to the box. A set `height` holds a
box open below short content.

## Text colours

`color` (and `<font color>`, `<body text>`) is inherited as CSS says.
The book's own text colour, its body's or black, stands for the theme's
ink. Any other colour is drawn on the page's greys: as far toward the
paper as its contrast with its background falls short of the book's
text, at most 55%, so it stays readable; never darker than its hue
needs. The hue rides on that grey.

Borders take `border-color` (a side's own, or the `border` shorthand's),
and with none set, the element's text colour, as CSS's `currentcolor`.
They are drawn the same way but may fade to 85%, so a book's light grey
rule stays light. A data table's grid takes the first border colour its
table or cells draw. List markers take their item's colour.

The page pass then maps the grey from the theme's ink to its paper, as
for all book text, and adds the hue back around it. If the hue does not
fit (a pale ink in a dark theme), the grey moves up to 0.1 toward the
middle, then the hue shrinks to fit. A theme with less contrast than
0.5 between ink and paper, as dim, mutes hues as much (to a quarter at
least), so colours stay as quiet as its text. Greys are drawn exactly as before,
so books without colours look the same, and a theme switch stays a
uniform change, with no new layout.

## Cascade

`css.rs` is a real cascade: compound selectors with descendant, child,
adjacent and sibling combinators, ids, classes, attributes, `:first-child`
and `:not()`, specificity, source order, `!important`, inline styles, a
small user-agent sheet, `@media` (screen, all), `@font-face`, inheritance
and computed lengths (px, pt, pc, in, cm, mm, em, rem, ex, ch, %, vh, vw).
Selectors needing later siblings (`:last-child`) don't match: the parser
streams.

## Designed chapters

A chapter counts as designed when its visible elements use layout CSS
(layout tables, flex or grid, borders, block backgrounds, block widths,
`vh` heights), or at least two lighter signals (embedded faces, wide
letter spacing, three or more body sizes). Layout CSS counts only when
the chapter is short (800 characters at most) or at least 30 % of its
text sits inside what used it, so prose with a framed figure or a
bordered note stays prose. The **Book design** setting
picks Auto, On or Off. Under it, **This book** sets one book's own
choice (As set, Auto, On or Off). The choice is saved on the book's
library record as a stamped field, so it merges like a title would
once sync arrives; older builds keep it untouched.

Designed books are made for a phone's page, about 26 of their ems wide.
A designed chapter is set as if the column were that wide, so its type
grows with a wide pane, up to 1.6 times the reader's default size and
never below it, then scales as the reader's size does.

## The book's type and the reader's settings

In Auto and On, every chapter with a style of its own (a sheet or a
`style` attribute) is set in the book's type; Off, or a chapter with no
style, as converted Markdown and text are, reads in the reader's type.
The book's sizes are in its own ems, and the reader's size is the
book's 16 px, so headings, notes and drop numbers keep their steps.
Each reader setting moves the book's value as it moves off its default,
so a bigger setting makes every size bigger rather than one size:

| Setting | Over the book's type |
| --- | --- |
| Font size | Scales every size (default 17 equals the book's own). |
| Line height | Scales a line height the book sets; unset, the reader's. |
| Paragraph spacing | Scales the book's margins (default 1 keeps them). |
| Weight | Shifts every weight by the change from 400. |
| Indent, letter and word spacing | Add to the book's. |
| Justify | On justifies start-aligned text; off leaves the book's choice. |
| Fonts | Embedded fonts stay. In prose, the reader's fonts replace named machine fonts; `sans-serif` and `monospace` take the reader's sans and code faces. Designed chapters keep every named font. |
| Hyphenation | The reader's own. |
| Text colours | The book's, adapted to the theme: see Text colours. |

## Supported layout features

| Feature area | Scope |
| --- | --- |
| Cascade & typography | Cascade engine; Look (sizes, line height, spacing, indent, weight, embedded fonts, justify, margins, spacer paragraphs); designed-chapter detection; Book design setting. |
| Boxes & borders | Borders (solid, double, dashed, dotted; per side; `border-radius`), padding, widths, auto margins, rules from bordered empty blocks, vertical centring of tall flex or `vh` containers. |
| Tables & flex grids | Tables with block cells, fixed and `%` widths shrunk to fit, cell boxes, `vertical-align`, nested tables read flat, one-column tables as framed boxes, content-derived column widths, `colspan` and `rowspan`, flex rows (justify, align, wrap, gap, grow, basis). A flex row holding one layout table lets the table be the grid. |
| Floats, decorations & transforms | Design scale, floats (left and right) with line-by-line text wrapping, `break-before`/`break-after`, underline and line-through, small caps and `text-transform`, sub and super, background colours and pictures, box heights, inline pictures (at most 3 ems tall), `transform: rotate()` about the block's middle. |
| Per-book overrides | Per-book choice in the Fonts tab saved with the book; hostile grids, flex rows, spans, turns, radii and line pictures open within the time cap. |
| Column flex & rowspan | `flex-direction: column` (narrow or aligned items as single-item rows; stretched items as prose; `row-gap`); inline picture `vertical-align`; `rowspan` cells spanning rows; `vh` flex vertical centring. |
| Prose typography scaling | Book type in prose (sizes, weights, indents, alignment, margins, inline sizes and spacer lines) shifted by the reader's settings; unstyled chapters keep the reader's type; designed type scales with the reader's size. |

## Book CSS everywhere (2026-10-22)

Every chapter with a style of its own is now laid out by its CSS, prose
included: boxes with their margins, padding and borders, grids, floats,
turns and the book's page-break rules (`break-before`, `-after` and
`-inside: avoid`, widows and orphans). A bordered note in a prose chapter
keeps its frame and indents rather than becoming a bare rule. What still
tells a designed chapter from prose is only its scale: designed type
grows with a wide pane as above, while prose stays at the reader's size
and keeps the reader's fonts for named machine fonts. Off still reads
every chapter in the reader's type, without the book's boxes.

## Pane-sized pages (2026-10-22)

In the Single and Two pages views a book's pages take the pane's size,
so a spread fills the whole window's height. Each page is what the pane
leaves it (half for two pages) at the pane's zoom: a fit sets the page
at the pane's own scale, and a set zoom makes the page smaller in
layout units, so its text reads larger. The text column runs inside
the **Top and bottom margins** and **Side margins**, never past the
**Longest line** (720 pt by default); a wider page centres it, and Book
layout leans it a little toward the spine. Such a page is shown at its
own size, so a designed chapter's type counts from the reader's text
size, with no card scale. A resized pane sets its book
anew once it has held still for 0.3 s, keeping the place read; the next
book opened in the pane starts at its size. The vertical stream keeps
its column (Column width) and PDFs are unchanged.

## The reader's settings over a book (2026-10-22)

**Text size** is a percentage: 100 % is the standard size, 17 pt, and a
book's own sizes count from it, so its headings and small print grow
and shrink with it. **Minimum size** (8 pt by default) is the smallest
any of a book's text sets. Settings files keep the size in points, so
older ones read as the nearest percentage.

Two switches decide who wins where the reader and the book disagree:

- **Override book font** sets all of a book's text in the reader's
  faces, code keeping the monospace one, and its body copy (paragraphs,
  list items, code blocks) at the reader's text size. Headings keep the
  book's sizes. Off, a book's embedded fonts lead and the reader's faces
  stand in for families it only names.
- **Override book layout** gives paragraphs the reader's line spacing,
  paragraph margin, indent, word and letter spacing and justification
  outright; their gap still meets the margins of the boxes and headings
  around them as CSS margins do, the larger winning. Off, the reader's settings move the book's own: line
  spacing scales the book's, indent and spacing add to it, and the
  paragraph margin adds its distance from 1 to the book's gap, so a
  book with no gaps between paragraphs gains them at 1.5 and one with
  wide gaps loses some at 0.5. A book that justifies stays justified.

Fixed-layout pages leave all of these alone.

## Grouped book settings (2026-10-22)

With a book in the focused pane, the Appearance tab groups its settings
as **Book** (Book styles On or Off, and This book: As set, On or Off),
**Font**, **Paragraph**, **Page** or **Column**, and **Colour**. Page
holds the margins and longest line and shows in Single and Two pages
views; Column holds the column width and alignment and shows in the
vertical stream. A PDF or an empty pane shows none of them. On is the
old Auto: a chapter's own design leads, plain chapters read as prose.
A stored On keeps working.

The Appearance popover adds a **Text** group while a book is focused:
text size, line spacing, paragraph gaps and both overrides.

Checked on 2026-10-22 in a 1600 by 1000 window: Moby-Dick's extracts
(two pages, Black), Alice and Children's Literature in Two pages view,
and the table layout sample on a single page. Extracts sit in their
bordered, inset boxes with right-aligned small-caps sources, headings
centre, and both pages fill the window's height.
