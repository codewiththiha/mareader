# Memory

> **Status:** M8 audit recorded; generated icon-atlas budget pending release measurement · **Last updated:** 2026-11-07

Every memory claim in this repository needs:
- what was measured (RSS peak, texture bytes, heap);
- how (tool and command);
- the workload;
- before and after;
- known limits.

Add a dated section per audit; never overwrite one.

## 2026-10-13: renderer baseline

- **What:** peak memory above the process baseline while rendering every page
  at 150 %, one thread.
- **How:** the harness in the earlier study (`bench/`, not in this repo).
- **Limits:** pdf.js and WASM ran in Node, not a webview, so pdf.js numbers are
  pessimistic for the GPU canvas path.

| Document | hayro native | pdf.js (Node) |
|---|---|---|
| tracemonkey, 14 pp | 12 MiB | 128 MiB |
| arXiv "Attention", 15 pp | 21 MiB | 269 MiB |
| PDF 1.7 spec, first 40 pp | 18 MiB | 56 MiB |
| scanned, 8 pp at 400 dpi | 49 MiB | 195 MiB |
| CAD, 4 pp × 170k paths | 118 MiB | 550 MiB |

## Budgets to hold (set at M2)

- **Idle window, no document:** measure it at M1 and record it here as the floor.
- **Closing the last pane on a document:** texture count and bytes return to
  their value before it opened (a kittest test).
- **The same book in two panes:** one parse, one tile set; the second pane adds
  only the tiles its own zoom needs.
- **Thumbnails:** LRU of 16, as in mareader.
- **Resting icon glyphs:** context-local cache capped at 4 MiB of accounted
  premultiplied RGBA pixels and 512 textures. One tightly cropped texture holds
  only the canonical first sample of one pack and one device-pixel size.
- **Animated icon atlases:** context-local cache capped at 8 MiB of accounted
  premultiplied RGBA pixels and 512 textures. One texture contains tight,
  deduplicated part variants for every 60 Hz sample of one pack and one
  device-pixel size. Atomic part scenes retain nested compositing without
  whole-frame cells. Driver overhead and release-build RSS remain to be
  measured; a pending or unadmitted atlas uses the generated resting glyph.

## 2026-10-13: M2 audit, reading and closing (slice 7)

- **What:** process RSS and its peak (`VmRSS`, `VmHWM` in `/proc/self/status`;
  the peak is reset after each row), with the app's own counts: open
  documents, cached page rasters and their bytes, text layers, queued jobs.
- **How:** `crates/ui/examples/memory.rs`, run as
  `capped.sh cargo run -q -p mareader-ui --example memory -- BOOK.pdf 12`.
- **Workload:** a 1200 × 800 pt window at 2 px per pt, two panes, 0 workers,
  scroll jumps landing at once. Open the book in the right pane, go to every
  page in turn (each drawn), close the pane. Repeat open, read and close in a
  new pane (rounds). Then open it in one pane and again beside it.
- **Limits:**
  - The GPU is lavapipe, a software Vulkan device. Its textures and its own
    startup live in process memory, so RSS here counts what a real GPU keeps
    in VRAM, and the idle floor is high.
  - It is a dev build (`opt-level` 2 for dependencies, 1 for ours, no LTO).
    That changes code size more than heap.
  - glibc keeps freed memory for reuse, so RSS after a close stays above
    idle. The rounds tell that apart from a leak.

| Step | tracemonkey, 14 pp | arXiv "Attention", 15 pp |
|---|---|---|
| Idle, two empty panes | 163 MiB | 164 MiB |
| Opened in one pane (3 rasters, 22 MiB) | 217 MiB | 220 MiB |
| Every page read (rasters: 104 MiB / 111 MiB) | 319 MiB, peak 319 | 339 MiB, peak 339 |
| Its pane closed (0 documents, rasters, layers, jobs) | 236 MiB | 250 MiB |
| Rounds 2 to 12, each read and closed | 254 to 255 MiB | 250 MiB (rounds 2 to 4) |
| Opened again, then in a second pane too | 261 MiB, then 261 | 250 MiB, then 250 |

Findings:

- **Closing the last pane on a book returns everything the app counts:**
  documents, rasters, text layers and jobs all reach 0, as the kittest
  `closing_the_last_pane_on_a_book_frees_its_textures` asserts.
- **No leak:** after a close, RSS levels off at 254 MiB over eleven rounds of
  reading all 14 pages. The difference from idle (about 90 MiB) is memory the
  allocator keeps, and the next book reuses it.
- **One parse for the same book twice:** a file is opened by its canonical
  path, and a pane opening a book another pane shows shares its document and
  rasters. The second pane added no document, no raster and no RSS. Before
  this audit it parsed the file again
  (`one_file_in_two_panes_is_read_once_and_drawn_once`).
- **A page jump draws only where it lands:** during a glide, pages flown past
  are neither rendered nor prefetched. Without this, going to page 200 of 300
  cached 22 pages instead of 2
  (`a_jump_glides_and_only_its_landing_is_drawn`).
- **Rasters dominate:** a fitted page at 2 px per pt is about 7.4 MiB, so the
  256 MiB cap holds about 34 such pages. A book under the cap stays whole once
  read, which is why "every page read" ends at 104 MiB of rasters.

## Tiles (M2 slice 9)

What was measured: the page cache's bytes in
`a_deep_zoom_draws_tiles_once_the_view_rests`, a three-page sample at 500 %
in a 900 × 700 pane without a GPU, where egui's texture side is 2048.

- **Before tiles:** a page past the texture side rendered at the capped
  scale and was stretched, so deep zoom was blurred.
- **With tiles:** the page draws a backdrop of at most 2048 px under tiles
  of 1024 to 2048 px. At rest the view held 2 tiles; after a pan, 4. The
  cache stayed at 58 MiB with every page and scale counted.
- **Bounds:** a tiled page is at most 16,384 px a side. Tiles share the
  256 MiB cap and go cold like pages. A page drawn at another scale drops
  its old tiles. A pan queues no tile until the view rests 0.25 s.
- **Threadless runs:** inline work queued by a demand now asks for a frame.
  Before, a test or the web preview could stop with jobs left queued.

## 2026-10-14: EPUB layout (M4 slice 7)

What was measured: resident memory (VmRSS, and VmHWM for the peak) from
`/proc/self/status` before and after `Book::open`, on CI's Linux runner.
How: `crates/epub/tests/layout_memory.rs`, an ignored test, run with
`cargo test -p mareader-epub --test epub -- --ignored --nocapture layout_memory::`.
Each workload is one generated chapter in bundled fonts.

| Workload | Units | Open time | Pages | RSS growth | Per unit |
|---|---|---|---|---|---|
| Prose, 1,000-character paragraphs | 4M characters | 0.92 s | 1,883 | +171 MB (peak 177 MB) | ≈45 bytes a character |
| `<p>w</p>` repeated | 200,000 paragraphs | 0.97 s | 9,524 | +523 MB (VmHWM 700 MB) | ≈2,700 bytes a paragraph |
| One paragraph | 2M characters | 0.41 s | 866 | +0 MB | unreliable: the allocator reused freed memory |

- **Why it grows:** a book is laid out whole when it opens, and each
  paragraph and table cell keeps its parley `Layout` for drawing and
  selection. The cost is per paragraph as much as per character.
- **The budget:** `Block::layout_cost` estimates 3,000 bytes a paragraph or
  cell plus 48 a byte of text (`LAYOUT_BYTES`, `CHAR_BYTES`), a little over
  what was measured. Before layout, a book estimated past 1 GiB
  (`MAX_LAYOUT`) is refused as too large
  (`a_book_past_the_layout_budget_is_refused`).
- **Real books:** *War and Peace* (3.2M characters) is about 190 MB; the
  complete Shakespeare about 560 MB. A 40M-character omnibus is refused.
- **Known limit:** a book near the budget holds most of it for as long as
  it is open. Laying out only the pages near the view, and dropping the
  rest, would bound it; that is the follow-up.

## 2026-10-14: EPUB layout on demand (M4 follow-ups)

What was measured: the same as the section above, with the same test, now
also reading every page's text after opening, which lays every block again
through the kept few. CI's Linux runner, VmRSS and VmHWM.

| Workload | Open | Held after opening | After reading every page | Peak |
|---|---|---|---|---|
| Prose, 4M characters, 1,883 pages | 1.39 s | +16 MB (was +171 MB) | +44 MB, read in 1.98 s | 54 MB (was 177 MB) |
| `<p>w</p>`, 200,000 paragraphs, 9,524 pages | 0.97 s | +59 MB (was +523 MB) | +78 MB, read in 1.21 s | 130 MB (was 700 MB) |

- **Why:** a set book keeps only each block's line boxes and its text; the
  parley layouts, about 3,000 bytes plus 48 a character each, are laid
  again for the pages drawn. At most 32 MiB of them are kept (`MAX_LAID`).
- **What stays:** about 4 bytes a character of prose, and about 300 bytes
  a one-word paragraph (its `Block` and its line boxes). `MAX_BLOCKS`
  (1,048,576) bounds that part near 300 MB; real books are far below it.
- **The cost:** a page not kept is laid again before it draws, a
  1,000-character paragraph in about half a millisecond. Reading all 9,524 pages' text took
  1.2 s.
- **Known limit:** each open book has its own 32 MiB of kept layouts.

## 2026-10-16: M8 audit, both formats and the library (slice 3)

- **What:** as at M2 (`VmRSS`, `VmHWM` reset after each row), plus open
  searches, cover texture bytes and egui's texture count. Each workload ends
  in a table of budgets, and a breach fails the run.
- **How:** `crates/ui/examples/memory.rs`, in CI on Linux as the Remote
  memory task (`tools/ci.py memory --ref BRANCH`), which also runs the EPUB
  layout test. Run 38919011226 on `a2a844c`; tables in the `memory` artifact.
  Modes: `memory BOOK [ROUNDS]`, `memory --library N` (N one-page PDFs read
  in place, in three folders), `memory --seeded N` (books without files).
- **Workload:** the M2 one, now searching each book for "the" before its
  pane closes, and drawing a second of frames after the close. The library
  is scrolled to its end by the wheel in the grid, then in the list,
  rendering every fourth turn as frames do.
- **Limits:** those of M2 (lavapipe counts VRAM as RSS; dev build; glibc
  keeps freed memory). The harness only submits GPU uploads when it renders,
  so a scroll that never renders holds every upload until it does (below).

| Step | tracemonkey, 14 pp | arXiv, 15 pp | The Raven, EPUB | War and Peace, EPUB |
|---|---|---|---|---|
| Idle, two empty panes | 184 MiB | 177 MiB | 177 MiB | 177 MiB |
| Opened in one pane | 243 | 231 | 240 | 247 |
| Every page read (rasters) | 357 (104 MiB) | 365 (111) | 517 (251) | 600 (251) |
| Its pane closed, a second later | 350 | 367 | 397 | 493 |
| Later rounds (2 to last) | 396 to 412 | 405 | 542 | 621 (one round) |
| Floor of the later rounds against the early top | −8 MiB | +0 | +1 | — |
| A second pane on the same book | +0 MiB | +0 | +0 | +0 |

| Library | RSS | Peak |
|---|---|---|
| Empty page | 186 MiB | 200 |
| 1,000 PDFs read in place | 217 | 231 |
| 1,000 more, second folder | 249 | 249 |
| 1,000 more, third folder | 258 | 258 |
| Grid scrolled to the end (401 turns; covers 63.3 MiB) | 410 | 410 |
| List scrolled to the end (450 turns) | 411 | 411 |
| 20,000 books without files, then scrolled | 255, then 332 | 332 |

Budgets, all held:

| Budget | Measured |
|---|---|
| A closed book leaves no document and no search | 0 and 0, every round |
| Later rounds' floor rises at most 16 MiB (no leak) | −8 to +1 MiB |
| A second pane adds under a quarter of the first open | +0 MiB of 54 to 70 |
| Page rasters within 256 MiB | 251 MiB at most (EPUBs) |
| Cover textures within 64 MiB | 63.3 MiB at most |
| Each book's share of the library, covers aside, at most 16 KiB | 3.3 KiB read in place, 3.4 KiB without files |
| A scroll to the end adds at most 256 MiB at its peak | +152 MiB grid, +0 list |

EPUB layout (`crates/epub/tests/layout_memory.rs`): 4 million characters
open in 1.39 s for +16 MB (peak 54); 200,000 paragraphs in 0.98 s for
+60 MB (peak 131).

Findings:

- **Covers no longer pile up when scrolled past.** Every cover asked for
  stayed waiting even after its card left the view, so a fling over 3,000
  books read and uploaded all of them. Now only covers asked for this frame
  or last wait, at most four load inline a frame, and they upload nearest
  first (`covers_scrolled_past_never_pile_up_waiting`).
- **A closed book's search let go of it late.** The find state kept the
  closed pane's document until the bar was used again; it is dropped on the
  frame the pane closes (`a_book_set_again_or_closed_lets_its_old_search_go`).
- **The 5 GB peak in the first run was the harness.** Run without rendering,
  egui_kittest writes each texture to the wgpu queue but only submits on
  render, so all 3,000 covers' staging buffers waited at once. Rendering
  every fourth turn, as a real frame does, brought the grid's peak from
  4,981 to 410 MiB with the same covers.
- **The first books carry fixed costs.** The first 1,000 books in place
  cost 31 MiB and the second 1,000 a further 32 (28 and 26 KiB a book,
  covers aside): SQLite's page cache and the memory kept after scanning.
  The third thousand cost 9 MiB, 6 of them covers: 3.3 KiB a book, as
  without files.
- **After a close, RSS stays higher than at M2** (350 against 236 MiB for
  tracemonkey; rounds 396 to 412 against 254). The app's counts reach zero
  and the later rounds stay level, so this is not a leak. A second of
  drawn frames returned nothing, which rules out wgpu's deferred frees. The
  difference is what lavapipe's allocator and glibc keep since the glass,
  the search and EPUB landed. On a real GPU that part is VRAM, and the
  rounds show it reused.
- **Seeding the library adds books** instead of renumbering the same ones,
  so `books=N` can grow a library in steps.

## 2026-10-19: Markdown books with emoji on a Mac (fix/reflow-memory)

- **Report:** a six-page `.md` book grew to 1.5 GB while scrolled on a Retina
  Mac and barely shrank; `.txt` books did not. CEFR shading was off.
- **What:** the Remote memory task's RSS rows (as at M8) for a new workload,
  and a raster probe's RSS after each round of drawing.
- **How:** `memory --markdown 6` (four real workers, outline open, the
  README wheeled down and up six times, a row per pass), and
  `markdown_drawn_again_and_again_holds_level` in
  `crates/epub/tests/layout_memory.rs` (every page at 2, 3 and 2.5 px per
  unit on four threads, twelve rounds). Runs on `6f295f2` (before) and
  `9983d42` (after). CI installs `fonts-noto-color-emoji`.
- **Workload:** `mareader_epub::sample::markdown(6, true)`: per section,
  emoji in headings and text, a task list, a code block, a table, an alert,
  a quote and a numbered list.
- **Limits:** Linux's colour emoji file is about 10 MB and is picked once,
  so CI shows the path, not the Mac's size. The cause below is from the
  code; the numbers show nothing else grows.

| Step | Before (`6f295f2`) | After (`9983d42`) |
|---|---|---|
| Idle, two empty panes | 183 MiB | 183 MiB |
| Opened in one pane (3 rasters, 12 MiB) | 267 | 253 |
| Scrolled down and up, passes 1 to 6 | 294, 281, 286, 286, 286, 286 | 286, 303, 298, 298, 298, 310 |
| Floor of later passes against the early top | −8 MiB | −5 MiB |
| Pane closed (0 documents, rasters, jobs) | 286 | 310 |
| Raster probe, emoji, rounds 4 to 12 | 129 to 159 MB | 129 to 205 MB |

Findings:

- **The leak was the UI's machine fonts.** When egui's own faces lack a
  letter (emoji, U+FE0F, ZWJ), `machine_fonts.rs` asks parley for the
  machine's face and adds it to egui. `label::fallbacks` copied the whole
  font file (`bytes.to_vec()`), and keyed it by a blob id that is new for
  every ask, since each ask built its own source cache. So each batch of
  new letters on screen added another copy under a new name. On a Mac
  that face is Apple Color Emoji, about 200 MB. egui also clones owned
  font bytes each time it rebuilds its fonts (`blob_from_font_data`), so a
  batch held two copies. Three or four batches reach 1.5 GB.
- **Fixed:** a face is keyed by its PostScript name, and its mapped file is
  kept once for the process and lent to egui as `&'static [u8]`. egui
  borrows it, nothing is copied, and mapped pages are clean file memory
  the system can drop (`a_machine_face_is_kept_once_and_named_the_same`).
  `Fonts::system()` jobs now share one source cache, so a font file is
  mapped once rather than once per layout.
- **Drawing pages does not leak.** The raster probe and the scroll passes
  stay level before and after, emoji or not. The spread between rounds is
  the allocator's reuse.
- **A page raster is no longer copied** at the end of a draw: reflow, PDF
  and label pixmaps hand their buffer over (`take_rgba8`). That was a
  second page-sized buffer per worker, up to 47 MB each at Retina scale.
- **CEFR shading grades only pages on screen,** memoised per page (96 at
  most) with 8,192 word levels cached; it never reads the whole book.
- **Every other cache was checked and has a cap:** page textures 256 MiB
  (thumbnails share it), finished jobs 32 MiB, text layers 96 pages, laid
  paragraphs 32 MiB a book, decoded pictures 64 MiB a book, covers 64 MiB,
  searches 3, shaped labels 96.

## 2026-10-22: Books at rest and after closing (fix/epub-memory)

- **Why:** a book's memory at rest was often larger than a PDF's.
- **What:** as at M8, plus counts the reflow crate now keeps
  (`mareader_reflow::held`): set books (flows) alive, their laid blocks and
  their decoded pictures, as a column of every row.
- **New steps:** after the search, four quiet seconds ("left alone"); six
  text sizes and four window sizes (each sets the book again); the page
  grid shown.
- **How:** `tools/ci.py memory`, CI's Linux runner with lavapipe. Before:
  run 39755733368 on `e1236d2` (the counts alone). After: on `11df8cc`.

| Workload, step | Before | After |
|---|---|---|
| War and Peace, its pane closed: flows alive | 1 (25.1 MiB laid) | 0 |
| War and Peace, opened again: flows alive | 2 | 1 |
| The Raven, its pane closed: flows alive | 1 | 0 |
| War and Peace at rest: page textures, text layers, laid | 251 MiB, 96, 16.0 MiB | 57 MiB, 2, 4.0 MiB |
| The Raven at rest: page textures, text layers | 251 MiB, 46 | 57 MiB, 2 |
| tracemonkey at rest: page textures, text layers | 104 MiB, 14 | 59 MiB, 1 |
| Six text sizes and four window sizes: flows alive | 1 | 1 |

Findings:

- **The leak was the page grid's picks.** They kept the last book read
  (`Picks::counted`) so picked pages could be found again in a new
  typesetting. Nothing let it go, so a closed book stayed whole, with its
  blocks, layouts and pictures, until another book was read. Now the
  picks hold it only while pages are picked, and an empty pane lets go.
- **Setting a book again does not pile up.** Ten re-sets in a row leave
  one flow; the old ones go with their last pane.
- **Why a book looked larger than a PDF:** per page they cost about the
  same (8 MiB of texture at this size against 7.4). A novel has hundreds
  of pages, so reading fills the 256 MiB texture cap, where a 14-page PDF
  stops at 104. On top came up to 32 MiB of laid blocks and 96 text
  layers, which only a page being drawn needs.
- **Fixed: caches rest.** Three quiet seconds after the last input or
  job, page textures go down to 64 MiB, text layers keep only the pages
  shown, and each book keeps 4 MiB of laid blocks and 16 MiB of decodes
  (`a_resting_book_keeps_only_its_newest_layouts`,
  `at_rest_cold_pages_go_and_the_pages_in_view_stay`). Pages in view and
  their prefetched neighbours are touched every frame, so they stay.
- **The layout measurement ran alongside another.** The two ignored tests
  in `layout_memory.rs` shared one process, so 4M characters read +186 MB.
  Each now runs alone: +18 MB to open (M4: +16), peak 59 MB.
- **Known limit, measuring:** the runner's RSS does not follow freed
  textures. Lavapipe keeps texture memory in large blocks and wgpu frees
  late, so RSS stayed level at rest and on closing, before and after,
  while the counts above fell. On a Mac, Metal frees each texture on its
  own; check the footprint in Activity Monitor.
- **Known limit, blocks:** 200,000 one-word paragraphs now hold +154 MB
  (807 bytes each; M4: +59 MB). Most likely the book-design fields added
  since M4, which every block and set line carries even when unused. A
  novel of 20,000 paragraphs pays about 16 MB; moving those fields into one
  boxed, often absent part would bound it. Not yet measured field by field.

## 2026-10-22: A paragraph's block and set (perf/block-size)

- **Why:** the known limit above: 200,000 one-word paragraphs held 807
  bytes each, against about 300 at M4.
- **What:** the ignored `layout_memory.rs` test, each run in its own
  process, and the full audit as before. Sizes came from `size_of`.
- **How:** `tools/ci.py memory`, CI's Linux runner. Before: `11df8cc`
  (the section above). After: run 39769928578 on `f9ee4b3`.

| Measure | Before | After |
|---|---|---|
| `Block`, one per paragraph the book keeps | 392 B | 264 B |
| `Set`, one per paragraph a set book keeps | 376 B | 200 B |
| 200,000 paragraphs: open | +154 MB, 807 B each | +93 MB, 488 B each |
| 200,000 paragraphs: read through, peak | +171 MB, 282 MB | +112 MB, 189 MB |
| 4M characters: open, peak | +18 MB, 59 MB | +18 MB, 58 MB |
| War and Peace: every page read, at rest | 622, 642 MiB | 606, 633 MiB |

Findings:

- **Empty lists cost 24 bytes each.** A text kept seven `Vec`s (spans,
  links, pictures, ruby, faces, combined runs) and a frame two (bars,
  boxes), nearly always empty. They are now `ThinVec`s: 8 bytes, no
  allocation while empty, the same methods.
- **Design parts nobody uses rode on every set block.** Its grid cell,
  float, grid place and positioned box took 136 bytes inline; they are
  boxed now, 8 bytes each when absent. Its line counts are `u8` and
  `u16`, and a picture's links and a table's head rows are thin too.
- **The block list kept its spare room.** Grown by pushing, 200,000
  blocks sat in room for 262,144; it is shrunk before it is shared.
- **Guarded:** `a_paragraphs_block_and_set_stay_small` fails if either
  grows past these sizes.
- **Known limit:** 488 bytes is still above M4's 300. What is left is
  each paragraph's own text and its line tops and offsets, three small
  allocations, plus the frame's look, cell and marker. Merging the line
  tops and offsets would save about 50 bytes more.

## 2026-10-22: Copies while drawing, and the search index (perf/copies)

- **Why:** a pass over code that copies or keeps more than it needs:
  hard-word shading, marks, highlights, page AI and search.
- **What:** the full audit as before, run twice because RSS on the
  runner wanders by about 15 MiB between runs. Sizes are computed.
- **How:** `tools/ci.py memory`, CI's Linux runner. Before: run
  39769928578 on `f9ee4b3` (the section above). After: runs 39774922698
  and 39777829841 on `cac57e6`.

| Measure | Before | After (two runs) |
|---|---|---|
| War and Peace: every page read | 606 MiB | 615, 587 MiB |
| War and Peace: searched for "the" | +27 MiB | +2, +18 MiB |
| War and Peace: at rest, then closed | 633, 524 MiB | 619, 492; 605, 470 MiB |
| tracemonkey: rounds 2 to 12 | 421 to 440 MiB | 425 flat; 14 MiB lower by round 12 |
| Search index, per character kept | 4 B, plus spare room | about 0.3 B |

Findings:

- **Every frame copied boxes.** Marks, hits, read-aloud and hard words
  each built a `Vec` of boxes per line, twice; a noted mark boxed every
  character of its page. Line boxes now stream from the text layer.
- **Hard-word shading kept each word's boxes apart.** The words of a
  shaded page now share one list behind an `Arc`, each plan reads the
  page's characters once, and the memo of 32 pages drops the least
  recently used instead of starting over.
- **Each pane listed its marks from scratch every frame,** scanning
  every mark in the library, and page AI cloned its index and scanned
  every result. Tracked maps now carry a stamp that moves on any
  change; lists are rebuilt only when it moves, then shared.
- **The search index kept a `u32` per character** pointing into the
  text layer: 13 MB for War and Peace. It is now runs that count up
  together plus the gaps' places, and each page drops its spare room.
- **Guarded:** `every_change_moves_the_stamp_and_reads_do_not`,
  `every_mark_change_moves_the_marks_stamp`,
  `listed_marks_are_shared_until_one_changes` and
  `the_layer_map_reads_as_a_list_in_a_few_runs`.
- **Noise:** the first run's emoji Markdown and library grid rows came
  out 15 to 30 MiB higher; the second run matched the earlier ones.
  Neither workload runs the changed code.

## 2026-10-24: An egui app's memory, and macOS giving it back

- **Why:** to find memory handling carried over from a web front end that
  an immediate-mode app does not need, check whether virtual lists help in
  egui, and make macOS let go of memory while reading and scrolling as
  Linux and Windows do.
- **How:** `tools/ci.py memory`, now on two runners. Linux as before
  (lavapipe). macOS on a real GPU, reading the footprint Activity Monitor
  shows (`footprint`), each book run as the app runs, with
  `MallocLargeCache=0`, and tracemonkey once more with macOS's default
  allocator for comparison. Before: `4942826`. After: `b66b326`.

What an immediate-mode app needs, and what was checked:

- **No retained widget tree.** egui rebuilds the frame from the app's own
  state, so there is no DOM, style engine, script heap or second process to
  feed. egui's own share is a few MiB, mostly the font atlas. Idle, two
  empty panes, the app's footprint on macOS is **27 MiB**.
- **Nothing web-shaped was found.** Page rasters go straight to GPU
  textures and the CPU copy is dropped; no images travel as PNG or base64;
  JSON is only for settings, export and AI replies. The things a web front
  end needed (frames, message passing, measuring the DOM, pdf.js latches)
  were never carried over (ADR 0005).
- **Virtual lists stay.** egui's `show_rows` and `show_viewport` lay out
  and paint only what is in view: library grid and list, the tree, find
  results, thumbnails and the outline. That saves work each frame, not
  memory egui would keep; the data behind them is capped on its own. The
  highlights panel lays out every row (rows differ in height) and costs
  only while open; the word list stops its preview already.
- **Kept, as they help an egui app:** the byte-capped caches that rest
  after three quiet seconds, the upload limit per frame (no frame stalls),
  waiting for a resize to settle before drawing pages again, and machine
  fonts lent to egui from mapped files, never copied. egui's URI image
  loaders keep every image forever; here they hold only a few small logos.

| Workload, step (MiB) | Linux before | Linux after | macOS, default allocator | macOS, as the app runs |
|---|---|---|---|---|
| tracemonkey, every page read | 372 | 370 | 383 | 133 |
| tracemonkey, left alone four seconds | 365 | 350 | 443 | 91 |
| tracemonkey, its pane closed | 366 | 383 | 414 | 33 |
| tracemonkey, after six rounds | 426 | 438 | 972 | 33 |
| War and Peace, every page read | 605 | 598 | — | 326 |
| War and Peace, left alone | 621 | 558 | — | 131 |
| War and Peace, its pane closed | 493 | 493 | — | 40 |
| The Raven, left alone | 541 | 518 | — | — |
| Attention, left alone | 371 | 341 | — | — |
| Library of 3,000, list scrolled to the end | 450 | 429 | — | 108 |

Findings:

- **macOS kept what the app freed.** Its allocator holds freed large blocks
  (page rasters, pictures, decoded fonts) as its own until the machine is
  short, and counts them in the app's footprint: six rounds of reading and
  closing a 14-page PDF reach 972 MiB with nothing open. The app now starts
  itself with that cache off (`apps/mareader/src/main.rs`); the same rounds
  end at 33 MiB, and memory follows the caches down as pages scroll away,
  rest and close. The setting is read only at start, hence the relaunch.
- **Resting caches now hand memory back.** A second after caches rest, a
  short thread asks the allocator to return free pages
  (`mareader_memory::give_back`: `malloc_trim` on Linux, pressure relief on
  macOS; Windows returns it already). On Linux, books at rest are 15 to 63
  MiB lower and the library 21 MiB.
- **Known limit, measuring:** on Linux the rows right after closing hardly
  move, as before: lavapipe keeps texture memory in large blocks. macOS
  frees each texture, so its rows show what the app holds.
- **Known limit, cost:** without the large-block cache, macOS gets that
  memory from the system again each time. Others report 4 to 8% slower
  allocation-heavy work; not yet measured here.

## 2026-10-31: Progressive EPUB opening

A saved-place open publishes one bounded, target-centered preview of up to
seven spine items, falling back to five when its markup or block budget is hit.
Once the target raster is ready, one continuation parses the remaining spine
and publishes the completed layout; no widening prefix snapshots are published.
Missing scroll rasters remain blank, without a per-page loading animation.

The remote memory task also runs a generated 800-chapter EPUB with one worker,
a saved location at chapter 400, and one adjacent-page raster request during
continuation. Its table records preview, target-raster, scroll-raster and full-
completion latency, plus process RSS above the generated-input baseline. These
are scheduler and whole-book measurements, not device-wall-time guarantees.
PDF page-tree enumeration remains eager, with rasters requested on demand.

## 2026-11-06: generated animated-icon part-atlas bound

- **What changed:** active icon animation no longer retains independently parsed
  SVG frames. `mareader-animated-icons` generates atomic top-level part scenes,
  rasterises their tight state variants and packs them into one texture per pack
  and device-pixel size. `mareader-ui` composes each sample as ordered egui mesh
  quads.
- **Admission bound:** both one generated atlas and the context-local sum of
  retained atlas pixels are capped at 8 MiB. The cache also remains capped at
  512 entries. Generation is lazy; all 104 packs and DPI sizes are never
  allocated eagerly. Exhaustive atlas tests require every current pack to fit
  the bound without whole-frame cells.
- **In-flight bound:** the native queue has two jobs and one raster worker, while
  a context retains at most one completed pixel atlas awaiting publication. A
  threadless build generates synchronously. The generated resting glyph remains
  visible while work is pending or storage cannot be admitted.
- **Limits:** these are byte-accounting rules, not RSS or driver measurements.
  Texture-manager overhead, upload staging, temporary generation buffers and
  release-build RSS still need the performance harness and a real GPU run.

## 2026-11-07: generated resting-icon bound

- **What changed:** stock idle icons no longer use `egui-lucide`. The same typed
  scene used by animation evaluates only its canonical first sample, composites
  it once, tightly crops transparent margins and uploads one neutral-white
  texture. A virtual-canvas UV keeps the 24-unit margins while drawing one quad.
- **Admission bound:** the context-local resting cache is capped at 4 MiB and
  512 `(pack, device-pixel side)` entries. A single cropped glyph is bounded by
  the existing 192-pixel side ceiling. Idle display never builds an animation
  atlas.
- **Limits:** generation is synchronous on the first use of each pack and size
  so the glyph is never blank. Release startup time, texture-manager overhead,
  cache churn across DPI changes, RSS and driver bytes remain to be measured.
