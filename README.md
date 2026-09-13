# PDF Reader

A desktop document reader built for long-form reading. Native Tauri v2 shell, Rust/WebAssembly
interface written in Leptos, and Mozilla's pdf.js vendored locally as the PDF rendering engine.
Alongside PDFs, the reader opens plain text and Markdown files, which it reflows into pages with
reader-controlled typography.

The design goal is eye comfort over hours of reading: instead of a fixed set of themes, the
appearance system exposes a continuous colour space (base mode plus a computed tint) layered with
optional paper textures and film grain, all persisted between sessions.

---

## Table of contents

- [Highlights](#highlights)
- [Features](#features)
  - [Document viewing](#document-viewing)
  - [Text and Markdown formats](#text-and-markdown-formats)
  - [Zoom and fit](#zoom-and-fit)
  - [Motion](#motion)
  - [Navigation](#navigation)
  - [Search](#search)
  - [Appearance system](#appearance-system)
  - [Presets](#presets)
  - [Opening documents](#opening-documents)
  - [Library](#library)
  - [Interface](#interface)
  - [Persistence](#persistence)
  - [Accessibility and motion](#accessibility-and-motion)
  - [Performance and memory](#performance-and-memory)
- [Keyboard shortcuts](#keyboard-shortcuts)
- [Architecture](#architecture)
  - [Layer overview](#layer-overview)
  - [Project layout](#project-layout)
  - [Engine API](#engine-api)
  - [State model](#state-model)
- [Getting started](#getting-started)
  - [Prerequisites](#prerequisites)
  - [Installation](#installation)
  - [Development](#development)
  - [Building for release](#building-for-release)
  - [Tests](#tests)
- [Configuration](#configuration)
- [Technology stack](#technology-stack)
- [License](#license)

---

## Highlights

- Three document formats: PDF (through pdf.js), plus plain text and Markdown, which the reader
  paginates and renders itself with full typography control.
- Four view modes: single page, two-page spread, and virtualized continuous scroll in
  vertical and horizontal orientations, with a unified zoom pipeline across all of them.
- A Fonts settings tab for the text formats: default and per-family font pickers, size, weight,
  line spacing, paragraph margin, word/letter spacing, text indent, book layout, full
  justification and hyphenation.
- Continuous appearance model with three base modes plus a hue and strength tint, replacing
  hard-coded themes.
- Five paper textures with adjustable opacity and scale, plus static or animated film grain.
- Five built-in presets, and unlimited user-saved ones organised into named groups.
- Full-text search with per-page results, highlight rectangles and wrap-around navigation.
- Document outline (table of contents) with automatic highlighting of the active section.
- Virtualized thumbnail grid backed by an LRU bitmap cache.
- Clickable internal links that navigate the reader, and external links that open in the browser.
- Word glossing: select a word (or long-press a saved mark) for a dictionary-style card —
  Apple Intelligence on Apple Silicon, a deterministic mock everywhere else.
- Native file dialog, drag-and-drop opening, and restoration of the last-opened document.
- Settings persisted to local storage with a migration path across schema changes.
- Roughly 460 Rust unit tests across the workspace, plus a stub-vm smoke suite for the
  TypeScript layer, and five scripts that keep facts written down twice from drifting.

---

## Features

### Document viewing

**Single page view.** One page at a time, centred in the viewport, with a page-turn animation
whose direction follows the navigation direction.

**Continuous scroll view.** A vertically scrolling column of pages with a reading-progress bar.
The list is virtualized: only pages near the viewport are mounted, so a large document does not
allocate a canvas per page. Page height bookkeeping is computed from the intrinsic page sizes
returned when the document opens, which keeps the scrollbar honest even for pages that have never
been rendered.

**Mixed page sizes.** Fit calculations read the size of the page currently on screen rather than
assuming every page matches page one, so a landscape plate inside an otherwise portrait book is
not cropped.

**Selectable text.** A pdf.js text layer is overlaid on each page canvas. The layer is built
detached and swapped in atomically once complete, so a superseded render cannot append overlapping
spans. Selection paints a translucent tint over transparent text, so the canvas glyphs read
through and text is never doubled.

### Text and Markdown formats

Plain text (`.txt`) and Markdown (`.md`) files open through the reader's own pipeline instead of
pdf.js: the file is parsed into blocks, the blocks are packed into A4 pages, and the pages render
as real text in the DOM.

- **Every view mode works.** Single and spread show pages (spread under the book layout reads as
  a facing pair); the horizontal strip scrolls page by page; the vertical mode streams the text as
  one continuous column with no visible page breaks.
- **Typography you control.** The Fonts settings tab (shown only while a text document is open)
  offers a default font plus serif/sans/monospace family pickers, font size and weight, line
  spacing, paragraph margin, word and letter spacing, first-line indent, a book layout with
  spine-side gutters, full justification, hyphenation, and where the reading column sits while
  streaming (left, centre or right). The Appearance menu adds a body-ink intensity slider for
  text documents — a comfort dial that softens the ink toward the paper without filtering
  anything. Font pickers list the system faces today; the schema already reserves room for fonts
  bundled with the app.
- **Markdown is rendered, not displayed.** Headings, lists, tables, blockquotes, links and fenced
  code (GFM included; raw HTML is refused) render block by block, so a heading never gets split
  across a page break. An image is the one construct that does not arrive: the app's
  content-security policy admits no network origin and the asset protocol serves documents only,
  so a Markdown image displays when — and only when — its source is a `data:` URI.
- **Pagination follows the type.** Every block's rendered height is reported back into the shared
  model as it is drawn — estimates seed the cut at open, measurements refine it block by block —
  so what you see is paginated by the real fonts, and the pages re-cut whenever a typography knob
  moves, keeping your place on the block you were reading. Zoom never re-paginates: pages scale
  uniformly, which provably preserves the cut.
- **Search and theming carry over.** Full-text search scans the document in-process (no engine
  round-trip) and paints its hits over the type with the same boxes a PDF gets — one per line
  fragment, the current match in the same amber, stepping to a result scrolling to the block it
  sits in. Base mode, tint, textures and grain apply to text pages exactly as they do to PDF
  pages. Blend mode stays a PDF feature — a text page is recoloured by its tokens directly.

### Zoom and fit

- Fifteen zoom presets from 25 percent to 500 percent, clamped to that range.
- Zoom in and out step through the presets; repeated presses chain from the in-flight target, so
  a fast double press advances two steps rather than being swallowed.
- Fit width and fit page, recomputed on window resize and sidebar toggle.
- Turning to a page of a different size never touches the zoom while Settings → Layout → Auto
  Resize is off: the scale, the scroll position and the measured layout all stay exactly where
  the reader left them, and a page too wide for the window overflows and scrolls instead. Turn
  the switch on and each page re-fits itself to the window as it comes into view.
- Shrink to fit: when the window is too narrow for the chosen zoom, the page shrinks to the space
  available and remembers the zoom to grow back to once space returns.
- Every zoom — a button step, a preset, a fit, a window constraint — runs through one transition
  pipeline. The layout is what animates: the virtualized strips are rescaled every frame and the
  engine holds the document point under the middle of your window exactly where it was, working
  gap-aware because page heights scale and the space between pages does not. Pages stretch the
  bitmap they already hold while the scale moves, the crisp re-render happens once at the settled
  scale, and recently evicted pages linger a moment as zombies so the surface never pops.
- The sidebar slides its width over 300ms and the page rides the slide: every frame of it reports
  a new container width, and the layout follows each one, so the column narrows with the rail
  instead of snapping when a debounced refit finally lands. The crisp re-render is what waits — it
  is issued once the width has been quiet — and the page host never lets flex-shrink resize it, so
  there is no frame of squished paper either way.
- The same follow covers a window drag, with or without a fit: a hand-picked zoom shrinks to stay
  out of the way as the window narrows and grows back to exactly the zoom you chose when the room
  comes back, because the ceiling is computed from the remembered zoom rather than from the last
  frame's scale.
- Zooming a horizontal strip keeps its vertical position: the strip is always at least as tall as
  the tallest page, so the overflow — and with it the scroll position — shrinks and grows with the
  zoom instead of resetting.
- Zooming in at 500 percent, or out at 25 percent, does nothing at all rather than wrapping to the
  other end of the preset ladder — and it leaves the active fit mode alone, so leaning on a button
  that has nothing left to do cannot quietly take you out of fit width.
- The reader keeps written motion principles (see ARCHITECTURE.md): no entrance animations on
  document content, no per-frame virtualizer work, one bounded zombie bridge across commits.

### Motion

Every motion the reader *interpolates* is a switch, and the master for all of them sits in
Settings → Layout. What a switch turns off is the interpolation, never the change: the end frame
still arrives, in the frame the change is asked for. That is what makes freezing the reader safe —
a disabled animation loses nothing, it just skips the frames in between. The converse also holds,
and is why some resizes have no switch: a motion that is the only way to stay correct is not
decorative, so it is not offered.

- **Animations** (the master, in the Layout tab) collapses everything, including the
  micro-transitions that have no switch of their own: menu pops, toasts, the theme cross-fade,
  hover fades. One surface is exempt, because it is the exception to the sentence above: the
  loading mark's motion is the whole message, and a frozen spinner cannot be told apart from a
  hung app — so under either net it trades its travel for a fade and keeps going.
  It is the reader's own `prefers-reduced-motion`, and while it is off the Animations tab is not
  shown at all, because there is nothing left there for it to offer. The other
  switches keep their saved values through it, so turning the master back on returns exactly the
  set of motions you had chosen.
- **Sidebar slide** — the rail tweens its width over 300ms. Off, it appears at its new width, and
  the panel it was holding open is released in the same frame instead of waiting out a slide. The
  page follows the rail either way, and that is what keeps a frozen slide to one step.
- **Canvas follows the window** — the page re-fits on every frame of a window drag. Off, it takes
  the new space in one step once the drag has stopped, and a page too wide for the window overflows
  and scrolls for as long as it is being shown. A rail slide is deliberately not gated here: the
  page has to follow the rail, because the alternative is a page sitting outside the space it was
  given. Dragging a window is different — one relayout per drag frame is a real cost, and nothing
  is wrong in the meantime — so it is the burst that is offered up.
- **Zoom in / out** — a zoom eases to its target over the profile's duration. Off, every zoom
  (button step, preset, fit, window constraint) lands on the first frame.
- **Scroll to page** — a jump to a page, a search hit or a keyboard page turn glides the column
  over. Off, it lands there. The continuous scroll under a *held* arrow is not gated: that is the
  scrolling itself, not a decoration on top of it.

### Navigation

- Page counter with direct page entry.
- Previous and next page controls.
- **Outline panel.** The document's table of contents, flattened in document order with depth
  preserved. The entry containing the current page is highlighted; where several entries share a
  page, the deeper one wins. Pages before the first entry highlight nothing, since a cover belongs
  to no section.
- **Thumbnails panel.** A virtualized grid that tracks the reader's current page and auto-centres
  on it, with the glide debounced so a rapid scroll does not chase every intermediate page.
- **Internal links.** Link annotations inside the document dispatch a navigation event that moves
  the reader to the target page through the same path the outline and thumbnails use.
- **External links.** Rendered as ordinary anchors and opened by the system browser. The
  `javascript:` and `file:` schemes are refused.

### Search

- Full-text index built across the document on demand. Text and Markdown documents are scanned
  in-process — the blocks are already Rust strings, so the document is its own index and there is
  no engine round-trip.
- Results grouped by page with a surrounding text snippet for context.
- Match rectangles are returned in scale-independent coordinates and multiplied by the current
  scale, so highlights stay aligned at any zoom.
- Forward and backward navigation through results with wrap-around.
- A floating search overlay with a results dropdown, dismissible with Escape.

### Appearance system

Rather than a fixed theme list, appearance is composed from independent axes.

**Base mode** determines the structural family:

| Mode  | Canvas treatment | Texture blend | Intended use |
|-------|------------------|---------------|--------------|
| Light | Untouched        | Multiply      | Daylight reading |
| Dark  | Inverted         | Screen        | Dark rooms, text documents |
| Dim   | Dimmed, not inverted | Soft light | Preserves real colours in figures, photos and syntax highlighting |

**Colour tint** is a hue from 0 to 360 degrees and a strength from 0 to 100, applied by the same
maths regardless of base. A strength of zero short-circuits the colour pipeline entirely, so a
plain base is byte-identical to having no tint feature at all.

Tint hue is specified in sRGB, because the page tint is applied with a hue rotation that operates
in sRGB, but the interface tokens are emitted in OKLCH. The two hue circles are rotated relative
to one another by a non-constant amount, so the value is converted rather than reused; this is
what keeps the page and the surrounding interface in the same colour family at every setting.

**Textures** overlay the page with a repeating pattern: none, paper, lined, grid, dotted or cross.
Opacity is adjustable from 0 to 100 and scale from 25 to 400 percent of the natural pitch.
Textures are anchored to the page rather than the viewport, so they track the page during zoom
instead of sliding across it.

**Film grain** can be off, static or animated, with intensity from 0 to 100.

### Presets

Five presets ship with the application, in one group:

- **Classic:** Sepia, Green, Night, Parchment, Cinema, each a specific point in the appearance
  space.

Sepia, Green and Night were previously hard-coded themes and are now reproduced exactly as tints,
which is what allows the appearance model to be the only mechanism. The plain bases (Light, Dark,
Dim) are deliberately not presets: the Mode & colour section's buttons are the one home for that
choice, so the gallery holds only looks the buttons cannot express.

Users can save the current appearance as a named preset and organise presets into named groups.
As soon as any slider is nudged, the active preset selection clears and the menu reports "Custom"
rather than claiming an unmodified preset is in effect.

### Opening documents

- Native file dialog through the Tauri dialog plugin, admitting PDFs, plain text (`.txt`,
  `.text`) and Markdown (`.md`, `.markdown`, `.mdown`).
- Drag and drop, handled through the Tauri drag-drop event with a DOM fallback.
- OS file associations for the same formats, so double-clicking or "Open with" hands the file
  to the reader.
- The last opened document is remembered and restored on the next launch.
- Five sample PDFs ship in `public/samples`, covering deep outlines, internal links and awkward
  title metadata. Nothing in the app lists them: they are opened by path (`samples/Outlined
  Book.pdf`, which the engine's loader resolves as a served URL) when a bug needs a specific
  document shape to reproduce against.

**Filename display.** A PDF's `/Title` metadata is free-form and frequently contains junk written
decades ago by the producing tool, such as `file:///F|/Mis%20docum` from dvips or
`Microsoft Word - Chapter3.doc` from Word. The title is used only when it actually looks like a
title; anything path-shaped, URL-shaped, percent-encoded, extension-bearing or a known placeholder
is rejected in favour of the human-readable stem of the real file path. Path parsing splits on both
separators, so a Windows path reaching a macOS or Linux build is not treated as one long filename.

The toolbar title measures the live geometry of the surrounding controls and truncates only on a
genuine collision, adapting as the window resizes. Only widths are measured, never positions, so
there is no feedback loop between the label and the layout.

### Library

The `/` route is a library, not a list of the last twenty things you opened. A book is an
**address** — a folder you imported read in place keeps its books at the paths they stand at, and a
file you imported on its own is a copy the library stores under its own roof. No in-app move ever
touches a file you own.

- **Two ways to hold a book.** *Read in place* is a folder import's default and the only mode the
  app ever had — the book is the path it was opened from, and the app never moves, renames or
  deletes it. *Copy into the store* puts the bytes under the app's own data directory, so a book
  survives its source folder being renamed or deleted, and keeps the source path as provenance for
  a relink. Each stored book owns one folder under the id that never changes, so no name that can
  be renamed is ever on disk and two books called `report.pdf` cannot collide — and removing a
  stored book takes its folder with it, so a book that is gone leaves no directory behind. A file imported on its own — picked from the dialog or dropped on the library — is
  always a store copy: there is no folder behind it to rescan or answer for it, and a copy is a
  book the library owns outright, with its own highlights and its own place in it.
- **Import a folder.** The sheet asks five questions and nothing else: which formats (as an include
  or an exclude list), how large a file has to be to count as a book, whether to read in place or
  copy, whether to watch for new books, and whether subfolders become shelves. Every answer is
  stored with the folder, so a later rescan honours the import it came from. The watch answer
  belongs to the folder being imported: when it is part of a tree the library already reads, the
  switch opens on the state that tree is in and answers for this subfolder alone — the tree above
  keeps watching its own — and what the switch shows is always the value that lands.
- **Watched folders rescan when the app opens or returns to the foreground**, not through a
  filesystem watcher — a watcher fires while you are still copying files in, which is exactly when a
  half-written PDF is most likely to be measured. Only genuinely new files are ever added: a book
  you dragged to another shelf keeps its fingerprint in that folder's ledger, and a book you removed
  leaves a tombstone, so neither comes back on the next pass. Watching is a per-subfolder decision:
  a folder shelf's right-click turns its own rung — the whole tree from the root shelf, and only
  that subfolder from a rung of it, with the rest of the tree keeping its own answer — and a
  rescan adds files only under the rungs that are watched. The root's row is the whole folder's
  decision: turning it back on brings every subfolder on with it, and turning it off leaves none
  secretly watching.
- **A shelf is a level, not a row.** In the grid a shelf is a folder card — one cell of the same
  grid the books are cells of, wearing a 2×2 plate of what is inside it (covers for its books,
  plates of their own for its folders, recursively) and a count of both halves — which is what
  lets a shelf be filed inside another shelf. The dense list draws the same level as a tree: a
  shelf row unfolds in place, as deep as the forest goes, while its Open drills the breadcrumb
  route, because unfolding is a way of looking and must not move you. A hold on a row enters the
  selection and a movement lifts it — books and folders alike at both densities. Filing a book is a drag from
  a card to a folder, to a crumb or to another card, and the only thing a drop edits is an
  ordered list of ids.
- **One drag, and it is the app's own.** Nothing on the shelf rides the browser's drag-and-drop: a
  press that moves becomes a session that knows what it is holding, where the pointer is and which
  target is under it, so a drag can carry a whole selection at once with a ghost of their covers,
  can be dropped on a breadcrumb crumb to file onto a level you are not looking at, and can be
  folded into a new shelf by resting over a book for a moment — which is a gesture the browser's
  drag cannot report, because it never says how long a hover lasted. Escape puts everything back,
  and a card that fades while held always fades back.
- **Resting on a crumb says so.** A crumb is the one place on the page smaller than the ghost
  hovering it, so it is the one place the ghost shrinks: hold a drag still over a breadcrumb crumb for
  420ms and it sits at a third of its size on the crumb's centre, leaving the name of the level you
  are about to file onto readable instead of covered. Nothing on the shelf itself sinks — a folder
  card already wears the loudest marker the shelf has, and a book is a position rather than a
  container, so it answers at once with the insertion line and, after 650ms of resting, with the
  folder card's own plate filling in — one cell per item the new shelf would hold, so a single book
  rested on another is a shelf of the two. A book you are already carrying is never a partner:
  dragging onto yourself, or onto one of your own selected cards, reorders instead of counting that
  book twice. While the ghost is parked a move costs one comparison against a cached box, and the
  transition that glides it in is the parked state itself, so the follow resumes exactly under the
  hand on the first move out.
- **The view is a set of knobs, not a set of modes.** Grid or list, Auto or two to ten columns,
  covers fitted to their own aspect ratio or cropped to A4, and a sort by manual order, title,
  author, date added or last read. Titles sort with their volume numbers as numbers, so "Volume 2"
  comes before "Volume 10".
- **The search bar answers while you type.** The shelf behind it filters on every keystroke — by
  name, author or address, in any combination of words, case never mattering — and under the pill a
  panel offers up to seven books the query is probably about, strongest match first, with the
  characters that matched lit in the accent. The matching is fuzzy in the forgiving direction: a
  dropped vowel or a half-remembered spelling still finds the book (`mthmtcl prfs` finds
  *Mathematical Proofs*), but a query whose letters only sprinkle across a long title is not a
  match — a search that matched everything would be a shuffle, not a search. `Up arrow` and
  `Down arrow` move through the panel, `Enter` goes to the chosen book on its own shelf and lights
  it up, and `Escape` peels one layer at a time — the suggestions first, the text second. Nothing
  is indexed: the whole answer is one pass over the library per keystroke, computed when you type
  and not a moment else.
- **A missing book stays on the shelf.** An address that stops resolving is a badge and a Relink
  affordance, never a silent removal: the row keeps its resume point and every shelf it is on, so
  finding the file again puts you back on the page you were on. It greys where the library shows
  it — its card, its list row, and the plate of any folder that previews it.
- **Shelves you make yourself.** The view menu's *New shelf* row creates one and drills into it.
  Taking a shelf apart is the selection bar's receipt: select the folder and *Remove* itemises what
  survives it — every book stays in the library, the shelves inside it move up a level, and a shelf
  cut from a watched folder says the folder keeps watching.
  The crumb of the shelf you are on carries its own popover: *Rename…* replaces the label with a
  field in place — Enter commits, Escape cancels — and *Remove shelf* is the same receipt the
  selection bar gives, with a note when the shelf was cut from a watched folder.
- **The breadcrumb keeps three crumbs and elides the rest behind `…`.** A chain has no end and a title
  bar does. The ellipsis is its own affordance rather than an arrow on a crumb, because an arrow on the
  third level whose panel lists the first and second reads as "deeper than three" when those levels are
  shallower. Hovering it opens the elided levels drawn the way the bar draws them — `2 > 3 > 4 >`
  wrapping to `5 > 6` — in a rectangle whose width the window sets, so a resized window re-wraps the
  chain with nothing measured and nothing listening. ArrowDown opens it too, since those levels are on
  no other surface. Every crumb is a drop target, which is what makes a deep level reachable with a
  hand full of books; the ellipsis itself is a place to rest and not a place to drop.
- **A right-click is a menu, per kind of thing.** A book gets Open, Select, Rename…, Duplicate,
  Reveal in folder, Find again when its address died, and Remove; a folder gets Open, Select,
  Rename…, Duplicate, Reveal in folder, the watch toggle for the seat its shelf stands on — the
  whole tree from the root shelf, only that subfolder from a rung of it — a New shelf filed inside
  the one you asked whichever level the page is on, and Take shelf apart; a card already in a selection
  gets the set's menu — New shelf from these, Duplicate, Remove, Clear — because a right-click on
  one of several things means all of them; and the empty shelf gets New shelf and Select all.
  **Reveal in folder** opens the OS file manager on the thing itself: a book the library copied
  reveals the copy in the library's store, a book read at its place reveals the file where it
  stands, a link reveals what it points at, and a folder shelf reveals the directory its tree cut
  it from — with no row at all for a shelf the reader owns, which has no directory to show.
  **Rename…** opens one field, seeded with the name the thing already shows, and commits on Enter.
  It renames what the library SHOWS and nothing on disk: the file a book reads keeps the name it
  has, which is what makes the row safe to rename at all — its id, its address, its resume point,
  its highlights and every shelf it is filed on all stay put, and only the label changes. A link
  renames its own name, and a shelf renames the shelf, both the same gesture from the same menu.
  A name you type is yours and survives as-is: a title that looks like a filename is dropped when
  it came from a document's own metadata, which is where such titles are download debris, but not
  when a reader typed it at this sheet.
  **Duplicate** makes a second instance of the thing under the pointer: a book read at its place
  gets a second FILE beside the first, in the file manager's counter name (`dune_1.pdf`, and a
  duplicate of a duplicate steps rather than stacks); a book the library copied gets a second copy
  in the store; a link duplicates as a link. Either way the duplicate is its own row — its own
  name on the level's counter, its own resume point and highlights — filed right behind the row
  you pointed at, on every shelf the original is on. A SHELF duplicates as a shelf: a second one
  of your own holding the same books, with the whole tree inside it copied along, in the level's
  counter name and right behind the shelf it came from. It costs no disk, because a shelf holds
  membership and never held a byte — and a folder's shelf copies as one of yours rather than as a
  second shelf of the folder, because one directory is one linked shelf. No row carries a second line explaining itself:
  a right-click is a reader who knows what the rows mean. One host answers all four, so the menu
  is the same menu wherever it was asked
  from, and a right-click never starts a drag: a menu row is clicked by a pointer that has already
  been released, and a session begun from one would have no release to end it.
- **Removing a book shows you the receipt first.** The sheet itemises what goes with it — the resume
  point, the highlights, the cached cover, every shelf it was filed on — and leaves out the rows for
  things the book does not have. For a book the app copied there is a switch for the copy, on by
  default; the file it was copied from is never touched either way. There is no undo toast, because
  the sheet is the safety and the folder's import menu is the undo.
- **Removing a shelf can take everything inside it with it.** Off — the default — the books inside stay
  in the library and the shelves inside move up a level. On, the books are purged by the same receipt a
  selected book gets and the shelves inside are taken apart too, deepest first. The switch is offered
  only when there is something inside to decide about, and every row on the sheet, the store-copy
  switch and the confirm button's own wording all describe the set the removal will actually take
  rather than the one you clicked — so a cascade that reaches stored copies is a cascade that shows
  you the copies first.
- **Moving a read-at-place book out of its folder makes it the library's own.** The departure is
  the one move that writes a byte: the book becomes a stored copy — its name, its place in it and
  its highlights all travel with it — and the folder records a moved-out log, so no rescan files
  the OS copy back and no menu offers it as a book that is gone. Importing that file again brings
  the linked book back beside the copy that left, and lights it up: two books of one content, each
  with one address. Dragging the stored copy back onto a shelf of the folder it left binds the log
  to it by name, and from then on importing the file just lights that copy up — unless you renamed
  it, in which case the folder does not recognise it and the import brings the linked book back
  instead. What ties the book to its folder is the rung its own file stands on, not the folder's
  shelves, so a drag between two rungs of one watched folder departs as well; only a re-order on the
  rung the book is already on, and the move of any book the library already stores, copies nothing
  and stays the membership edit a drag has always been.
- **Moving a shelf a folder reads in place asks first: a copy, or its place back.** The shelf IS
  the OS directory — the way a linked book is the OS file — so a hand taking it off the seat the
  folder's tree names is a departure, and the one move that asks before it runs: the sheet says
  which folder reads the shelf, how many books the copy costs and the name the copy will wear at
  the level it lands on. Confirm and the shelf and every read-at-place book standing on it become
  the library's own copies — bytes into the store, names, highlights and places in them
  travelling along — and the copy takes the level's next free name, which keeps the folder's own
  name free for the original. The copy is a book and a shelf of their own, bound to nothing: no
  rescan, no import and no light ever answers through it, and importing the folder again brings
  the original back on the seats the disk names, in the old names, lit inside its family. Cancel
  leaves the shelf where the tree put it.
- **A move inside the shelf's own family never has to cost a copy.** Dropping a read-at-place
  shelf on a rung of a tree its directory belongs to — the tree it was cut from, or the family
  that covers a folder you imported on its own — offers a third answer beside the copy and the
  cancel: **Put it back in its place**. A shelf you imported on its own folds back into the
  family tree, on the rung its directory names, with the folder that was reading it folded into
  the tree's ledger; a shelf an older build left off its seat simply reseats, and the disk owns
  the place again. Nothing is copied either way, the files are never touched, and the light
  lands on the shelf where it went home. A re-order among its own siblings copies nothing, and a
  shelf of a folder the library COPIES moves freely, the way a stored book does.
- **A watched folder can give a book back.** Its import menu keeps a tombstone per removal — the name
  the shelf showed, how long ago it went, how big it is — and offers the book back after measuring the
  file to check it is still there. An explicit restore ignores the folder's format and size filters,
  because an explicit ask is an explicit choice. So does an explicit re-import of the same file, from
  whichever side it comes: the log is spent, the book returns wearing the name the shelf showed, and
  it is lit up where it lands — and until that ask, every rescan stays silent about the file, which
  is what the log is for. The same menu answers the other question without
  walking anything: a book still inside the folder on disk but no longer on any shelf the folder owns
  is offered as a move, with two answers — file it here as well, which is a second membership and no
  second copy, or go and look at where it went, which closes the menu, moves the breadcrumb and lights
  up the card it scrolled to.
- **A book whose file moved or vanished is a question, not an error.** Every focus re-measures the
  addresses the library holds before it walks the watched folders, so a shelf notices a file that
  disappeared instead of finding out when you click it. A book whose address no longer resolves
  stays on the shelf — greyed, with its place in it and its highlights — and opening it raises the
  Find-again sheet: pick the file it is now, or name a folder and the app walks inside it looking
  for the book's own name. A match is re-measured and healed, a miss says so in one sentence, and
  Cancel changes nothing.
- **An import reports from the corner.** One card per run in the bottom-left, with a ring that spins
  while the folder is walked and fills while files are copied. It stays put when you open a book,
  because the run outlives the page that started it.
- **Dropping files on the library files them**; dropping them anywhere else still opens one. Both go
  through the same admission, which is the format registry.
- **A shelf that already holds the name asks.** Importing a file, dragging a book or filing one onto
  a level that already holds a book OF THAT NAME — the same name, not the same bytes; a second
  format of one title is a second book and is simply placed — used to do nothing at all, which read
  as the book vanishing into the shelf. Now it asks, and what it asks depends on which side of the
  question has a book of its own.
- **Importing a file onto a shelf that has the name asks what to put there**, with the three answers
  a reader can mean: **Already imported** adds nothing and takes you to the book you have, on the
  shelf it is filed on, with its card lit up; **Add as new** keeps both and names the arrival `1_1`,
  `1_2` and so on, the counter a file manager appends, counted against that shelf's own names and
  promised on the row before you click; **Make link** puts a pointer on the shelf instead of a copy.
  A file has no book of its own yet, so none of these answers can cost you one. Home is a level like
  any other and asks the same question of the books nobody has filed. **Add as new** lands a copy
  the library stores and owns — a book of its own bytes with its own highlights and its own place
  in it, even when it is the very file the library already reads in place — so no row is ever
  withheld: nothing a file's answers can do is a second door on one linked file.
- **Dragging a book onto a shelf that has the name asks which book the shelf keeps.** Both sides are
  books you already have, so the three answers are the three things a reader can mean about them:
  **Merge** folds the one you dragged into the one that was there — the further place in it wins, so
  a merge never sends you backwards, the page count is the best either knew, names fill gaps rather
  than overwrite, and the moved book's other shelves and its highlights join the survivor instead of
  leaving with it, the shelf it was dragged OFF excepted, because that departure is the move you
  made and a merge that put the survivor back there would be a move that never happened; **Replace** sends the book that was there out of the library and seats yours in
  its place, on every shelf it was filed on, and the row says how many highlights leave with it
  before you click; **As new** keeps both, yours under the next free name beside it. The two sheets
  are two different types in the code, so a file can never be offered a replace — and a move is
  offered a link in exactly one shape: when the book you dragged reads a file at its place and the
  book on the level is the library's own stored copy, **Make link** takes Replace's slot, because
  neither side is yours to destroy. The dragged book becomes a pointer, the file stays on disk and
  the copy stays in the store.
- **Re-importing a folder the library reads in place continues it.** The walk runs, as the
  explicit import it is: files the folder gained join the shelf as linked books, books you had
  removed or moved away — whose logs have been keeping rescans quiet — come back in their old
  names, and the books that were already there keep every bit of their data, because nothing
  about them is a question. A book the folder still holds but its shelf no longer does comes back
  too, which is the merge half of a re-import and the half no ledger table can answer: a book you
  filed onto a shelf of your own, and a book whose shelf you took apart, are each a file the walk
  finds and the library holds, so each is a *nothing to do* to the rescan's table. The re-import
  files them back onto the rung their own directory names — as a second membership and not a move,
  so the shelf you carried a book to keeps it. What a FOLDER import lights is the FOLDER: you stay on the level
  that holds it and its card lights up — only a file import goes to a book and lights that.
  Only a walk that found nothing new says so: the *already imported* note, which lights the
  shelf when you close it. Picking a SUBFOLDER of a tree the library reads in place is the same
  reconciliation, not a sentence that skips it: the tree's walk runs on the tree's ledger — a
  linked folder cannot mint a second instance of itself, or of a rung inside its own tree — so
  a book you removed inside the nested folder comes back on a re-import of the nested folder
  exactly as on a re-import of the root, and the light lands on the rung you picked. A shelf of
  the library's own copies can mint another of anything, so a stored folder still asks the name
  question.
- **A folder you import back goes home to its family.** Picking a folder whose directory stands
  inside a tree the library reads in place, when the tree no longer holds a shelf for it — you
  removed the rung, or it left as a copy — imports it back INTO that tree: the walk runs on the
  folder's own ledger, so the books that left come back in their old names, and the shelf lands
  on the rung its directory names, inside the nesting, lit where it stands. The same happens at
  the end of any import of the outer tree that finds one of its folders standing outside it: an
  import is you asking for the folder, and a member outside its family is an ask answered — the
  note that rises is the report of the shelf that went home, not a question about it.
- **A copies import is the library's own business, and asks only the level's name question.**
  Turning read-at-place off means *the library should own these books* — a second instance of
  its own, unrelated to any tree — so the import checks one thing: the name at the level it
  lands on. Nothing holds it and the copies simply import. Something does, and the sheet offers
  the level's own three: **Show it** imports nothing and lights the shelf that is here where it
  stands; **Replace** sends the books that shelf holds out of the library — the row says how
  many before you click, highlights and all — and seats the arriving copies on it, which of a
  folder's own read-at-place tree is the log-spending sweep, so the copies come back in the
  names the shelves showed; **Add as new** mints the counter-named second shelf, and of ground
  the library already reads in place its books are copies of their own — independent books of
  their own bytes beside the linked ones the old tree keeps reading. A copies import of a
  subfolder of a read-at-place tree is no different: it is not related to the tree, only the
  level's names can ask, and the ground the tree still reads is no obstacle — every file the
  library reads in place becomes a book of its own on the copies shelf, an independent copy of
  its own bytes beside the linked book the tree keeps, never a silent "Imported 0 books". The tree
  itself is untouched by a copies run — still linked, still watched, its ledger its own — because a
  copies answer asks for a second instance, not a conversion; only **Replace** of the tree's own
  shelf converts.
- **Importing a FOLDER whose name the level already holds asks its own question**, before the walk
  rather than after it, and which answers it asks is the arrival's mode: a copies import gets the
  three above — **Show it**, **Replace**, **Add as new** (`Books_1`, promised on the row) — and a
  read-at-place arrival of a DIFFERENT folder's name gets two, since as new of a linked folder is
  the second instance the family rule exists to prevent: **Make link** imports nothing and leaves
  a pointer that lights the folder where it is when you tap it; **Merge into it** files the
  folder's books onto the shelf that is here, and a book whose name a shelf already holds is
  asked one by one on a compact sheet — Merge, Replace or As new — at every level of the tree
  that already stands, not only the top, with an apply-to-all switch for a reader who has seen
  enough of the folder to answer for the rest. As new is withheld when the arriving file is the
  very file the shelf's book reads: two rows of one linked file are a duplicate, and the library
  does not make those. Books nothing collides with simply go in.
- **Importing a file puts a book where you dropped it.** A file the library already holds somewhere
  else is not a reason for a shelf to stay empty: importing `notes.md` into a second folder gives
  that folder its own book — a copy of its own, with its own highlights and its own place in it —
  and the first folder keeps the one it had. The same file imported twice onto ONE shelf is the
  collision above, and asks.
- **Importing a file a read-at-place folder already holds asks before it copies.** The file stands
  inside a folder the library reads where it is, and that folder's book for it is alive and
  standing: a second linked row of one read-at-place file is the one duplicate the library never
  makes, so the import asks — *import a copy here*, which stores the file as a book of its own on
  the level you dropped on, or *show the imported one*, which adds nothing and lights the book the
  folder holds, wherever in its tree it stands. A file whose folder never placed it is no question
  and simply imports as a copy. And a file the folder's log remembers — removed, or moved out as a
  copy that has since gone — is not a question either: the import spends the log and the book comes
  back where the FOLDER holds it, in the name the shelf showed, lit up, because a book that
  reappeared somewhere new is a book the reader cannot find.
- **A link is a row, not a book.** It carries the name of the book it points at and nothing else —
  no file, no cover of its own, no page, no highlights, no second copy of a two-gigabyte PDF — and
  tapping it goes to that book wherever in the library it is filed and lights its card up. Because
  it is not a book it is invisible to everything a book is checked by: it never collides, never
  blocks a collision, is never offered back by a watched folder and never queues a cover render. It
  can be dragged, selected, right-clicked and removed like any other row, and removing it takes
  nothing but itself; removing the book it points at takes the link with it, because a pointer at
  nothing is the one thing a link must never be.
- **Two books of one file are two books.** Answering *add as new* to a second copy of the same file
  gives the second one its own name, its own highlights and its own place in it: read one to page
  90 and the other stays where it was, highlight a word in one and the other does not light up, and
  removing either takes nothing from the other. A book nobody asked to separate still shares the
  file's own truth with its twin — one resume point, one highlight list, one removal — because two
  rows of one address are two names for one book until a reader says otherwise.


### Interface

- Glass toolbar with sidebar toggle, open button, document title, centred page navigation, zoom
  popover, view-mode switch, search and an overflow menu.
- A library bar of its own: a breadcrumb starting at Home on the left, a search bar in the centre
  slot that filters the shelf as it is typed and suggests books underneath, and a view menu and
  appearance menu on the right — the appearance menu without its page
  texture section, because the shelf has no page. No Open button and no settings gear: adding books
  is a shelf affordance ([Library](#library)), and settings are about reading, so the gear lives on
  the reader's bar. The bar's pin is remembered per route — the shelf's bar starts pinned, and
  unhitching the reader's bar does not move it.
- Overflow menu with fullscreen and a keyboard shortcut reference.
- Animated sidebar with an outline and thumbnails rail. Panels stay mounted while the sidebar is
  collapsed, so thumbnails survive a toggle without re-rendering.
- Status bar showing the current page position, rendered as a click-through overlay.
- Toast notifications for errors, auto-dismissed after about 3.5 seconds.
- Tooltips on the icon controls.
- The library is what is on screen when no document is open, and it keeps the drag-and-drop
  affordance: a drop there files the documents rather than opening the first of them.

### Persistence

Settings are stored in local storage under `pdfreader.settings.v1` and cover appearance, the
active preset, user presets, default zoom, the layout and motion switches, the two title-bar pins
(the reader's and the library's — separate memories, and the library's starts pinned), and the
last opened path. A group added later simply defaults: a document opened by an older build keeps the behaviour
it had, because every new switch defaults to what the app used to do.

The appearance model changed shape during development, from six fixed themes to base mode plus
computed tint plus presets, but the storage key was deliberately not bumped. A new key would have
silently reset every reader's last-opened file and zoom as well. Instead the retired fields are
retained as optional values, migrated on load to the preset that reproduces the theme previously
in use, and dropped when writing, so the migration runs at most once.

The library is a separate blob under `pdfreader.library.v2`, holding the books, their shelves, the
watched folders and their ledgers, and the view — one key because they are one invariant: a shelf
member that names no book is a hole in the grid. It is deliberately *not* part of the settings blob,
because a resume point moves on every page turn while settings repaint the theme on every write. The
cover cache stays on its own key for the same reason: a cover is a base64 JPEG, and putting the art in
the same blob would make turning a page re-serialise the whole shelf's covers.

A watched folder's blob also carries its ledger: the fingerprints it has placed, a tombstone per
deliberate removal, and what its last scan saw. That is what lets a rescan be honest and a restore menu
open without walking a directory tree, and it is bounded by the library's own cap — a folder cannot have
placed more books than the library holds.

A previous build's recent-books list (`pdfreader.library.v1`) is migrated on first load rather than
discarded: every row becomes a book read in place, in the order the reader had it, with its resume
point intact. The migrated rows carry a placeholder fingerprint until the startup pass measures
them, and a watched folder will not rescan against a placeholder — a real fingerprint matches none,
so every book the folder already held would be added a second time. The old key is left in place, so
a downgrade still finds the library it wrote.

Writes are debounced by 350 milliseconds so dragging a slider does not hammer local storage.

### Accessibility and motion

- `prefers-reduced-motion` is honoured: page-turn animations, the animated grain and general
  transitions are disabled or reduced to a negligible duration. Settings → Layout → Animations
  applies the same collapse on request, and Settings → Animations then narrows it per motion (see
  [Motion](#motion)). The one exemption is the loading mark, which keeps a motionless fade rather
  than a still frame: an indicator that stops moving reports the wrong thing. Its animation is a
  `transform` on three dots, which the compositor keeps running while the main thread parses a
  document, so the mark is never frozen by the work it stands for.
- Interactive controls carry `aria-label`, `aria-pressed`, `aria-current` and `aria-disabled`
  as appropriate, and icon-only segments carry a title so they are labelled for screen readers
  and on hover.
- Decorative icons are marked `aria-hidden`.
- Keyboard shortcuts do not fire while focus is inside a form control, and chrome scrollers such
  as the thumbnail rail, the sidebar and popovers keep their own arrow-key behaviour.

### Performance and memory

- Release builds optimise for size with link-time optimisation and a single codegen unit, and the
  WebAssembly output is passed through `wasm-opt`.
- Thumbnails are cached as bitmaps in an LRU of 16 entries — each thumbnail is a pair of rasters,
  so the tight cap is what keeps the whole grid near eight megabytes. A cache hit blits
  synchronously with no skeleton, no pulse and no transition, so a remounted row has nothing left
  to flicker.
- New thumbnail renders draw into a detached canvas, so a live canvas is never shown mid-render.
- Canvas backing stores are explicitly released on unregister, which avoids a WKWebView leak where
  memory grew with every page scrolled.
- Colour slider changes repaint CSS at most once per frame and commit to settings 180 milliseconds
  after the last tick, so dragging does not reallocate the page filter continuously.
- Resize observers are disconnected on cleanup. Without this, a resize queued during teardown
  could invoke a dropped closure and abort the WebAssembly runtime.
- Device pixel ratio is respected when sizing canvases, so pages stay crisp on high-density
  displays.

---

## Keyboard shortcuts

| Action | Shortcut |
|--------|----------|
| Open document | `Cmd/Ctrl` + `O` |
| Search — the document while one is open, the library's shelf filter while it is not | `Cmd/Ctrl` + `F` |
| Fit width | `Cmd/Ctrl` + `0` |
| Single page view | `Cmd/Ctrl` + `1` |
| Continuous view | `Cmd/Ctrl` + `2` |
| Zoom in | `+` or `=` |
| Zoom out | `-` or `_` |
| Previous page | `Left arrow` |
| Next page | `Right arrow` |
| Scroll up / down (continuous) | `Up arrow` / `Down arrow` |
| Turn page (single) | `Up arrow` / `Down arrow` |
| Screen up / down | `Page Up` / `Page Down` |
| Screen down / up | `Space` / `Shift` + `Space` |
| Auto-scroll on or off (the two scrolling modes) | `Shift` + `A` |
| Move through the library's search suggestions | `Up arrow` / `Down arrow` in the search bar |
| Go to the chosen suggestion's book, on its shelf | `Enter` in the search bar |
| Dismiss overlay or search; close the library's suggestions, or clear its search | `Escape` |

In continuous mode the reader owns the arrow keys and scrolls the page list directly. Leaving them
to the browser meant scrolling whatever held focus, which was usually a text-layer span; when
virtualization unmounted that page the focused node disappeared, key repeat died, and the next
press landed on the document body. The page list is focusable and reclaims focus when a descendant
is removed, so hold-to-repeat stays aimed at a node that outlives any single page.

A held arrow glides continuously at roughly 1000 pixels per second after a 350 millisecond delay,
rather than stepping discretely, so browser key repeat cannot chunk the motion. A single tap moves
8 percent of the viewport, clamped between 40 and 80 pixels, which matches native line scrolling.
Page-level keys move 90 percent of a screen, leaving a sliver of overlap so the last line read
stays visible.

---

## Architecture

### Layer overview

```
+-------------------------------------------------------------+
|  Tauri v2 shell (Rust)                                       |
|  native window, file dialog, asset protocol, fullscreen      |
+-------------------------------------------------------------+
|  Leptos 0.8 interface (Rust, compiled to WebAssembly)        |
|  features / components / state / effects, reactive signals   |
+-------------------------------------------------------------+
|  Typed bridge (wasm-bindgen)                                 |
|  pdf-engine: snake_case Rust mapped to camelCase engine      |
+-------------------------------------------------------------+
|  reader bundle (JavaScript, public/readerEngine.js)          |
|  selection tracking for every format; needs no pdf.js        |
+-------------------------------------------------------------+
|  window.PDFReader engine (JavaScript, public/pdfEngine.js)   |
|  render queue, thumbnail cache, text layer, search index     |
+-------------------------------------------------------------+
|  pdf.js 6.2.108, vendored into public/vendor/pdfjs           |
+-------------------------------------------------------------+
```

Pure logic lives in `reader-core`, `pdf-core`, `reflow-core`, `txt-core`, `md-core`,
`ui-geom` and `ai-core` — no DOM and no Leptos — so the view-mode arithmetic, the zoom
ladder, filename rules, colour conversion, the search index, text typography and pagination,
settings migration, the floating-panel placement and the AI word-card's geometry and spring
are all unit-testable on the host.
The layering is a fan with a rule: `reader-core` knows no format at all, the format cores
depend on it, and no core depends on another format's core — which is why adding a format is
a new crate plus a new directory, not an edit to the ones already there. `virtual-list` is the
generic windowing-math library under the viewer, and `ui-geom` the geometry every floating
surface places itself with — a leaf with no dependencies, so the window chrome and the AI
card can share one placement rule and one spring without either depending on the other.

PDFs and text documents share one page/zoom/navigation machinery above the rendering layer; only
the leaf differs. A PDF page is a canvas the pdf.js engine paints; a text page is an A4 host the
reader lays out with real type. Both report the same per-page sizes into the same virtualized
strips, so view modes, zoom and navigation are format-agnostic.

### Project layout

```
src/
  main.rs                 mount entry point
  app/                    bootstrap, routes, the shell that hosts the sidebar
  components/
    primitives/           button, switch, popover, floating positioning,
                          motion and interaction hooks (the long-press, the
                          pointer-drag stream, and the card wrapper that
                          decides between a tap, a hold and a drag; the
                          chrome's own primitives — icon, icon button,
                          tooltip, the generic DOM/timer hooks — live in
                          app-chrome)
    shell/                the unified application shell: the ShellController
                          (one source of truth for layout), the titlebar
                          family, the sidebar rail family
    menus/                app menu, appearance menu, reader menu
    settings/             the settings modal and its tabs (layout, theme,
                          animations, fonts); the theme tab composes sections
                          it does not own — the AI's from ai/, the raster ones
                          from its own paper module
    viewer/               the SHAPE of reading: the mode dispatch, the four
                          layouts (single, two-page, continuous, horizontal),
                          the shells that own the scroll container,
                          page_host — the one seam that picks a format —
                          refresh, the fingerprints an overlay repaints on,
                          and controls/ (bottom bar, overlay scrollbar, page
                          indicator, page navigation)
    formats/              the SUBSTANCE of a document: pdf/ (canvas + strip),
                          reflow/ (A4 page host, continuous stream, strip,
                          the spot walk that finds a block's
                          characters in the DOM, and the search-hit layer that
                          paints over them), txt/ and md/ block views, and
                          block_render, the renderer dispatch
    search/               floating search bar and result list
    ai/                   selection pill, word card, gloss popover, the anchor
                          resolvers that place a mark's stroke, and the AI
                          appearance section of the settings modal
    app_overlays/         drag-and-drop feedback, toast host
  effects/
    app/                  window title, shortcuts, persistence wiring,
                          drag-and-drop admission, and the library's
                          app-lifetime wiring (startup measurement, focus
                          rescan, the progress sink)
    reader/               fit and zoom follow, page tracking
    appearance/           the appearance-to-CSS bridge (shared, raster,
                          reflow)
  features/
    library/              the library page: its three-slot bar (breadcrumb,
                          search, view menu), the grid and the list, book cards
                          and the folder cards a shelf nests in, the two ways in
                          (add card, empty state) and the sheet they open, the
                          import dock, and the drag both views share (the
                          session and its sink, the targets, the table that
                          decides what a drop means, and the layer that
                          draws it)
    reader/               the reader page and its two virtualizers
  state/                  the reactive state tree: app (chrome + UI), reader
                          (document, viewer, zoom, search, gloss, AI selection),
                          library
  services/               the document open pipeline, the AI chunk bridge, and
                          the library's filesystem wire (the shell's invoke
                          wrappers and progress bridge, the import orchestration
                          that runs the ledger, and the moves a reader makes by
                          hand)
  storage/                loads and saves over localStorage (settings,
                          library, covers, gloss marks)
  zoom/                   the zoom pipeline: posted commands, target
                          resolution, the tween, and the actuator that owns
                          the one relayout path over both strips
  dom_contract.rs         the attribute, class and element-id names the engine
                          reads and the app writes — one table, both sides
  events.rs               the window-event names the engine dispatches and the
                          app listens for
crates/
  ai-core/                the format-agnostic AI core: the word-explanation
                          wire types (WordInfo, AiError, the chunk envelope),
                          the gloss card's geometry (stepping `ui-geom`'s
                          spring), the gloss mark + MarkAnchor trait
                          (PageAnchor is the PDF impl), the per-document gloss
                          cache and its persistable JSON, and the Tauri
                          explain_word kickoff (the reader settings those types
                          feed live in reader-core, because they are the
                          reader's)
  reader-core/            the reader with no format in it: the format list, the
                          view modes and spread arithmetic, the settings schema
                          (layout, animation, typography, gloss), the colour
                          pipeline, the presets, filename rules, zoom maths, the
                          outline shape, and the shared search model — result
                          shape, the scan both pipelines run, the snippet window
  pdf-core/               pure PDF domain math: page layout constants, the
                          outline wire entries and their clamping, the
                          device-pixel grid the page hosts snap to, and the
                          engine-side page-text index a query scans
  reflow-core/            the shared maths of reflowable text: the block shape
                          and its splitting, the A4 geometry and spine sides,
                          the page cutter, the height estimate, typography
                          resolution and the search over blocks
  txt-core/               plain text: normalisation, paragraph parsing and the
                          line-bounded subdivision a tight page pack needs
  md-core/                Markdown: construct classification, prose subdivision,
                          front-matter metadata and the heading outline
  library-core/           the library's domain: the book and its fingerprint,
                          the two origins (read in place, copied into the
                          store), shelves and watched folders, the scan
                          predicate, the rescan ledger that answers
                          add/relink/skip, the sort, the persisted blob and its
                          migration, the merge rule for two rows of one book,
                          and the wire types the shell and the frontend share
  pdf-engine/             wasm-bindgen bridge to the imperative engine
  pdf-paper/              the blend backdrop's colour brain: the detection
                          area, dominant-colour detection (whole page or edge
                          margins), the per-page palette
  virtual-list/           generic windowing math: the prefix-sum strip,
                          windows, budgets, anchor correction
  virtual-list-leptos/    the Leptos adapter: virtualizer, rows, retention
  ui-geom/                pure geometry for the surfaces that float: panel
                          placement and viewport clamping, plus the damped
                          spring the floating panels and the gloss card ride
  tauri-bridge/           the raw window.__TAURI__ externs (invoke, event
                          listen, window handle, dialog) + the has_tauri
                          probe, declared once for every frontend crate
  app-chrome/             format-agnostic window chrome: the platform probe,
                          the window commands, the caption cluster (Windows
                          squares, GNOME circles), the native macOS traffic
                          lights, the generic titlebar shell, the floating
                          surface adapters (placement glue, shared dismissal),
                          and the shared UI primitives + hooks those surfaces
                          render with
public/
  readerEngine.ts         the format-agnostic bundle (the selection tracker),
                          loaded first and needing no pdf.js
  reader/                 its modules
  pdfEngine.ts            the imperative engine wrapper (bundled to .js)
  engine/                 the engine modules the wrapper imports
  vendor/pdfjs/           vendored pdf.js build, worker, viewer CSS, cmaps
  samples/                five fixture PDFs for manual testing (the smoke test
                          runs against stubs, not these)
src-tauri/                native shell, AI providers, the library's filesystem
                          commands (folder walk, path check, store copy and
                          delete), capabilities, icons
styles/
  input.css               Tailwind v4 entry point assembling the design system
  tokens.css              the @theme block, base palettes and runtime vars
  page_host.css           the .pdf-page host, its canvas and the zoom snapshot
  text.css                the reflowable page host and the continuous stream
  textures.css, noise.css texture modes, and the grain overlay + its crawl
  components/library/     the bookshelf, one file per surface: the grid and its
                          book cards, the folder's cover plate, the list rows,
                          the import dock's ring, the reveal, the drag layer a
                          held set is carried in, the breadcrumb's elided chain,
                          the multi-select a hold starts, and the search
                          suggestions
  components/             shell, title bar, animations, ai, gloss, appearance,
                          thumbnails, pdf.js's text layer, and the search-hit
                          box both format families share
tools/                    engine bundling, the engine smoke test, the
                          repo-reading prelude the checks share, and the
                          consistency checks CI runs (versions, formats, doc
                          paths, event names, the DOM contract)
scripts/                  generated only: the compiled tools above. Gitignored,
                          and ignored wholesale by Trunk's watcher, so the hook
                          rewriting them on every build cannot retrigger one
tests/                    source-level tests (e.g. the conditional-class lint)
release-notes/            one file per version; the release workflow publishes
                          the one matching the tag as the release body
```

### Engine API

`window.PDFReader` is the single boundary between Rust and pdf.js. Every function resolves and
never rejects: success is `{ok: true, ...}` and failure is `{ok: false, error: {name, message}}`,
so the Rust side reads `ok` first and then deserializes.

| Function | Purpose |
|----------|---------|
| `version` | Engine version string |
| `open` | Load a document, return page count and intrinsic page sizes (the outline is NOT resolved here — that is `resolveOutline`'s job, so opens stay fast on chapter-heavy books) |
| `resolveOutline` | Flatten the open document's chapter tree after the reader is up |
| `destroy` | Tear down the current document |
| `registerPage` / `unregisterPage` | Bind and release a canvas for a page |
| `cancelPage` | Cancel an in-flight page render |
| `renderPage` | Render one page |
| `renderThumb` / `cancelThumb` | Thumbnail rendering on a separate, cheaper path |
| `hasThumb` / `blitThumb` | Probe the bitmap cache and blit a cached frame |
| `extractPageText` | One page's text items with their rects — the input to the search index, which is Rust (`crates/pdf-core`'s `SearchIndex`), not the engine's |
| `setSearchContext` / `setActiveMatch` / `clearHighlights` | Paint, move and clear the engine's highlight rects in the text layer |
| `refreshTheme` / `setScrubMode` | The appearance theme: pre-render (re-bake) it into every canvas, or hold the rasters raw under the live CSS filter chain for the length of a slider scrub |
| `setPaper` / `setPaperActive` / `takePaperFrame` / `samplePaperPage` | The paper session: the backdrop's own raster, handed to and sampled from the pages |
| `coverDataUrl` / `prefetchThumb` | The shelf cover and thumbnail prefetch |
| `stats` | Internal counters, used to assert memory is actually released |

Load order in `index.html` is deliberate. The reader bundle goes first because it needs nothing;
then pdf.js, which is ESM-only in version 6 and must execute before the engine so
`globalThis.pdfjsLib` exists; then the engine. All three run before the WebAssembly module, which
top-level-awaits its own init: the app reaches for `window.PDFReader` and for the selection state
as soon as its first components mount, and a module script that had not run yet leaves both
undefined.

### State model

A single `AppState` tree of Leptos signals is threaded through the component tree: the persisted
`settings`, the reader, the library, and the app's own `ui` state. The reader's branches are the
document, the viewer — page, mode, fit, container size, and the zoom transaction inside it —
search, the gloss marks and the AI's selection state. Effects subscribe to it rather than
components talking to one another, which keeps ownership of each concern in exactly one place.
During a zoom, for example, a single system owns the display scale and render scale, and every
zoom control posts a request to it rather than writing the scale directly.

---

## Getting started

### Prerequisites

- **Rust** with the `wasm32-unknown-unknown` target
- **Node.js** 20 or newer, required by the Tailwind v4 CLI
- **Trunk**, the WebAssembly bundler
- **The Tauri CLI**, which the `cargo tauri dev` and `cargo tauri build` commands below run
  through
- **Tauri v2 system dependencies** for your platform, listed at
  <https://tauri.app/start/prerequisites/>

```bash
rustup target add wasm32-unknown-unknown
cargo install trunk
cargo install tauri-cli --version "^2" --locked
```

### Installation

```bash
git clone https://github.com/codewiththiha/pdf-reader.git
cd pdf-reader
npm install
```

### Development

Run the desktop application with hot reload:

```bash
cargo tauri dev
```

This generates the TypeScript bundles, starts Trunk on port 1420 and opens the native window. Tailwind is compiled by a Trunk pre-build hook, so stylesheet changes rebuild
automatically. To run the interface in a plain browser instead, without the native shell,
generate the engine bundle once and serve:

```bash
npm run build:ts
trunk serve
```

Note that the file dialog and drag-and-drop rely on Tauri and are unavailable in a browser.

### Building for release

```bash
cargo tauri build
```

Bundles are produced for the host platform in `target/release/bundle` — the workspace shares one
target directory, so nothing lands under `src-tauri/`. The bundle
configuration targets all available formats and ships icons for macOS, Windows and Linux.

### Tests

```bash
cargo test --workspace --exclude pdf
```

The manifest root is also the `pdf-reader` app package, so a bare `cargo test` would test
only the app and silently skip every member crate. The `pdf` shell crate is excluded because
`tauri::generate_context!` resolves the frontend dist at compile time; it is clippy-checked
and unit-tested natively on the macOS CI job instead.

Around 460 tests cover the pure layer: zoom and fit maths, page layout and spread stepping,
filename derivation, colour conversion, appearance CSS generation, presets, settings
migration, search index arithmetic, outline activation, thumbnail geometry, the frame delta
the animation loops share, and the virtual-list windowing invariants. On top of that, the
TypeScript layer has its own stub-vm smoke suite (`node scripts/test-engine-smoke.js` in CI)
covering open, render, theme baking, scrub mode, thumbnails, search, teardown, and the reader
bundle's selection tracker — the last in a sandbox with no engine and no pdf.js in scope,
which is the point of it.

Five small scripts guard facts that are written down more than once, where nothing else
would notice a drift: `check-versions.ts` (the app version in four files, plus the two
lockfile entries cargo derives from them),
`check-formats.ts` (the openable formats in the reader-core registry, the shell's
filesystem gate and the bundle's file associations), `check-doc-paths.ts` (every module and
file path named in a Rust comment still resolves, every path named in a stylesheet comment,
every component name the two documents put in backticks, and every method in the README's
engine table), `check-events.ts` (the window-event names
the engine dispatches match the app's table) and `check-dom-contract.ts` (the attribute,
class and element-id names the app writes match the ones the engine reads, and appear
nowhere as a raw literal). Each is TypeScript in `tools/`, compiled by the same Trunk pre-build
hook into `scripts/`, and each fails CI rather than warning. They share one prelude, `repo.ts` —
the repo root, `read`, `isFile`, and the tree walk with its list of directories that are not
source — because five copies of a skip list is five chances for one tool to start scanning
`node_modules`. `scripts/` holds nothing but their compiled output, which is why git ignores the
directory and Trunk's watcher does too: the hook rewrites those files on every build, and a
watcher that notices would rebuild forever.

---

## Configuration

| File | Purpose |
|------|---------|
| `Trunk.toml` | Build target, the Tailwind pre-build hook, watch exclusions, dev server port |
| `src-tauri/tauri.conf.json` | Window size and title, bundle targets, asset protocol scope |
| `src-tauri/capabilities/default.json` | Permission grants for the main window |
| `styles/input.css` | Tailwind v4 entry point and the complete design system |
| `.taurignore` | Paths excluded from the Tauri file watcher |

The asset protocol is scoped to the home, desktop, documents and downloads directories, which
is what allows the engine to read a chosen file while keeping the rest of the filesystem out
of reach.

The default window is 1200 by 800, with a floor of 640 by 480.

---

## Technology stack

| Layer | Technology |
|-------|------------|
| Desktop shell | Tauri v2 with the dialog and single-instance plugins |
| Interface | Leptos 0.8, client-side rendered |
| Language | Rust 2024, compiled to WebAssembly |
| Bundler | Trunk |
| Styling | Tailwind CSS v4, CSS-first configuration |
| PDF rendering | pdf.js 6.2.108, vendored locally |
| Text/Markdown rendering | In-app pagination plus `leptos-md` (GFM, no raw HTML) |
| Serialization | serde with `serde-wasm-bindgen` |

pdf.js is vendored into the repository rather than fetched at runtime, so the application renders
with no network access and the exact engine build is pinned alongside the source.

---

## License

Released under the MIT License. See [LICENSE](LICENSE) for the full text.

This project bundles [pdf.js](https://github.com/mozilla/pdf.js), which is distributed under the
Apache License 2.0.
