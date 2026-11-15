# EPUB support: what a full reader draws

The EPUB 3.3 reading system rules, checked item by item against our
engine (2026-10-21), and the capabilities implemented to cover them. Layout
itself (boxes, grids, floats, the book's type) is in
[epub-layout.md](epub-layout.md).

## What the specification asks

| Area | Level | Rule |
| --- | --- | --- |
| Core images | MUST | PNG, JPEG, GIF, WebP and SVG. |
| Fonts | MUST | TrueType, OpenType, WOFF and WOFF2; obfuscated fonts are de-obfuscated (IDPF and the older Adobe scheme). |
| CSS | MUST | The CSS Snapshot as browsers do it; the HTML suggested rendering as the default sheet; `@supports`; vendor `@media` types ignored. |
| SVG | MUST | SVG pages in the spine and SVG inside chapters, styled by CSS. |
| MathML | MUST | Presentation MathML drawn (a script library is allowed). |
| Fixed layout | MUST | `pre-paginated` books: one page per spine item, sized by the viewport `meta` or the SVG `viewBox`, no margins added. |
| Language | MUST | `lang`/`xml:lang` and `dir`; `page-progression-direction`; vertical writing. |
| Fallbacks | MUST | Manifest fallback chains, `object` content, `epub:switch` (deprecated) fallbacks. |
| Navigation | MUST | The nav document; the NCX for EPUB 2; page lists and landmarks. |
| Scripting, media overlays, audio and video | SHOULD / MAY | Optional. |

## Audit (2026-10-21)

| Feature | State before | Section |
| --- | --- | --- |
| PNG, JPEG, GIF, WebP | Done | — |
| SVG files (`img`, `image`, `object`, `embed`) | Not drawn | SVG |
| SVG inside chapters | Skipped; only its `image` showed | SVG |
| SVG pages in the spine | Not read | SVG |
| `data:` images | Refused | SVG |
| Black art on transparent ground | Kept its colours, so it vanished on dark paper | Black art |
| Borders, backgrounds and padding in prose chapters | Drawn only in designed chapters | Boxes in prose |
| `::before`, `::after`, `content`, counters, quotes | Dropped | Generated content |
| `::first-letter` drop caps, `::first-line`, `::marker` | Dropped | Drop caps and first lines |
| `list-style-type` (roman, alpha, none, strings), `list-style-image` | Decimal and bullets only | Lists |
| `border-image`, `box-shadow`, gradients, multiple backgrounds | Missing | Backgrounds and borders |
| `outline`, `opacity`, `visibility`, `text-shadow`, `filter` | Missing | Backgrounds and borders |
| `hr` in the book's width, style and colour | The reader's faint rule | Rules as the book draws them |
| `calc()`, `var()`, `min()`, `max()`, `clamp()` | Missing | Values and queries |
| `@media` features (`prefers-color-scheme`, widths), `!important` | Partly | Values and queries |
| `@import`, `@supports`, `@layer`, `@container` | Skipped, with all their rules | Values and queries |
| `initial`, `inherit`, `unset`, `revert` | Read as unknown values | Values and queries |
| `media` on `link` and `style`, alternate sheets | Every sheet applied | Values and queries |
| Obfuscated fonts, WOFF, WOFF2 | Missing | Fonts the book ships |
| `font-variant`, `font-feature-settings`, `text-align-last`, widows and orphans, `break-inside` | Missing | Type features and page breaks |
| `dir`, right-to-left text and page order | Missing | Right to left |
| Ruby | Read as plain text | Ruby and run fonts |
| Vertical writing (`writing-mode`) | Missing | Vertical writing |
| Fixed layout (`pre-paginated`, spreads) | Read as reflowable | Fixed layout |
| MathML | Skipped | MathML |
| Audio, video, `object` fallbacks, `epub:switch`, manifest fallbacks | Missing | Media and fallbacks |

## Supported capabilities

| Capability | Scope |
| --- | --- |
| SVG, black art & prose boxes | SVG everywhere (files, inline, spine pages, `data:`); black art on transparent ground follows the theme's ink; painted boxes (borders, backgrounds, padding, radius) in prose chapters. |
| Generated content & lists | `::before`/`::after` (strings, `attr()`, counters, quotes, pictures, empty bordered blocks), drop caps, `::first-line`, `::marker`, every list style and list pictures. |
| Backgrounds, borders & rules | `border-image`, shadows, gradients, layered backgrounds, outlines, opacity, visibility, text shadows, `hr` as the book draws it. |
| CSS values & queries | Maths functions, custom properties, media features, importance. |
| Fonts & type features | De-obfuscation, WOFF and WOFF2, font features, last-line alignment, widows and orphans, unbroken blocks. |
| Bidirectional text & ruby | Bidirectional text, right-to-left page order, ruby, runs in their own font family. |
| Vertical writing | `writing-mode`, upright and sideways runs, columns read right to left. |
| Fixed layout | Pre-paginated pages at their viewport, spreads, absolute positions. |
| MathML | Fractions, scripts, roots, rows, operators, tables. |
| Media & fallbacks | Audio and video as their poster or a labelled box, `object` and `epub:switch` fallbacks, manifest fallback chains. |
| Read-aloud overlays | Media overlays played in the reader, the phrase read lit, pages turning to follow. |
| Subtitles & transcripts | `track` files beside clips handed to the player, and transcripts shown in the pages. |
| Interactive triggers | `epub:trigger`: clicks that show and hide parts of the page, and play, pause and mute clips. |
| Scripted content | A chapter's scripts run in a sandboxed engine and change the page, which is laid again. |

## SVG

SVG draws through resvg at the size it is shown, so it stays sharp at any
zoom. A file's size comes from its root `width`, `height` or `viewBox`;
images it links are read from the book. An inline `svg` is kept as markup
(names keep their SVG case) and drawn as a picture; one holding only an
`image`, the usual cover wrapper, still shows that image directly.

## Black art

A picture that is mostly see-through and whose marks are dark and nearly
colourless (line drawings, ornaments, title lettering) is drawn as ink: it
takes the theme's tone like text, so it reads on dark paper. Coloured and
opaque pictures keep their own colours.

## Boxes in prose

Prose chapters draw the boxes the book paints (a border, a background
colour or picture), at the reader's size, with their padding, margins,
width and corners. Boxes that only move text are left out, so prose still
reads in the reader's column.

## Generated content

`::before` and `::after` (one colon too) show their `content`: strings with
CSS escapes, `attr()`, `counter()` and `counters()` in any list style,
`open-quote` and `close-quote` by `quotes` and depth, and `url()` pictures.
A block box is laid as a block; an empty one shows only when it draws
something, so a bordered divider is a rule and a clearfix is nothing.
An `hr` with generated content shows that ornament instead of a line.
`q` quotes as browsers do. Counters reset, increment and set as CSS says;
one never reset counts from the root, and `list-item` is the list's count.

## Drop caps and first lines

A floated `::first-letter`, or a short floated run starting a paragraph,
is a drop cap: the letter is drawn large, spanning lines by its size, and
the text wraps beside it. The letter stays in the text, so search and
selection still read the word. Unfloated, the letter is only restyled.
`::first-line` styles what the first line holds, after a few passes. Both
need the book's design; read as the reader's own, the text is plain.

## Lists

Every `list-style-type` counts its own way: decimal (with a leading zero),
Roman, Latin, Greek, Armenian, Georgian, Hebrew, kana and iroha, CJK, the
native digits of Arabic, Indic, Thai, Lao, Khmer, Tibetan, Mongolian and
Myanmar scripts, symbols, and strings. `type` and `value` attributes count
as in HTML. `list-style-image` hangs the picture as the marker, at most a
line tall. `::marker` sets the marker's text and colour.

## Backgrounds and borders

Boxes draw what CSS paints around them, in the theme's ink:

- `box-shadow`, outer and inset, blurred and spread, several per box.
- `linear-gradient` and `radial-gradient`, repeating too, with the old
  prefixed forms; several background layers, the first on top.
- `border-image`: the picture cut in nine by `slice`, its sides stretched,
  repeated or rounded, with `width`, `outset` and `fill`.
- `outline` and `outline-offset`, outside the border, taking no room.
- Groove, ridge, inset and outset borders in two tones.
- A transparent border takes its room and draws nothing.
- `opacity` fades a box and everything in it, text included.
- `visibility: hidden` keeps the room and draws nothing; the text is still
  found by search. `filter` applies `opacity()` and `drop-shadow()`;
  colour filters are left to the theme.
- `text-shadow` draws a soft copy behind the glyphs.
- Background pictures and border pictures may be `data:` URLs.

Shades are strengths of the ink, as backgrounds are, so they follow every
theme. A gradient behind text is kept faint enough to read; in an empty box
it shows at full strength.

## Rules as the book draws them

An `hr` the book styles itself (a border, a background, a gradient, a
picture, a shadow) is drawn as a box in the book's width, margins and
style, so a centred short line, a double rule or a gradient fade shows as
designed. Read as the reader's own, it is the reader's plain rule. An `hr`
the book leaves alone is the reader's faint rule.

## Values and queries

Lengths may be sums: `calc()`, `min()`, `max()` and `clamp()` work out to
pixels where they can; a share of the column stays a share, so
`calc(100% - 2em)` keeps tracking the page. Custom properties cascade and
inherit like any other; `var()` takes the winning value or its fallback,
and one that names itself, or names nothing, leaves its property unset.
Properties set on `:root` reach the whole chapter, though `html` opens
before its sheets load.

Sheets meet their conditions before any rule counts. `@import` loads up to
four sheets deep, a few per sheet, each once, imports first; one with a
`media` list or a `supports()` test that fails is skipped. `@supports`
asks the engine itself whether it reads a property or selector, so
fallbacks a book writes for older readers stand down when ours draws the
real thing. `@layer` blocks rank in the order first named, nested ones
within their parent, all below unlayered rules; important rules turn
that order round. `@container` reads the column. `initial`,
`inherit`, `unset` and `revert` reset what lower rules set.

Media queries see a page 650 pixels wide and 980 tall, a colour screen
with a fine pointer and no scripting. `prefers-color-scheme` always
answers `light`: the reader already turns the book's colours and black art
to each theme, and a book that also flips them under `dark` would come out
inverted twice, its ornaments vanishing on dark paper. Vendor `@media`
types, `print`, alternate sheets, and `link` or `style` elements whose
`media` doesn't match are not applied.

## Fonts the book ships

A book's fonts load however they are packed. WOFF tables are inflated
one by one; WOFF2 is a single Brotli stream whose `glyf`, `loca` and
`hmtx` tables may come transformed, and these are rebuilt point by point
into plain TrueType, outlines, composites, hinting programs and side
bearings as the original had them. A font collection packed as WOFF2 is
not read; its family falls back to the next one named.

Fonts listed in `META-INF/encryption.xml` are revealed before use. The
IDPF scheme XORs the first 1040 bytes with the SHA-1 of the unique
identifier, white space removed; the older Adobe scheme XORs the first
1024 with the bytes of a `urn:uuid:` identifier. Every identifier the
book declares is tried, the unique one first, and a key is kept only if
the result starts as a font. Any other encryption leaves the file alone.
Unpacked faces keep the per-face and per-book size caps.

## Type features and page breaks

`font-variant-numeric`, `font-variant-ligatures`, `font-variant-position`,
`font-kerning` and `font-feature-settings` become OpenType features of the
run, settings last, each group inheriting on its own; the `font-variant`
shorthand sets all three groups and the caps, and `font` resets them.
Features a face lacks do nothing, as in a browser. Small capitals are
drawn smaller where the face has none, now for `all-small-caps` too, with
petite caps read as small caps.

`text-align-last` places a paragraph's last line, and any line before a
forced break, in designed chapters. Paragraphs split between pages keep
the book's `widows` and `orphans` (up to eight lines; two by default).
`break-inside: avoid` keeps a block, or a box and all it holds, on one
page when it fits on one, and `break-after: avoid` keeps a block with the
next as headings are kept. These follow the book in designed chapters;
prose keeps the reader's two-line rule.

## Right to left

A block's direction comes from its `dir` attribute or the CSS `direction`
property, inherited, the sheet winning over the attribute. `auto`, or no
direction at all, reads the block's first strong letter: Hebrew, Arabic,
Syriac, Thaana and N'Ko read right to left, anything else left to right.
Lines then start at the right and mixed runs are ordered by the Unicode
bidirectional algorithm, so English words and numbers in an Arabic line
keep their own order. List and quote indents, quote bars and list markers
move to the right side. Drop caps, table columns and an explicit `left`
or `right` alignment keep their sides.

Pages turn right to left when the spine's `page-progression-direction` is
`rtl`, or, with none given, when the book's language is Arabic, Hebrew,
Persian, Urdu, Yiddish, Pashto, Sindhi, Uyghur or Dhivehi. A spread then
shows its first page on the right, and in single and spread views the left
arrow goes forward. Scrolling views are unchanged.

## Ruby and run fonts

The text of `<rt>` and `<rtc>` becomes a note over the base read since the
ruby began or since its last note, trimmed of space; `<rp>` fallbacks are
hidden. Notes are drawn at half the base's size, centred over the first
line part of their base, and a line holding one grows by 0.6 em to make
room. Notes keep to reading order, never overlap, and hold at most 256
bytes; they aren't part of the page's text, so search and selection see
only the base.

In designed chapters, a run whose `font-family` differs from its block's
is set in its own stack (up to 255 per block), and a font the book ships
loads when only such runs use it. The book's family on `code` now wins
over the reader's monospace.


## Vertical writing

`writing-mode: vertical-rl`, or its `-epub-` and `-webkit-` forms and the
older `tb-rl`, on the root or body turns a chapter: its lines run down the
page and follow one another from right to left. `vertical-lr` and the
`sideways` modes are read as `vertical-rl`. A chapter that doesn't ask
stays level, so a book may mix both; each run of vertical blocks gets
pages of its own.

In a vertical line, Chinese, Japanese and Korean letters, kana, Hangul and
full-width forms stand upright and take the font's vertical forms (`vert`),
so punctuation and brackets turn. Other letters lie on their side.
`text-orientation: upright` stands every letter, Latin ones in the
font's full widths (`fwid`) when it has them; `sideways` lays every
letter down. `text-combine-upright: all` (or the older `horizontal`)
sets its run, at most 256 bytes, across the column in half widths,
squeezed to one em: dates such as 10月21日. Ruby notes sit to the right
of their base. Pictures, inline or alone, stay upright, sized for the
column.

When the spine gives no `page-progression-direction`, a book with any
vertical chapter turns its pages right to left. Selection, search, links
and pictures follow the turned page: a drag down a column selects along
it, and columns are read from the right.

Margins aren't remapped to the turned page, `vertical-lr` columns aren't
read left to right, `text-orientation` applies to whole paragraphs, not
to inline runs, and `text-combine-upright: digits` combines the
whole run.

sha256 of `vertical.epub`: `18aadeea5e9ac0d666d5fddc2b5537cb4348ab481945b52bf72d5fd7472231f8`

## Fixed layout

A book whose package sets `rendition:layout` to `pre-paginated` is laid
out by the book, page by page. A spine item's
`rendition:layout-pre-paginated` or `rendition:layout-reflowable`
overrides that, so one book may mix fixed pages and reflowed chapters.

Each fixed item is one page at its viewport: the size its
`<meta name="viewport" content="width=…, height=…">` names, else its
body's width and height in px, else the reflowed page's size. A picture
in the spine, SVG or bitmap, is a page the picture's size. The page is
laid out in CSS px, an em being 16 px, with the book's own type: the
reader's font, size, spacing and margins don't apply. It's then scaled
to fit the reader's page and keeps its shape. The theme's paper and ink
still apply, as in reflowed chapters.

`position: absolute` and `fixed` take a box out of the flow and set it
at its `top`, `right`, `bottom` and `left`: px, percentages, or viewport
units of the page. Its `width` and `height` come from the book, or else
from the room its offsets leave. A box with no offsets sits at the corner
of its containing block. `position: relative` stays in the flow and is
the containing block for the boxes it holds. Positioned boxes paint over
the flow, in `z-index` order, ties in document order; a negative
`z-index` paints under the flow. The background of the root (else the
body) covers the whole page, under everything.

In the Two pages view, a first page marked `page-spread-right` (or
`page-spread-left` in a right-to-left book) shows alone, and the pages
after it pair. `rendition:spread` set to `none` shows every page alone.
Each page in a spread keeps its own size. Selection, search, links,
pictures and page AI work on fixed pages at their scale.

A positioned box with no width takes the room its offsets leave, not
its content's width. `position: relative` doesn't move the box itself.
Only the first page's `page-spread-*` is read, so
`rendition:page-spread-center`, `rendition:orientation` and the
landscape-only spreads aren't followed.

sha256 of `fxl.epub`: `85764517a37055bb1ffea996559fad292c7067a9f66a5cf4c4357ca056333361`

## MathML

Presentation MathML is laid out by the reader, without scripts, and
drawn as a picture. Its type is STIX Two Math (OFL), bundled as a subset
of the math ranges with its MATH table, whose constants set every shift,
gap and rule. Glyphs are drawn as outlines, so formulas stay sharp at any
zoom. A black formula is line art: it takes the theme's ink, as text does.
`mathcolor` keeps its colour.

A `math` element sits in its line, its baseline on the text's. With
`display="block"`, or `display: block` from the book's CSS, it's its own
block, centred and set in display style. Formulas show in table cells
too, where display math sits in the cell's line. A prefixed `m:math`
reads the same.

| Element | How it's set |
| --- | --- |
| `mi`, `mn`, `mo`, `mtext`, `ms`, `mspace` | One-letter `mi` in italic; `mathvariant` maps letters, Greek and digits to the math alphabets |
| `mo` | An operator dictionary: spacing by kind, a sign after an operator is unary, `form`, `lspace`, `rspace`, `stretchy`, `largeop`, `movablelimits`, `accent` |
| `mrow`, `mstyle`, `mpadded`, `mphantom`, `merror` | `displaystyle`, `scriptlevel`, `mathsize`, `mathcolor`; padding by lengths, percentages and `width`, `height`, `depth` |
| `mfrac` | `linethickness` (none for binomials), `numalign`, `denomalign`, `bevelled` |
| `msqrt`, `mroot` | The radical grows by size variants, then by parts; the index sits by the MATH kerns |
| `msub`, `msup`, `msubsup`, `mmultiscripts` | Scripts shrink a level and shift by the MATH rules, prescripts too |
| `munder`, `mover`, `munderover` | Limits above and below in display style, beside as scripts in a line; accents sit close; braces and arrows stretch across |
| `mtable`, `mtr`, `mtd` | Centred on the math axis; `columnalign`, `columnspacing`, `rowspacing`, `columnlines`, `rowlines`, `frame` |
| `menclose` | `box`, `roundedbox`, `circle`, sides, `actuarial`, `madruwb`, `longdiv`, `radical` and the strikes |
| `mfenced`, `semantics`, `maction`, `mstack` | Fences and separators; the first child that isn't an annotation; the selected child; rows right-aligned |

Fences in a row grow to what they hold. Large operators take their
display size in display style. A formula that nests deeper than 48
levels or holds over 4096 elements isn't set. Its `altimg` shows
instead, else its `alttext`, else its text.

`mlabeledtr` labels are left out. `mglyph` and `annotation-xml` aren't
drawn. Line breaking inside a formula isn't done: a long inline formula
stays in one piece. Copy and search skip formulas.

sha256 of `math.epub`: `a9b54ffb7b329a7352072e0379844b7b1a351122880b5b5260f385217f6cb488`

## Media and fallbacks

The reader has no player and runs no scripts. Sound and video show as
cards, and clicking one hands the clip to the system's player.

| Content | How it shows |
| --- | --- |
| `video` | Its `poster` under a play mark, else a drawn player frame; a caption names it by `title`, `aria-label` or file |
| `audio` | A drawn player strip and its caption |
| `source` | The first one that plays is the clip; else the first is named |
| `object`, `embed` | A picture shows as one; sound or video as a card; anything else shows the `object`'s own content |
| `iframe` | A link to the page it embeds |
| `epub:switch` | The first `epub:case` whose `required-namespace` the reader draws (MathML, SVG, XHTML), else `epub:default` |
| `noscript` | Shown, since scripts never run |
| `canvas` | Its content, drawn as the fallback |
| `picture`, `srcset` | The first `source` whose `media` applies and whose `type` the reader decodes; a `srcset`'s largest candidate |
| `a` around a picture | The whole picture is the link |
| `img usemap` | Each `area` of its `map` is a link over its share of the picture; circles and polygons by their bounds |

Drawn cards are line art, so they take the theme's ink. Text inside
`video` and `audio` is for readers that can't play them, so it stays
hidden. A clip at an `https` address opens in the browser. A card whose
clip the book lacks says so, and one whose type no player opens says it
can't play here; neither is a link. A picture
small enough to sit in a line of text isn't a link or a map itself; only
the linked text around it is.

A clip is copied to the system's temporary folder before it plays. Its
name comes from the type the manifest declares, never from the book: only
`audio/*` and `video/*` types with a known file extension are handed out.
Copies older than a day are cleared. Only books with clips keep their file
in memory to read them from.

Manifest fallback chains are followed, eight items deep at most. A
picture that won't decode shows its first fallback that does. A spine
item that isn't XHTML or a picture reads as its first fallback that is,
and links to the item land on it.

Media overlays read aloud in the reader (see Read-aloud below), and `track`
subtitles show as transcripts (see Subtitles below). `epub:trigger` works too
(see Triggers below), and scripted content runs (see Scripts below).

sha256 of `media.epub`: `1c735eea7efc902ce4e1231a3cef6cfb5d4f0c20a8d585fa16578ee5c82c64d8`

## Read-aloud

A book whose chapters name a `media-overlay` can be read aloud. Right-click
a page and choose **Read aloud from here**: reading starts at the phrase
under the pointer, or at the page's first phrase. The sound plays inside
the reader; clips in the text still open in the system's player.

| Overlay | How it reads |
| --- | --- |
| `par` | One phrase: the text its `text` names, read by its `audio` clip |
| `text src` | `file#id`, or `file#xpointer(id('id'))`; a phrase runs from where the element starts to where it ends. No fragment, or one the book lacks, reads the whole chapter |
| `audio` | `clipBegin` and `clipEnd` in full, partial or timecount form; no `clipBegin` starts at 0, no `clipEnd` plays to the file's end |
| `par` without `audio` | Skipped |
| `seq` | Nesting is followed; its `epub:type` applies to the pars inside |
| Skippable | Notes (`footnote`, `endnote`, `rearnote`, `note`, `noteref`) and page numbers (`pagebreak`) can each be left out |
| Escapable | In a `table`, `list`, `figure`, `aside` or `sidebar` (and their rows, cells and items), a button leaves the whole structure |
| `media:active-class` | Not applied: the phrase read is lit in the theme's accent, as voices are, so a book's colour can't hide the text on a tinted page |
| `media:narrator`, `media:duration` | Read; the whole book's length is kept |

The phrase read is lit on the page and pages turn to keep it in view.

A book with no overlay is read by a voice, a sentence at a time. In a
`table`, the same button skips the cells left, to the words after it;
in a list (`ul`, `ol`, `dl`, `role="list"`), a `figure` or an aside
(`aside`, `epub:type="sidebar"`, `role="complementary"`) it skips the
rest of that. Each kind can be read, asked about or always passed over;
a nested list goes with the list it is in.
Reading goes on across chapters to the book's end. Text inside a table
isn't lit, but the page still turns to it. The lit colour lies lightly
over the page, more faintly on dark themes, so the text stays readable.

The bar at the foot of the pane has the previous and next phrase, play
and pause, the way out of a table, the speed (0.75× to 2.5×, at the
narrator's pitch), and the options to skip notes or page numbers and to
turn pages. Its choices are kept with the settings: by default page
numbers are skipped and pages turn.

The sound is decoded by the reader itself: MP3, AAC and M4A, FLAC, Ogg
Vorbis and WAV, with no system codecs. Only a little of a file is decoded
ahead of what is heard, so a long chapter doesn't fill memory. Phrases
whose clips follow on in one file play as one, without gaps. Without a
sound output, or with a file that won't decode, the bar says so.

`media:playback-active-class`, which styles the whole document while it
plays, isn't applied. Overlays with more than 100,000 phrases are cut
there, and an overlay file is read up to 16 MB.

sha256 of `narration.epub`: `480889f7ab1f0ca0d752d2e24e74f93112f8ae737bdac3f51470ea420006b645`

## Subtitles

A `video` or `audio` element's `track` children of kind `subtitles` or
`captions` (or no kind) are read; `chapters`, `descriptions` and
`metadata` tracks are skipped, as are files the manifest doesn't list.
Eight tracks a clip are kept.

| Format | Read |
| --- | --- |
| WebVTT | Cue timings in both clock forms; voice tags name the speaker, other tags are dropped; `NOTE`, `STYLE` and `REGION` blocks are skipped |
| SubRip | Numbered cues with comma times; tags are dropped |
| TTML | Each `p` with `begin` and `end` or `dur`, clock values with frames or offsets; `br` keeps a space |

The format is told by what the file holds, not its name. Rolling captions
that repeat a line keep it once.

**Transcripts.** Under a clip's card, **Show transcript** lays the track's
lines into the page in its place: a **Hide transcript** link, the other
tracks as links that switch to them, then each line with its start time.
The pages after it move down, as page results do. The track shown is the
first that matches, by full tag then by language:

1. the language pages are translated into,
2. the language words are explained in,
3. the system's languages,
4. else the track marked `default`, else the first.

A clip missing from the book still shows its words. A clip in a table cell
or on a fixed page has no transcript link; its subtitles still go to the
player. Transcripts belong to the pane and close with the book. Where a
page result covers the same text, the one asked first is shown.

**Players.** Playing a clip also copies its tracks beside it in the
temporary folder, named after the copy as `<clip>.<language>.vtt` (or
`.srt`, `.ttml`), so players that look for subtitles next to a video find
them. The language is kept to letters, digits and hyphens.

A track file is read up to 4 MB and 4,000 lines; a line keeps 2,000
characters.

sha256 of `subtitles.epub`: `db6ee795ebaea483877a8d3ccdcb0e1b50a5a686caeb892a2bf8ee30b708142d`

## Triggers

`epub:trigger` (EPUB 3.0, deprecated since 3.3) makes a click on one
element act on another. A trigger is read from the chapter's `head` or
body, by any namespace prefix; `ev:observer` names the element clicked,
`ref` the one acted on, both in the same chapter. Only click-like events
count (`click`, `activate`, `DOMActivate`, mouse and touch presses);
hovering and the rest never come from a reader, so those triggers are
ignored. An observer's text and block pictures become a link, its
tooltip saying what it does; a link it already was is replaced.

| Action | What it does |
| --- | --- |
| `show`, `hide` | The element shows or hides, keeping its room as `visibility` does; the page is laid again |
| `play` | An `audio` clip plays in the reader from its start, pausing read-aloud; a `video` opens in the system player |
| `pause`, `resume` | The clip playing pauses or plays on; `resume` starts a clip not yet playing |
| `mute`, `unmute` | The clip plays on silently, or is heard again; it stays muted when played again |

Once a target shows or hides, its chapter is read again with the target
changed, and only the blocks that came out different are laid in place
of the book's own. An element the book hides with `display: none` takes
no room until shown, so the text after it moves down; that holds inside
a line, in a table (a row or cell left out until shown), on a fixed page
and for an element with no content, such as an `img`. Hiding keeps the
room, as `visibility: hidden` does: text, pictures, rules, tables, cards,
list markers and the element's own borders and backgrounds draw nothing
there, and its links lead nowhere. The book's own `visibility: hidden`
works the same way before any click. A run laid again grows to the
whole box, grid row or fixed page it cuts through, so boxes stay whole;
links in it lead where the book's own did.

A small picture set in a line is a link of its own now: an observer, or
a picture inside a link, can be clicked on its box. A hidden `audio` or
`video` that a trigger plays has no card until a trigger shows it. Only
one clip plays at a time, and it stops when its book closes or
read-aloud starts; pausing, muting and resuming a video do nothing, as
its player is outside the reader.

What is shown belongs to the pane and resets with the book. A chapter
keeps 65,536 triggers and 1,024 for one click; a book shows or hides as
many elements as it keeps ids. An element shown from `display: none`
lays out as a block, table or line by its tag, so a flex or grid
`display` it had is read as a block.

sha256 of `triggers.epub`: `2aa2ea9eaa31efb7a821dd4acaa4182cdbbbe7d073d43a4552d32b6f2dd49cda`

## Scripts

A chapter with `script` elements (no `type`, or a JavaScript one;
inline or a file in the book) runs them in the reader's own engine:
Boa, a JavaScript engine written in Rust, on a thread of its own, one
context per chapter. No web view is involved. Scripts see a small page
model built from the chapter, and nothing else: no network, files,
storage, workers or other chapters. `localStorage` keeps values only
until the book closes.

| Scripts can | How |
| --- | --- |
| Find elements | `getElementById`, `querySelector(All)` with CSS selectors (combinators, attributes, `:not`, `:is`, `:has`, `:nth-*`, `:checked`), `closest`, `matches`, the tree's parents, children and siblings |
| Change them | text, `innerHTML` and `outerHTML`, attributes, `classList`, `dataset`, inline `style`, creating, moving and removing nodes |
| Read styles | `getComputedStyle` for `display` and `visibility`, from the book's sheets, `style` elements, inline styles and `hidden` |
| Answer the reader | `click` listeners, `on…` properties and attributes, capture and bubbling, `preventDefault`; `change` and `input` on form controls |
| Wait | `setTimeout`, `setInterval`, `requestAnimationFrame`, promises |
| Speak | `alert` shows a message under the page; `confirm` says yes |

After each click, field change or timer that changed something, the
page is written back as markup and laid again the way triggers do:
only blocks that came out different replace the book's own, and
triggers still act on the scripted page. Before the first run the
chapter shows as written; then `noscript` goes.

What a script answers clicks on becomes a link: elements with a click
listener, labels, buttons and `summary`, and, under a parent that
handles its children's clicks, the likely targets (links, list items,
cells, pictures, and elements with an `id`, `class`, `role` or `data-`
attribute). Form controls are drawn as text: □ and ■ checkboxes, ◇ and
◆ radio buttons, a menu as its choice and ▼ (a click picks the next
one), buttons in bold, and text fields underlined with their value or
hint. Clicking a text field opens a small card to type in; Enter, Done
or a click away sends it, as leaving a field does in a browser, and
Escape drops it.

Each loop stops after 2,000,000 turns and calls nest 400 deep, and a
script that throws stops alone. A runtime that takes longer than four
seconds over one call is turned off for the book, with a message. The
page model has no layout, so sizes and positions read as zero; canvas,
media elements' methods, frames and network calls are absent, and
large frameworks won't run. The browser build shows scripted chapters
as written.

sha256 of `scripts.epub`: `3bdb9d8c054c336d1697221cfdc0f3d66347aca9bbcd96eb97d3c440f0c82ddf`
