# Library: design

> **Status:** design for M5 · **Last updated:** 2026-10-15

mareader's library is ~32k lines (`library-core` 10k, `library-runtime` 22k,
plus `src-tauri/src/commands/library.rs`), and its design notes
(`Mareader.md` § The library) run past 1,000 lines. This document keeps what it
achieves and drops the complexity its model created.

## What the library is for

Read from mareader's README § Library and `Mareader.md`. A user can:

1. **Gather books from anywhere.** Read a folder in place, or have the app
   keep its own copy. Loose files (dialog, drop) are kept as copies.
2. **Stay in sync with the disk.** Watched folders rescan on launch and when the
   window regains focus: new files appear, moved files are followed,
   vanished files are flagged missing, never silently dropped.
3. **Organise.** Nested shelves, drag to file (several books at once), a book
   on more than one shelf, a manual order or a sort.
4. **Find.** Search as you type over title, author and path, with substring and
   fuzzy matching and suggestions; grid or list, column count, sort, covers.
5. **Never lose their place.** Reading position and marks belong to the book
   and survive moves, renames and relinks.
6. **Never lose a file.** The app deletes only copies it made. Removing shows
   what goes first. A book removed from a watched folder can be restored. A
   missing book can be found again (pick the file, or a folder to search).
7. **See imports progress** without blocking the window.

## Where mareader's size comes from

| mareader mechanism | Why it was needed there | mareader2 |
|---|---|---|
| A watched folder's tree *is* a set of editable shelves ("rungs" on "seats") | Moving a read-in-place book off its rung had to turn it into a copy ("departure"), with families, returns, moved-out logs and a copy-cost sheet | **Sources are read-only.** A watched folder is browsed as the disk's own tree; shelves are separate, user-made collections. Filing a book never touches a source, so departures don't exist. |
| Names unique per level | Every drop, import and filing could collide: a conflict sheet with three answers per gesture | **Shelves hold book ids.** Two books may share a title; dropping a book on a shelf that has it is a no-op that highlights it. Only sibling *shelf* names must differ (rename refuses). |
| Rows vs books: link rows, twin rows, `independent` rows | One file could be several rows; every writer had to fold twins | **One content, one book, unless the reader asks for a twin.** A book has a list of locations; a book on two shelves is two memberships. A twin is a second book with its own id, and reading state is keyed by id, so nothing folds (§ Independent twins). |
| Fingerprint = size + mtime + 8 KiB head hash; placeholders gating rescans | mtime changes on copy; migrated rows had no measurement | **Content identity:** size + hash of the first and last 64 KiB. A `(path, size, mtime)` cache skips re-hashing unchanged files, so a focus rescan stays cheap. |
| Per-folder tombstones with names, addresses and shelves | To stop rescans re-adding removed books, and to offer them back | **`hidden` per source:** removing a source's book hides it there and keeps its record (with progress) for restore. |
| A custom pointer-drag engine, DOM probes for crumbs, virtual-list crates | Browser limits | egui `dnd_*`, `Galley` measurement, `ScrollArea::show_rows`. |

## Model (`crates/library`, L1)

Plain data plus pure operations. Filesystem access sits behind a small `Disk`
trait (walk, measure, copy, delete-in-store) so every rule is a unit test with a
fake disk.

```text
Library  { books: BTreeMap<BookId, Book>, shelves: BTreeMap<ShelfId, Shelf>,
           sources: BTreeMap<SourceId, Source> }
Book     { id, content: ContentId, title, author, format: Pdf | Epub,
           locations: Vec<Location>, added, opened, position, marks_key,
           twin_of: Option<BookId>, design: Option<Auto | On | Off> }
Location { path, origin: Source(SourceId) | Store, present: bool, stamp: (size, mtime) }
Shelf    { id, name, parent: Option<ShelfId>, books: Vec<BookId> }   // manual order
Source   { id, root, formats, depth, hidden: BTreeSet<BookId> }
```

**Operations** return a `Result` and a description of what changed (for the
UI's toast and for undo later):

- `add_source(root, formats)` and `scan(source, walk) -> ScanReport`
  (added, followed, missing, back). Pure: the walk is input.
- `import_files(paths)` copies into the store (`<data>/items/<id>/`), keyed by
  id so names never collide on disk.
- `file(books, shelf)`, `unfile`, `reorder`; `create_shelf`, `rename_shelf`,
  `move_shelf` (refuses cycles), `remove_shelf(keep_books | remove_books)`.
- `removal_receipt(books)` then `remove(books)`: hides source books, deletes
  store copies, drops memberships; marks and position go with the record
  unless it is only hidden.
- `relink(book, path)` after re-measuring the file; `restore(source, book)`.
- `record_position(book, position)`.
- Queries: `query(text, sort, scope) -> Vec<(BookId, Spans)>` (one rule for
  the filter and the suggestions), `children(shelf)`, `path_to(shelf)`.

### Independent twins

Kept from mareader (owner decision, 2026-10-13): two books of one file, each
with its own name, place, highlights and shelves. Read one to page 90 and the
other stays where it was; removing one takes nothing from the other.

- **Making one.** "Read separately" on a book, or "Add as a separate book" when
  an import finds content the library already has. Both call
  `twin(book) -> BookId`.
- **What is its own:** id, title (the counter name, "Moby Dick (2)"),
  position, marks, memberships. All of these are keyed by `BookId`, so a twin
  needs no special case in any writer.
- **What it shares:** the file's facts. Locations, `present`, relinks and the
  cover belong to the content, so `scan` and `relink` update every book at
  that location.
- **Opening by path** (drop, file manager, recent) opens the original, the
  book with `twin_of: None`.
- **Removing** drops only that book's record. A store copy is deleted only
  when no other book still lists the location.

**Walk rules kept from mareader:** filter while walking (formats, depth,
result cap), refuse symlinks, skip hidden directories, re-measure before any
relink, and delete only paths that canonicalise inside the store.

**Persistence** (owner decision, 2026-10-15): versioned serde files, each
written atomically (temp + rename) on a worker after a short debounce.

- `library.json` holds the facts that rarely change: books, locations,
  shelves, sources.
- `state/<book-id>.json` holds one book's position (and its marks, M6). A
  page turn rewrites only that small file, never the whole library; the
  same split is the sync layout (`docs/sync.md` § Wire layout).
- Covers live in the cache directory, keyed by content id, rebuilt on
  demand.

## Identity and naming edge cases (2026-10-15)

- **Text is hashed whole.** Head and tail suffice for PDF and EPUB, whose
  tails (the xref, the zip's directory with each entry's CRC) change with
  any edit. A same-length typo fix in the middle of a Markdown note
  changes neither end, so `.txt` and `.md` (64 MiB at most) hash
  entirely.
- **Metadata never decides identity.** Generic PDF titles ("Microsoft Word
  - Document1", "PowerPoint Presentation", "untitled") would merge
  unrelated files; only content does.
- **Display title.** The document's title, unless it is empty, generic or
  just the file name; then the file name, without a copy mark: `book
  (1).pdf` shows as `book`.
- **A source file edited in place** (a Markdown note, a re-saved PDF) is
  the same book with new content: it keeps its id, position, marks and
  shelves. A source path is the user's own file. Store copies never
  change.
- **A newer version imported** (a re-downloaded or re-exported book) is new
  content. When its title, author and format match a book, the import
  sheet offers "Replace": the book keeps its id, shelves and marks, and its
  position clamps to the new length. One book, last import wins.
- **Hash races.** Two imports of one content in a single batch, or a scan
  and a drop at once, meet in one lookup by content id inside the
  operation, so they make one book.

## Text files in the library (2026-10-15, corrected 2026-10-25)

Markdown and plain text open by converting to an EPUB in memory on the
session's workers (`docs/epub.md` § Text files as EPUBs). Conversion is
cheap and byte-for-byte repeatable, so the library keeps only the source:
no EPUB is written, and every open converts afresh, so a better converter
reaches old books too.

- **Read in place.** The library holds the file's path; an edit to it is
  the same book with new content (§ Identity and naming edge cases).
- **Live reload (2026-10-25).** An open book saved elsewhere, in any
  format, reloads within about two seconds where it is read
  (`docs/architecture.md` § Threads and resources). Store copies never
  change, so only files read in place ever do.
- **Store copies** are the source file in `files/<content>/<name>`. A
  Markdown file's local pictures come with it (`disk::copy_beside`), so the
  copy shows them even once the original is gone. Paths inside the file's
  folder keep their place; the rest are kept inside it under a stand-in
  (`../a.png` as `^/a.png`, `/x/b.png` as `^root/x/b.png`), which the
  copy looks for when the real path isn't there. The copied note itself is
  untouched, so its content id holds. Deleting the copy deletes its folder,
  pictures and all.
- **Titles and covers** convert the file too: `describe` without its
  pictures, `cover` with them.

## Covers in detail (2026-10-15)

mareader made covers for PDFs only, keyed by path (a move re-rendered
them), held as data URLs and capped at 60. mareader2 covers every format,
keyed by content:

- **Where the art comes from.**
  - PDF: the first page, rendered at cover size on a worker, apart from the
    page cache.
  - EPUB: the manifest's `cover-image`, then EPUB 2's `meta name="cover"`,
    then the first picture of the first chapter, as the reader places it.
  - Markdown: the front matter's `cover`, then its first picture.
  - Plain text and books without art: a drawn cover, the title and author
    on a card tinted from the theme. Painted by egui, so it follows the
    theme instantly and costs no file.
- **The cached file:** `<cache>/covers/v<N>/<content-id>.jpg` (or `.png`
  when the art has transparency), long edge 512 px, never upscaled.
  - Written atomically, temp then rename.
  - Bumping `N` when rendering changes rebuilds every cover; the old
    directory is deleted at launch.
  - Twins share one cover, and a moved file keeps it.
- **Failures.** Undecodable art writes nothing; the content is remembered
  as failed until the next launch, and shows the drawn cover.
- **Loading in egui.** `ScrollArea::show_rows` names the visible rows, so
  no observer is needed.
  - The visible rows are requested first, then a screen above and below.
    Rows scrolled away are dropped from the queue by a generation number.
  - Workers decode to a `ColorImage` at the card's pixel size
    (`pixels_per_point`); the UI uploads a few per frame, so a fast scroll
    never stalls a frame.
  - Textures sit in an LRU keyed by content id with a byte budget; the
    drawn cover shows until the art arrives, then fades in unless
    Animations are off.
- **A cover the user picks** (later) is user data: it lives in the data
  directory, not the cache, and syncs.

## Ready for sync (2026-10-15)

Sync comes after 1.0 (`docs/sync.md`), but its ground is laid in M5 so no
library needs migrating later: ids nobody else mints, a hybrid-clock stamp
on each synced field, tombstones instead of deletion, sort keys for
manual order, and synced facts kept apart from a machine's own
(`docs/sync.md` § What M5 must lay down).

## Slice 1: records and resume (2026-10-15)

What the first slice built, and where it stops short of the model above.

- **Opened files are books.** Every file a pane opens (dialog, drop, the
  system, launch) is measured on the library's worker and recorded with a
  third origin, `Opened`: read where it is, never moved or deleted. Sources
  and the store arrive with their slices. Samples and other in-memory
  documents have no file, so they are not recorded.
- **Recording an open.** The book already at that path, while its content
  matches, is the one opened: a kept second copy stays its own book. Next,
  the book holding that content takes the path as its newest location (a
  move). Failing that, the book at that path takes the new content (an edit
  in place). Failing all, a new book is made, and a removed book of that
  content comes back. A path belongs to one book; the book that held it
  loses it.
- **Opening a copy keeps it a copy.** A path the book holds as a store copy
  stays `Store`, with the file it came from; only other paths are `Opened`.
  Before 2026-10-25 an open marked the copy `Opened`, so the AI skipped it
  and removing left its file. Loading turns any `Opened` location shaped
  like a copy (`files/<content>/<name>`) back into `Store`.
- **Positions** are the page, the page count then, and a book's block
  anchor, recorded each frame but written only when they change. Opening a
  known file without a page asks the library, so any open resumes. Opens
  that come before the library has loaded wait for it.
- **Measuring.** SHA-256 of the size and, for PDF and EPUB, 64 KiB from each
  end; text, and anything up to 128 KiB, is hashed whole. The first 16
  bytes are the content id.
- **Files.** `library.json` is `{ schema, clock, books }`;
  `state/<id>.json` is `{ schema, book, state }`. Each field carries its
  stamp. Writes leave at most every half second, and on exit.
- **Failure.** A `library.json` from a newer schema is not written to; the
  library lives in memory and says so. An unreadable one is renamed to
  `library.unreadable.json` and the library starts empty. An unreadable or
  orphaned state file is skipped. Each location is checked when the library
  loads; a book with none left shows as missing and says so when clicked.
- **The page.** The title bar's Library button swaps the panes for a list,
  last opened first: format, title, page and progress, and when it was last
  opened. Escape goes back if something is open; any open does too.
- **Not yet.** No `Disk` trait: the pure rules take what they need (the
  existence check is a closure) and `disk` does the rest. Authors, covers,
  the character offset for converted books, and following a file that
  moved while the app was closed all wait for later slices.

## Slice 2: grid, list, sort and covers (2026-10-15)

- **View.** `mareader_library::View` keeps the layout, the pinned columns
  (none is Auto), the cover fit and the sort, inside the settings as
  `library`. An older settings file without it loads the defaults: grid,
  Auto, Fit, last read newest first. The View options menu (in the title
  bar, § Routes) changes each at once and stays open to chain changes.
- **Sort.** Last read, title, author or date added, either way. A new key
  starts in its natural direction (names A–Z, dates newest first). Names
  compare ignoring case, with digit runs as numbers, so Volume 2 comes
  before Volume 10 and `007` equals `7`. A blank author sorts last both
  ways. Ties fall to the title, then the book id, so the order never jumps.
- **Grid.** Auto fits as many 150 pt cards as the width allows, 2 to 10;
  a pinned count shares the width. Each card is an A4 cover box, the title,
  the author (else the page reached), and a progress line. Fit shows the
  whole cover bottom-aligned; Crop fills the box. Covers shaped past 0.55
  to 1.8 are trimmed to it. Rows are virtual (`show_rows`).
- **List.** The rows gain a thumbnail; the format joins the detail line.
  A missing row fades its time too, and the header lines up with the rows.
- **Authors.** An EPUB's first non-blank `dc:creator`, or a PDF's
  `/Author`, fills a book's author when it has none; a later open never
  overwrites it, so a reader's own edit will stand.
- **Covers** follow the section above. The session's `cover` reads the
  art: a PDF's first page; an EPUB's declared cover, else a picture among
  the first four blocks of its opening chapter; text and Markdown are
  converted first. One `covers` thread takes the first still-wanted ask;
  each frame replaces the asks (shown, then a screen below, then above).
  Results keep as JPEG (opaque) or PNG at 512 px in
  `<cache>/covers/v1/<content-id>`, written atomically; other versions'
  folders are deleted. The cache is the platform's (`~/Library/Caches`,
  `$XDG_CACHE_HOME` or `~/.cache`, `%LOCALAPPDATA%`), and tests have none.
  Four textures upload per frame within a 64 MiB budget, least recently
  shown dropped first; each fades in unless Animations are off. A book
  without art gets a drawn cover: a cloth binding in a steady per-book
  tone, its initial and format (`docs/design.md`, Book covers).
- **Title bar.** The library keeps its own pin (on by default), set from
  the bar's Pin alone, as mareader's.
- **Not yet.** Manual order arrives with shelves. A cover that failed is
  retried only after a relaunch.

## Slice 3: folders read in place (2026-10-15)

- **Adding.** Folders (beside Open…, and on the empty page) adds a folder
  through the system dialog. Its books are read where they are: nothing is
  copied, moved or written there. A folder inside one already added is
  refused by name; one holding added folders takes them in (their books
  keep their places) and says so. The library's own folders are never
  walked. `library.json` moves to schema 2 with a `sources` list; an older
  app leaves the newer file alone.
- **What counts.** Each folder has formats (all four by default, at least
  one), a size floor and watching (on), under What counts. Narrowing
  keeps books already found; widening rescans that folder. A walk skips
  hidden and `~$` names, system folders (`node_modules`, `__MACOSX`, the
  recycle bin and the like) and links, stops at
  20,000 files, 20,000 folders or 24 levels, and says when a folder was cut
  short. A file written in the last 3 s waits for the next look, 3.5 s
  later, so a copy in progress isn't measured half done.
- **Rescans.** On launch, when the window comes back (at most every 2 s),
  from Rescan all, and per folder from its Rescan button. A full rescan
  first re-checks every known file, then walks the watched folders; a named
  one walks only that folder. Unchanged files (same size and modified time)
  aren't hashed again, and names are read only from contents the library
  hasn't met. The walk, hashing and reading run on the library worker.
- **What a scan does.** A known content under a new path is the same book,
  followed with its place, and its lost addresses are dropped. A known path
  with new content is an edit in place: the same book. A removed book's
  content comes back as that book, with its place. Anything else is a new
  book, found but never opened: it lists "Not opened" and sorts last by
  last read. Titles come from the file (an EPUB's `dc:title`, a PDF's
  `/Title` unless generic, Markdown front matter), else the file name; PDFs
  opened in the reader now use the same rule.
- **Missing books.** A missing book stays listed, faded, until found or
  removed. When its folder's drive isn't there, it reads Offline ("Drive
  not connected") instead of Moved or deleted. The chips over the books
  show All books, Missing (when any) and each folder with its count.
- **Find again.** Clicking a missing book opens a sheet with its last path:
  Choose the file… or Search a folder…. A chosen file of the same content
  heals the book and opens it. A search walks the folder for the same
  content under any name, checking only files of its old name or size.
  A namesake with other content (another edition, or edited) is offered:
  Use this file keeps the book and its place. A file that is another book,
  or another format, is refused in one sentence. Cancel or Escape changes
  nothing, and Escape doesn't leave the library while a sheet is up.
- **Removing a folder** asks first and says how many books found only there
  leave the library. Books also opened from elsewhere stay. Files are never
  touched; adding the folder again brings the books back with their places.
- **Not yet.** Folders as a browsable tree, hiding single books from a
  folder and restoring them, and a progress card for big first scans come
  with slices 4, 5 and 7. There is no file watcher: rescans are cheap and
  run when the window comes back, as mareader does.

## Slice 4: shelves and the breadcrumb (2026-10-15)

- **Shelves.** New shelf (in the page header and the View menu) makes
  one where the reader is, opens it and selects its name in the
  breadcrumb: type, then Enter or click away; Escape keeps the old name.
  Shelves nest without limit and sort by name, numbers by value. Names are
  one line, trimmed, at most 120 characters, and unique among siblings
  ignoring case; a clash is refused in a toast. A book may sit on any
  number of shelves; a shelf holds books by id, so moves, edits in place
  and find-again keep it on its shelves.
- **The page.** Shelf cards come first: a 2 × 2 plate of the first four
  shelves (a shelf tile) and covers, the name and "4 books · 1 shelf". In
  the list, shelves are rows with a chevron. A click opens a shelf; the
  header then shows its name and counts, and the top level keeps Open…
  and Folders. The chips (All books, Missing, each folder) show only at
  the top level and list matching books flat, without shelves; opening a
  shelf returns them to All books. Right-click on a shelf: Open, Rename…,
  Remove shelf….
- **What the top level shows** is a View setting, Library shows: All books
  (the default) or Unshelved, books on no shelf. Either way shelves come
  first.
- **Drag to file.** A book card dragged onto a shelf card or a crumb is
  filed there, with a label under the pointer. From an open shelf it moves:
  it leaves that shelf, unless Alt is held, which adds it and keeps it
  (the label shows "+"). Dropped on the top level's crumb, it leaves the
  open shelf only. A shelf dragged onto a shelf or crumb moves inside it;
  into itself or its own shelves is refused ("A shelf can't go inside
  itself."). A book already there says so. Escape drops what's carried
  and stays on the page. Each drop says what it did in a toast.
- **Manual order.** Sort by Manual (First to last, Last to first) shows a
  shelf in its own order; books are added at its end, and dragging a book
  beside another on that shelf puts it there, with an accent bar where it
  lands. Elsewhere Manual reads as date added. Each membership carries a
  base-62 fractional key, so a move rewrites one record and another key
  always fits between two (`crates/library/src/key.rs`); a merge that
  leaves two keys equal or out of order still yields a usable one.
- **Breadcrumb.** On a shelf the bar's left shows Library › … › the last
  levels, and the centre title gives way. Past four crumbs the middle
  folds into a … menu; each name is cut at 160 pt. Crumbs open their level
  and take drops. The open shelf's crumb has Rename… and Remove shelf….
- **Removing a shelf** asks first: its books stay in the library, and its
  shelves move up to its parent (or the top), a clashing one renamed
  "Name 2". The page then shows the parent. The shelf and its memberships
  become tombstones, so a sync can't bring them back. A shelf open while
  removed elsewhere gives way to its nearest living parent.
- **Saved.** `library.json` moves to schema 3 with `shelves` and
  `members`; names, parents, keys and the alive flag are stamped fields
  (`docs/sync.md`). A parent that is removed, unknown or loops back counts
  as the top level, so a bad merge can't hide a shelf.
- **Not yet.** Several books at once (selection) and shelf menus for
  books come with slice 5; dragging onto panes with slice 7.

## Slice 5: selection, menus, removal and imports (2026-10-15)

- **Picking.** Ctrl-click (⌘ on macOS) picks a book or shelf card;
  Shift-click picks the run from the last pick, in the order shown;
  Ctrl/⌘+A picks everything on the page. While anything is picked a plain
  click picks too, instead of opening (owner decision). Picked cards wear
  an accent ring and a check. Escape, a click on empty page or Clear
  selection lets go; changing level or chip does too.
- **The selection bar** rises at the page's foot: "3 books · 1 shelf
  selected", then New shelf from these, Take off this shelf (on a shelf),
  Remove… and Clear. Dragging a picked card carries all the picks; the
  drop files them together and lets go.
- **Menus.** Right-click a book: Open (Find… when missing), Select,
  Rename…, Read separately, Show in Finder / Explorer / folder, Take off
  this shelf, Remove…. A shelf: Open, Select, Rename…, New shelf inside,
  Remove shelf…. Right-click inside a selection: the bar's actions for
  all of it. Empty page: New shelf, Import books…, Select all.
- **Removal receipt.** Remove… asks first and says what goes: how many
  books, the places and shelf spots kept, the store copies deleted and the
  folder books hidden. A book that is also somewhere else is only taken
  off; removing a shelf among the picks keeps its books.
- **What removing does** depends on where the book lives (owner decision):
  - in a folder: hidden. Its file is never touched, a rescan leaves it out,
    and Folders lists it under Removed · N with Restore and Restore all.
    Restored, it comes back with its place and shelves.
  - in the library's own folder (a store copy): the record becomes a
    tombstone and the copy is deleted once no other book lists it.
  - opened from elsewhere: the record goes; opening the file again brings
    the book back.
  The toast names what went and, for folder books, where to restore them.
- **Read separately** makes an independent twin ("The Raven (2)") with
  its own place, name and shelves, sharing the file. Opening a twin reads
  at its own place; the original keeps its own. Removing the original
  promotes the first living twin, so opening the file by path still finds
  a book.
  Which book a pane reads as travels with its open request into the
  pane's view, and is saved with the last-read file. A failed open
  leaves nothing behind, another pane on the same file is never taken
  over, and a twin reopens as itself after a relaunch. A pane reading a
  removed book stops recording its place rather than hand it to another.
- **Rename…** edits a book's title in a sheet: Enter or Rename saves,
  Escape cancels; at most 120 characters. Rescans and later opens keep
  it; the title is a stamped field for sync.
- **Imports are copies** (owner decision). Import… (the page header) and
  files dropped on the library copy books into `files/<content>/<name>` in
  the library's folder, onto the shelf shown, and record them as store
  locations. The copy is written to a `.part` file and renamed, so a
  crash leaves no half book. Content the library already has is the same
  book, not a second copy ("was already in your library"); files it can't
  read are named in the toast. Folders dropped are added as folders read
  in place, and Open… still reads a file where it is.
- **Reveal.** Show in Finder selects the file (`open -R`); on Windows
  Explorer selects it; on Linux the file manager's D-Bus ShowItems, else
  the parent folder opens. A failure says so in a toast.
- **Saved.** `library.json` moves to schema 4: `Origin::Store`
  locations, `twin_of`, and each folder's `hidden` set.
- **Not yet.** Search with slice 6; dragging onto panes and import
  progress with slice 7.

## Slice 6: search (2026-10-15)

- **One rule.** `mareader_library::Query` decides both what the page
  keeps and what the bar suggests. A query is split into words, and every
  word must turn up in the title, the author or the address. A word can
  turn up as a substring, or as an ordered subsequence that keeps its
  shape: `mthmtcl` finds "Mathematical", `tlhod` finds "The Left Hand of
  Darkness", while `dunee` does not find "Dune". Case doesn't matter, nor do
  Latin accents (`elan` finds "Élan", `zola` finds "Zoła"), curly quotes or
  long dashes. Nothing is indexed: the page's list is cached instead.
- **Scores.** A substring scores by its length, and more at the start of a
  word. A subsequence scores per letter: more for runs, less for gaps, and
  more at word starts. Words start after a space or mark, at a case step
  ("MobyDick") and where letters meet digits ("vol10"). Each word takes its
  best field: title ×3, author ×2, address ×1. Ties go to the book read
  last, then by name.
- **The address** is what the file's place says: the path inside its folder
  ("Scifi/1984.pdf"), or the parent and name of a loose file
  ("Downloads/paper.pdf"). A store copy's path names only its content, so
  it isn't searched.
- **The field** takes the centre of the library's bar where the reader's
  title sits. It reads "Search 12 books", or names the shelf or folder
  chip it searches. ⌘F or Ctrl+F focuses it with its text selected. With
  no document open, the find key opens the library and focuses its
  search. While typing, or while a search filters the page, the bar stays
  shown even if unpinned.
- **The page** filters as you type. At the top level a search looks through
  every book, shelved or not, whatever Library shows; a folder or Missing
  chip narrows it, and on a shelf it searches that shelf. Shelf cards are
  kept by name; at the top a search lists nested shelves too. The header counts "3 of 12 books". When nothing here
  matches, the page says so and offers "Show the N matches elsewhere" and
  Clear the search. Going into a shelf ends the search (it was found);
  going up keeps it.
- **Suggestions** list under the field: the best seven books and shelves
  from the whole library, each with its thumbnail or shelf mark, the
  matched letters picked out, and the author underneath (or the address,
  when that's what matched; a shelf's parents for a shelf). Down opens the
  list or moves down, Up moves up and closes it from the top row. Enter or
  a click shows the chosen one. Enter with nothing lit closes the list and
  keeps the filter.
- **Showing a choice.** A book visible where you are is ringed in place.
  Otherwise the page goes to the top level, where the search lists every
  match; a shelf chosen from inside another shelf opens its parent. The card is scrolled to the middle
  and ringed in the accent with a halo that pulses twice. The ring fades
  after four seconds; with reduced motion only the ring shows.
- **Escape peels** one layer at a time. In the field: the list, then the
  text, then the focus. On the page: the selection, then the search, then
  back to reading.
- **Kept fresh** by a revision number the model bumps on every change it
  shows, saved or not (a rescan, a presence check). The page's list is
  cached by revision, open shelf, chip, sort, Library setting and query.
  That also ends the per-frame sort of slice 2.
- **Shots.** `search=<text>` types into the field and `open=search`
  focuses it: `books=10 library=show search=essays open=search`.

## Slice 7: the Library panel, drops on panes, progress (2026-10-15)

- **The panel.** The reading sidebar's Library panel lists shelves, then
  books, by name (natural order, case folded). The top level shows what
  Home shows: every book or only unshelved ones. A shelf opens in place
  with its chevron, a click or Right, and folds with Left; each counts
  the books under it. Books carry a format badge and a dot when open in
  a pane; the focused pane's book is lit. A missing book is faded and
  opens the find-again sheet instead of a pane.
- **Tabs.** While the window is split, the panes are listed above the tree
  as tabs, named by the library (so twins differ) or the document. A
  click focuses a pane, × closes it.
- **A click** follows Settings › Panes › Library panel:
  Open in focused pane (default), Open as a new split, or Do
  nothing. A book already shown only focuses its pane. Beside splits to
  the right, or below when there is no room, and an empty focused pane
  takes the book instead. Do nothing still lets Enter open beside, so
  the panel works from the keyboard. The Library page follows the first
  two; with Do nothing it opens in the focused pane, since the page has
  no panes to drag onto.
- **A drag** from a book row onto a pane shows where it lands: the half
  the new pane would take, lit, named "Split right of Middlemarch". The
  edge is the nearest one, ties going right, then below, left, above. An
  empty pane takes the book whole ("Open here"). Letting go elsewhere, on
  the rail, or on the library page drops nothing. The rule lives in
  `mareader_workspace::DropTarget`, so it is tested without a window.
- **Progress.** A folder walk or an import that runs longer than 0.4 s
  gets a card in the bottom-left corner: a ring that spins while a
  folder is walked, then fills as files are read or copied ("Reading 4 of
  12", "Importing 3 of 12") with the file at hand underneath. One card
  per run; it leaves when the run ends, and the toast reports the result,
  so the card never needs closing. The worker sends a beat only when the
  ring moves a whole percent, so huge folders don't flood the queue.
- **Not yet.** Importing a mareader library is deferred (owner,
  2026-10-15); see § Optional below. Dropped (owner, 2026-10-16).
- **Shots.** `sidebar=library` opens the panel; `library_click=split`
  picks the click; `carry=x1,y1,x2,y2` holds a drag without letting go;
  `runs=demo` shows two cards: `pdf=sample:5 books=10 shelves=demo
  sidebar=library panes=2`.

## Redesign: the page as a home screen (owner, 2026-10-15)

The owner asked for a page that looks like mareader's, with less on it, where
cards behave like apps on a phone's home screen. The owner's answers:

- **Order.** One order arranged by hand, "Arranged", is the default. Title,
  Author and the rest stay as views; dragging in one returns to Arranged,
  which is never lost.
- **Shelves sit anywhere,** mixed with books, drawn as a 2×2 plate of
  covers like a phone's folder.
- **Storage moves to SQLite** (bundled), shaped for sync as `docs/sync.md`
  describes. This replaces the decision to keep plain files.
- **The list stays** for scanning and selecting; arranging, stacking and
  filing by drag happen in the grid.
- **Old code goes** when the new design replaces it; no parallel paths.

Slices, each checkable on its own (`ROADMAP.md`):

1. **Adding (slice 8).** The header keeps the title and count. A book-sized
   card with a plus ends the grid (an Add row ends the list) and opens one
   menu: import copies, add a folder, a new shelf, and Restore for books
   removed from the folder shown. Files picked are always copied in, even
   from inside a folder read in place, and come back with their data when
   that folder had removed them; only a folder reads in place. One
   folder sheet, as in mareader, holds every choice (owner, 2026-10-15):
   formats included or excluded, a size floor, how books are held (copied
   in, read in place, or read in place and watched) and folder structure
   (a shelf per folder, one shelf, or none). Watched folders file books
   found later on the same shelves; a folder shelf removed stays removed.
2. **Storage (slice 9).** Synced records are rows whose fields carry their
   own stamps; a removal is a tombstone that field writes never revive.
   Local facts (locations, folders) sit in their own table and never sync.
   Only records an edit touched are written, in one transaction off the UI
   thread. A change cursor lets a later sync ask what changed since.
3. **Arranged order (slice 10).** Every item at a level, book or shelf, has
   a sort key between its neighbours', so two machines' moves merge.
4. **A living grid (slice 11).** Cards ease to their slots on springs while
   a drag moves the hole; only cards on screen animate, and the page asks
   for frames only while something moves.
5. **Stacking (slice 12).** Resting a carried book on another for a moment
   makes a shelf of both, named and ready to rename; a shelf takes drops,
   and opens when rested on; crumbs take drops.
6. **Many at once (slice 13).** A long press selects; carrying a selected
   card carries all of them as a fanned stack with a count.

**Cheap on weak machines.** The grid lays out only rows in view, keeps
animation state only for cards drawn, allocates nothing per card per
frame, and stops repainting when nothing moves.

## Slice 9: storage on SQLite (2026-10-15)

- **One file.** `library.sqlite`, bundled SQLite in WAL mode, opened by the
  library's disk thread and kept open; the UI never touches it. Tables:
  `meta` (clock, last save number), `records` (synced: book, shelf,
  member, state) and `local` (locations, folders). A page turn writes one
  `state` row (owner, 2026-10-15: positions joined the store, so one
  cursor and one transaction cover everything a sync needs). Old
  `state/<id>.json` files move in once; the folder becomes
  `state.migrated`.
- **Only what changed.** The model's maps are `Tracked`: every way to
  change a value marks its key, so no edit can forget to save. A save
  turns the marked keys into rows and writes them in one transaction;
  renaming one book writes two rows, not the library. Whether a file is
  there is never saved, so presence checks mark nothing.
- **Sync-ready.** Each save numbers the rows it wrote (`seq`);
  `Store::changed_since` lists records past a number. `Merge` joins two
  copies of a record in any order. No sync option exists yet.
- **Moving in.** A `library.json` from earlier builds is read once, written
  to the store whole, and renamed `library.json.migrated`. A newer or
  unreadable `library.json` behaves as before.
- **Failure.** A damaged `library.sqlite` is kept aside as
  `library.unreadable.sqlite` and a new one started, with a note. A file
  from a newer Mareader is read but never written. Rows of a kind this
  build doesn't know are skipped silently; a row that won't parse is told
  once and left as it was, since saves only write rows they changed.
- **The browser preview** has no store; its library lives in memory.

## Slice 10: the arranged order (2026-10-15)

- **One order per level.** The top level holds shelves and every book; a
  shelf holds its shelves and its books. `Library::arranged` lists either
  as `Item`s (shelf or book), mixed. Home's Unshelved choice filters the
  top's list without changing it.
- **Keys, not indexes.** A shelf's key places it beside its siblings; a
  book's own key places it on the top level; a membership's key places
  it on that shelf. Each is a stamped field, so two machines that move
  different cards merge, and the later move of the same card wins.
- **Joining a level** takes a key after the level's last. The top's last
  key is looked up once and kept, so importing thousands of books never
  rescans. A moved shelf joins its new level at the end; a twin sits right
  behind its book, on the top and on each of its shelves.
- **Moving.** `Library::arrange` puts cards, in the order shown, before
  another card or at the end, with keys spread evenly between the two
  neighbours, so a hundred cards dropped in one gap stay short. A level
  saved before keys existed shows as it did (shelves by name, then books
  by date added) and gets its keys on its first move.
- **Sorts are views.** Arranged is the default and has no direction; Last
  read, Title, Author and Added still sort either way, shelves first. A
  saved `manual` sort reads as Arranged.
- **Dragging.** In the grid, unsearched and showing everything, a card
  dropped on another card's left or right half lands on that side; a
  shelf's outer quarters arrange too, while its middle files the card
  into it. A bar marks the gap. A drop made while sorted switches the page
  to Arranged, so it shows what the drop did. The list files by drag but
  never rearranges. A shelf's plate shows its first four cards as
  arranged.
- **One type.** The page's `Pick` is the library's `Item`; the shelf-only
  reorder action became `ShelfAction::Arrange`.
- **Shots.** `books=6 shelves=demo library=show` shows the mixed grid;
  `carry=x1,y1,x2,y2` holds a drag over a card's side to show the bar.

## Slice 11: a living grid (2026-10-15)

- **A hole, not a bar.** Carrying a card from the level shown lifts it
  (or the whole selection) out of the grid. A hole as wide as what's
  carried opens where it was, and the other cards close up around it.
- **The hole follows halves, after a rest.** Over a card's left half the
  hole moves just before it; over its right half, just after. It moves
  only once the pointer has rested there 0.18 s, so cards passed over on
  the way, such as a shelf entered from its far side, stay put. Empty
  space and the plus card leave the hole put; the last card's right half
  sends it to the end. A shelf's middle half takes the card in at once,
  ringed, and the hole stays put.
- **Letting go** lands where the pointer is, without waiting for the
  rest (`ShelfAction::Arrange`); back where it started, nothing is written.
  Off the grid (a crumb, the bar) the grid does nothing. Escape, or a
  drop elsewhere, flies the cards home.
- **Springs.** Each card drawn eases to its slot on a critically damped
  spring, so it never overshoots. Cards entering view start in place,
  and a new column count or width snaps every card. With Animations off,
  or the system asking for less motion, cards jump.
- **The ghost.** The carried card rides under the pointer where it was
  grabbed, a touch larger on a deep shadow, with a count for a selection
  and a plus while Alt adds a copy. When it lands, it flies from there
  into its slot. The list keeps the small label.
- **Edges scroll.** Held within 32 points of the grid's top or bottom,
  the grid scrolls that way, faster nearer the edge.
- **Cheap.** Springs live in one map keyed by card, kept only for cards
  drawn this frame. A frame allocates the list of cards left behind only
  while carrying. The page asks for frames only while a card moves.
- **Code.** `library/living.rs` holds the hole, springs and lift, and
  `library/view/grid.rs` draws the grid.

## Slice 12: stacking and resting (2026-10-15)

Each rest is one `Aim` on the lift, timed from when the pointer settled
on it; moving to another place restarts the clock.

| Rest on | For | Then |
|---|---|---|
| A card's outer quarter (any card's half, for a shelf carried alone) | 0.18 s | the hole moves beside it |
| A book's middle half, carrying a book | 0.3 s | the book sinks into a ringed plate; letting go makes a shelf |
| A shelf's middle half | 0.7 s | the shelf opens; letting go before that files onto it |
| A crumb | 0.7 s | that level opens; letting go before that moves it there |

- **A stack** (`ShelfAction::Stack`) makes the shelf at the book's place
  in the arranged order and files both there. Inside a shelf both move
  into the new one, Alt keeping the carried copy, like any filing. It is
  named after the books' one shared author when that name is free there,
  else "New shelf". A carried shelf goes in too; a shelf alone never
  stacks. Letting go on a middle without the rest only moves the card,
  by halves.
- **Carrying across levels.** A carry outlives the level it began on.
  On a level it didn't come from, its cards open a hole at the end (or
  where one already shows, as the top shows every book). Letting go
  files or moves them here first, then arranges them at the hole.
- **No surprise after opening.** The card under the pointer when a level
  opens takes no rest until the pointer moves 8 points.
- **Another level snaps.** Springs are kept per level, so cards shown on
  both levels don't fly between pages when a shelf opens.
- **Edges scroll only with room**, and only with the pointer inside the
  grid, so a carry held over the bar or a short grid costs no frames.
- **Code.** `Lift::aim` in `library/living.rs`; the crumb's rest is in
  `library/crumbs.rs`, kept in egui's memory with the frame it was seen.

## Slice 13: many at once (2026-10-15)

- **Long press picks.** A press held still for 0.45 s on a card or row
  picks it, as on a phone, and starts picking: plain clicks then toggle.
  Its release is no click, so the card stays picked and nothing opens.
- **Holding on lifts.** Held past a click's length (egui's 0.8 s), the
  press becomes a carry without moving, of the whole selection when the
  card is in it. Press, wait, then move: the cards are already in hand.
- **A fanned stack.** The ghost of a selection shows up to two cards
  behind the one held, turned a few degrees either way in their own
  tints, with the count badge. Landing, they fly to their slots fanned.
  The selection bar steps aside while they're in hand.
- **Together.** A selection makes room as one hole, stacks onto a book
  into one shelf (books and shelves alike), files onto a shelf, opens
  shelves and climbs crumbs as one carry, and lands in place anywhere.
- **Touch screens.** egui turns a 0.8 s touch into a right-click, so on
  a touch screen the hold also opens the card's menu; the desktop app is
  driven by mouse and trackpad.
- **Code.** `long_press` in `library/view.rs`; `fanned` in
  `library/view/grid.rs`.

## Routes: the reader and the library (2026-10-15)

The window shows one of two routes, `crates/engine/src/route.rs`'s `Route`:
the reader (panes beside the rail) or the library (the shelf alone). The
library replaces the panes without closing them, unlike mareader, which
closes the book. `Route::allows` is one exhaustive table of which commands
each route takes. The keymap skips bindings the route drops, so their keys
reach the page's own widgets. The app drops the same commands from any
other source, and the bar, rail and lights read the route, not flags.

**No rail in the library.** The sidebar's panels (outline, pages) serve
the focused document, which the library hides. The library draws no docked
or overlay rail, no edge reveal and no toggle. The rail keeps its state, so
going back finds it as it was. On macOS the bar's gutter hosts the lights.

**The library's bar** keeps only what acts on the shelf or the window:

| Item | Library | Why |
|---|---|---|
| Sidebar toggle | gone | No rail in the library (above). |
| Library button | pressed, "Back to reading", only while a pane is open | Panes wait behind the page; with nothing open there is no way back. |
| Title | the search field | The centre holds the library's search (slice 6); the breadcrumb (slice 4) takes the left. |
| Settings | gone | Settings are about reading; the gear lives on the reader's bar, as in mareader. |
| Search | gone | It finds in the focused document. The library's own search field replaces the title (slice 6). |
| Split down, Split right | gone | They split panes, which the library hides. |
| View & tools (☰) | replaced by View options | Its rows serve the panes: folded tools, view mode, zoom, the reader's shortcuts. Open… stays on the shelf, full screen on its key. |
| View options | in the ☰ slot | mareader keeps the shelf's View menu in its bar; the slot keeps Pin and Appearance where the reader has them. |
| Appearance | kept, without Page texture | The shelf has no page, as in mareader. The ink dial and per-pane switches go too. It edits the window's look, which the shelf wears. |
| Pin | kept | The library's own pin, on by default. |
| Window buttons | kept | The window's. |

**Keys in the library.** Zoom, page moves, view modes, the sidebar, Split
and Settings do nothing. Find focuses the library's search (slice 6). Open (⌘O) and full screen still work. The
View menu command (`open=menu` in shots) opens the library's own menu.
Escape and the Library button go back only while a pane has a document.
Entering the library closes the find bar and the settings window.

## UI (`crates/ui-egui`, M5)

- **Sections:** All books, Shelves (a tree), Sources (each folder's own tree,
  read-only), Missing, Recent.
- **Grid and list:** `ScrollArea::show_rows`, card size from the column
  setting; covers load lazily on a worker.
- **Selection:** click, Ctrl/Cmd-click, Shift-click, select all; a selection bar.
- **Drag:** `dnd_drag_source` with a `Vec<BookId>` payload; drop zones on shelf
  cards, breadcrumb crumbs, the shelf tree, and panes (open or split).
- **Menus:** `Response::context_menu` per kind (book, shelf, empty space).
- **Breadcrumb:** crumbs laid out with `Galley`; the oldest fold behind `…`
  when they don't fit.
- **Search:** a `TextEdit` with a suggestion `Popup`; arrows and Enter move and
  open, Escape clears.
- **Sheets:** removal receipt, find-again, import options as `Modal`s.
- **Import progress:** a corner card per running import.

## Optional: importing a mareader library (P2)

A one-time import of `mareader.library.v3` maps linked books to source
locations, stored books to store copies, shelves to shelves, and keeps
positions. Departed shelves become plain shelves; tombstones become `hidden`.

Dropped (owner, 2026-10-16): mareader2 is a separate app and mareader
never shipped, so there is no library to carry over.

## Size target

`crates/library` under ~2.5k lines with its tests; the library UI under ~2.5k.
If it grows past that, the model is probably carrying a case it shouldn't.

## Dropped on purpose

- **Shelves that mirror folders and are editable.** Replaced by read-only
  source trees plus user shelves.

## Rewrite: mareader's library behaviour (owner, 2026-10-18)

The library was rewritten to behave like mareader's, edge cases included,
on mareader2's own storage: SQLite rows, stamped fields and tombstones,
ready for sync. Where this section and an earlier one disagree, this one
holds; in particular folders are no longer read-only source trees (see
§ Dropped on purpose) but shelves cut from the folder, as below.

- **Folders.** One import sheet: formats (include or exclude), a size
  floor (strictly larger), Copy into the library, Read at place, or Read
  at place and watch, and a shelf per folder or one shelf for everything.
  Answers can differ per subfolder; the deepest answer wins. A copying
  folder is walked once and owes no watch. Each subfolder becomes a rung:
  a shelf the folder cut. Watched rungs rescan on launch and window focus;
  unwatched ones add nothing on a rescan, but an explicit import does.
- **The ledger.** A folder remembers what it placed, where it last saw
  each file, and a tombstone for every book removed from it. A tombstone
  beats everything on a rescan; an explicit import lifts it. Moving a
  book read in place off its folder's shelves stores a copy and writes a
  move-out log, so the folder doesn't place it again.
- **Restore.** A folder shelf's Add menu lists what can come back:
  removed books first ("removed 3 minutes ago · 12 MB", or "not there any
  more"), then books that moved to another shelf, which offer "Also show
  it here" or "Show it in …". A restored file lands on its old shelf, or
  the folder's root, whatever the folder's filters say.
- **Removal.** One sheet for books, shelves and links, itemised: reading
  position, highlights, cover art, the app's own copies, the shelves it
  was on. Switches, off every time: remove everything inside, and delete
  the highlights and reading position (kept by default, they come back
  if the file is imported again). The library's own copies are deleted.
- **Questions.** One queue and one sheet: a held name on import (open,
  add as new, make link), a move onto a held name (merge, replace, as
  new; or merge, make link, as new when the book just became a copy and
  the one there is the library's own), content already held, a file
  inside a tree read in place, a folder merging into a shelf, and a
  folder whose name a top-level shelf holds, asked before the walk.
  Cancel leaves every waiting question; "Apply to all" answers the same
  kind at once.
- **Re-imports.** A pick inside a tree read in place continues that
  tree. A subfolder imported on its own earlier folds back in when its
  parent is imported, and a different shape re-files the tree. An import
  that finds nothing new, or folds back, says so in a note whose close
  lights the shelf.
- **Copy ask.** Any gesture that takes books read in place off their
  ground asks first: copy and move, copy and take apart, copy and
  remove, put shelves back in their places, or let the folder make them
  again. The folder on disk is never touched.
- **Links.** A pointer at a book or a shelf, never a copy. It stands
  among the cards, moves like one, never collides with a name, and goes
  when its target does. Clicking it shows the target where it lives.
- **Duplicate.** A book duplicates as the library's own copy, marks and
  place included, under the next free name. A shelf duplicates as a
  shelf of the reader's own with fresh copies of every book once.
- **Badges.** A shelf says where its books live: On disk, Copied or
  Mixed, from its own books, else everything below, else its folder's
  way of holding them. A book link counts as its book. Watched shelves
  carry a dot.
- **The list.** A tree: a shelf row unfolds in place (click, Space, or
  resting a dragged card on it) and its folder button opens it. Search
  unfolds shelves only. Cards show plates two levels deep, with empty
  cells hatched.
- **The panel.** The reading sidebar's tree also lists book links under
  their own names; a book on two open shelves is two rows. Counts include
  links. A missing book is faded and does nothing: it is never opened or
  dragged, and its tooltip sends the reader to the Library page to find it.
- **Kept apart from mareader.** The progress card leaves when its run
  ends and the toast reports the outcome; mareader kept a finished card
  until dismissed. Twins stay independent ("Read separately").

## AI organizer (2026-10-19)

The AI can shelve loose books. How it plans is in `docs/assist.md`,
§ Workflows; this is what the library does with the plan.

- **Starting.** "Organize with AI" sits under New shelf in View options and
  on the page's right-click menu, and covers every loose book. On a long
  press, the selection bar's sparkle button ("Organize these with AI")
  covers the picked books.
- **Which books.** Only imported copies on no shelf whose file is there.
  Books read in place from a folder are never touched, even off every
  shelf. If none qualify, or no model is chosen, a toast says why.
- **Which shelves.** Every shelf goes to the model, folder shelves too, so
  the plan may file onto any of them. Shelves are never locked.
- **While it runs.** A corner card shows a ring, "Planning shelves…" or
  "Filing books…", the count and the model, with ✕ to stop. A pass of
  light crosses the books' covers and their captions read "Finding a
  shelf" (`docs/design.md`, Book covers); they open as usual, but they
  don't move. A drag won't
  lift them, a selection carries the others, and a menu move leaves them
  with a toast. One run at a time.
- **The plan.** A sheet shows the tree: existing shelves by their path,
  new ones tagged New, each with its count and first titles, and the books
  left on no shelf. Only Apply or Discard closes it. It waits behind any
  other sheet, as questions do, so another sheet can't lose it. If it
  comes in while the reader is reading, a toast says so.
- **Apply** makes the new shelves, parents first, and files each book.
  Books that changed meanwhile (shelved, removed or missing) stay put, and
  the toast counts them. A new shelf left empty isn't kept.
- **Undo**, on that toast, takes the books off again and removes the new
  shelves that are empty.

Code: `crates/engine/src/library/organizing.rs` (which books qualify, the run's
state, Apply and Undo) and `filing.rs` beside it (what the library asks, hears
and files), `review.rs` (the sheet), `runs.rs` (the card), and
`crates/ui-egui/src/ai.rs`, which turns the library's ask into the service's
keys and its news back. The library side never names `mareader_assist`,
so it builds for the browser preview too.

Tests: `crates/engine/tests/filing.rs`, the unit tests in `review.rs` and
`runs.rs`, and `crates/ui-egui/tests/organize.rs` (a run held and stopped, Apply then Undo,
Discard, the View options row, books read in place refused). Shots:
`organize=running` or `organize=review` with `copies=N`.

### Pausing and resuming (2026-10-19)

A run that fails, is paused, or is cut short by quitting keeps every
step it finished (how is in `docs/assist.md`, § Pausing and resuming).
The library shows where it stands.

- **Pause.** The running card's ✕ is "Pause organizing", not a cancel.
  The books are free again at once: they move, and a paused run holds
  nothing.
- **The paused card.** It sits in the same corner, on the library page
  only, so pages being read stay clear. It has a still ring at the share
  done, "Organizing paused", "3 of 8 steps · 24 books", **Resume**, and
  ✕ "Discard what's done". It stays after a restart, and hides while
  another run goes or a plan is up.
- **A finished plan not yet reviewed** (the app quit while it waited)
  shows "Shelf plan ready" with **Review**. Review opens the sheet
  without asking the model again, even if the model is busy or gone.
- **Failures.** The toast says what went wrong and offers **Resume**.
  Busy, offline and slow servers are first waited out twice.
- **Menus.** With a run paused, View options and the page menu offer
  "Resume organizing" with how far it got, and "Organize from the start".
- **Starting another.** The selection bar's sparkle button, or Organize
  with AI, first asks "Replace the paused run?", with three buttons:
  **Resume it** (or **Review its plan**), **Start over**, and Cancel.
  Start over replaces the run only once the model takes it, so a busy
  model loses nothing.
- **Resuming** asks about the paused run's books that are still loose.
  For a whole-library run, books imported or taken off shelves since
  then join too. If none are left, a toast says so and the run is
  dropped.
- **Discard.** Discarding the paused run, or the plan on the review
  sheet, gives a toast with Undo, which brings it back. Applying a plan
  ends the run for good.

Code: `organizing.rs` (`Paused`, `ask_resume`, the card's state),
`runs.rs` (the paused and ready card), `sheets.rs` (Replace), and
`app.rs` (`organize`, `resume_organizing`, `take_organized`). Tests in
`crates/ui-egui/tests/organize.rs`: a paused run waits on its card with its
books free, a new run while one is paused asks first, and a finished
plan waits for review while a failure offers Resume. Shots:
`organize=paused` or `organize=ready` with `copies=N`.

## Managing the library from the rail (2026-10-19)

The reading sidebar's Library tree manages the library too, compactly, so
there's no need to leave the book.

- **The "+" button** by the Library heading, and a right-click on blank
  space, offer New shelf, Import books…, Import a folder…, and the AI
  organizer: Organize library with AI, or, with a run paused, "Resume
  organizing" and "Organize from the start".
- **A shelf's menu:** New shelf inside, Import books here…, Rename, Show
  in Library, and Remove….
- **A book's menu:** Rename, Take off "shelf" (on a shelf), Show in
  Library, and Remove…. Show in Library switches to the library page,
  showing the item.
- **Naming in place.** Rename, F2 on a focused row, and a new shelf all
  show a name field in the row with the old name selected. Enter or a
  click away keeps it; Escape, or a blank or unchanged name, keeps the
  old one. Delete or Backspace on a focused row asks to remove it.
- **Dragging in the tree.** Books, links and shelves are grabbed and
  dropped on a shelf row to move there. Dropping on a book's row files
  next to it, on its shelf, and dropping on the heading or blank space
  moves to the top level. The target shelf lights up, and a chip says
  what happens: "Move to Fiction", "Also on Fiction" (Alt keeps a
  shelved book on its shelf too), "Move to the top level", or "Already
  here". Resting on a closed shelf opens it after 0.6 seconds, and
  holding near the tree's top or bottom edge scrolls it. A shelf never
  goes inside itself or its own shelves. Missing books aren't carried.
- **Books the AI is organizing** show breathing dots, as their cards'
  captions do. They open as usual but stay put, so moving one says why.
- **The AI mark (2026-10-22)** replaces that sparkle, and the one on the
  selection bar's Organize button (`docs/design.md`, Icons).
- **At the pane limit.** With 4 panes open the tree is still grabbable
  for moving. Dropping onto a pane's edge shows "4 panes are open.
  Close one to open another." and opens nothing. An empty pane still
  takes the book. Shelves never open a pane.

Code: `library_panel.rs` (`drop_action`, `tree_drop`, the menus and
`rename_row`), `shelving.rs` (`ShelfAction::NewIn`, `take_made`),
`menus.rs` (`organize_rows`, shared with View options) and `app.rs`
(`Action::PickInto`, `drops_from_rail`). Tests: unit tests for
`drop_action` in `library_panel.rs`, and in `crates/ui-egui/tests/panel.rs` a
book dragged onto a shelf row moves, a shelf never goes inside itself,
F2 renames in place, and at 4 panes the tree still moves books but
splits no more.
