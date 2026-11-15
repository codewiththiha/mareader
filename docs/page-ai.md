# Page AI: summarize, translate, simplify

A right-click on a page offers three tasks that act on that whole page:
**Summarize**, **Translate** and **Simplify** (plain English, about B1).
The result covers the page, and the page itself is one click away. The
owner set the behaviour on 2026-10-19; it is new to mareader2, so there is
no parity entry.

## Asking

- **Where.** Anywhere on a page except a selection or a mark, including on
  text when nothing is selected. Those keep their own menus.
- **No model yet.** The menu says what the tasks do and offers
  "Set up AI…", which opens Settings › AI.
- **Translate.** "Translate to X" uses the last language picked; "Other
  language…" (or "Translate page…" before the first pick) opens a list
  with a search field, recent languages first. A pick is remembered, with
  the last five.
- **This page or continuous.** Translate and Simplify follow their
  setting: ask each time (the default), this page, or continuous. Asking
  shows a second step with "This page", "Continuous" and "Remember my
  choice", which saves the answer as the setting. A task set to
  continuous says so beside its row.
- **Already running.** The same task on the same text just shows the run
  under way; it is not asked twice.

## The result

- **Over the page.** A PDF's result is drawn on the pane's paper colour,
  so it matches the theme, texture and blend mode, and fades in over
  0.3 s. The text underneath can't be selected or marked while it shows.
  A book's kept result becomes pages of the book (see below); a card
  covers its page only while words stream in.
- **Type.** A book's result is set in the pane's own typography, in the
  page's text column. A PDF's result uses the pane's font, scaled to the
  page, inside an 8% margin. PDF results are text only.
- **Fitting.** A PDF's result is exactly its page's size and moves with
  it: the wheel scrolls the book, never the result. A longer result,
  often a translation, is set smaller until it fits, down to 40% of the
  reading size (never under 6). One longer still is drawn smaller as a
  whole. A book's result is never shrunk: it takes as many pages as it
  needs.
- **Streaming.** Words appear as they come, set again at most every 0.4 s.
  A redo keeps the old result on the page until the new words arrive.
- **Bubbles.** Each result on a page gets a numbered bubble at the page's
  end side. A click shows that result; a click on the active bubble shows
  the page again. A turning arc marks a result being written, "!" a failed
  one, and a dot one made for another layout of the book (see below).
- **A bubble's menu** (right-click): show it or the page; redo; copy the
  text; delete. While writing: show as it comes, or stop. Failed: the
  reason, try again, or dismiss. It also says which model made the
  result, and when.
- **Failures.** A toast names the reason in the reader's words. The page
  goes back to the original, and the bubble keeps the reason.

## Type settings for a PDF's results

A PDF has no Fonts settings of its own. Once it has an AI page (a kept
result, or one being written), Settings › Appearance shows the type its
results are set in for that pane:

- **Typeface:** body font, size, weight and the stand-in families.
- **Spacing:** line, paragraph, word, letter and first-line indent.
- **Lines:** full justification and hyphenation.

The book-only rows (book layout, column alignment, column width, ink and
pictures) stay hidden: the page sets the measure. Size and weight are
what a result grows to. A result never outgrows its page, so past the
room its text shrinks to fit, as under Fitting above. Each change sets
the results on screen again. With independent text for each pane, a
PDF pane keeps its own type. "Pane text" shows when a split includes a
book or a PDF with AI pages.

## A book's results as pages

A book's kept result is laid into the book itself, in the pane's type:

- **Pages.** The result replaces its page's text, and the rest of that
  page stays as it was. One longer than a page runs on to a second or a
  third page, and the book gains those pages. The page bar counts them.
- **Back.** "Show original page", or a click on the active bubble, takes
  the result out, and the book has its own pages again. Deleting it does
  the same, and so does erasing the book's AI pages.
- **Redo.** The old result stays in the pages while the new one is
  written, with a turning bubble, and the new one replaces it when done.
- **Type.** Font, size and spacing changes set the result again with the
  book: bigger type, more pages. The result stays shown.
- **The rest stays put.** Laying a result in or taking it out keeps
  every other page as it was, with its rendered tiles and the reading
  spot. With book layout, a blank page keeps the spreads' sides.
- **Marks.** The book's text around a result takes marks as usual. The
  result's own words are new text, so they can't be marked.
- **Bubbles.** They sit on the result's first page. Its other pages
  offer the same menu.
- **Overlaps.** One result of a stretch of text shows at a time: showing
  another of the same text takes the first one out.
- **Panes.** Each pane lays in only what it shows. Two panes on one book
  share its pages when they show the same results in the same type. A
  closed pane's results go with it.
- **Place and bookmarks.** The reading position, progress and bookmarks
  count the book's own pages, so they hold when results come and go. A
  result's pages share its first page's bookmark.
- **Continuous mode** counts the book's own pages, so a run never asks
  for a result's pages.

## Continuous mode

For Translate and Simplify. The aim: pages are ready as the reader gets
to them, without paying for pages they skim past.

- **The pill.** A small pill at the pane's bottom, above the page bar,
  names the run ("Translating to Burmese"), shows a ring, and has ✕ to
  stop. Its tooltip says what it is doing in a sentence.
- **Staying on a page.** The ring fills only while the page's result is
  on screen: by default 25 s, 5–120 s in Settings. Scrolling past pages
  never fills it, because it starts again on each page and only once that
  page's result shows.
- **Asking ahead.** When the ring fills, the next pages are asked
  together, up to "pages at once" (default 5, 1–16), and never more than
  the model can run at once (its "at once" times its usable keys). After
  that it tops up as you read: each full ring asks only as many pages as
  keep that many ready ahead. So at most that many pages are asked or
  being written, and it never runs far ahead.
- **Pages already done** with the same task and text show their kept
  result instead of being asked again. Pages with no text are skipped.
- **Reading past it.** On a page beyond what was asked, but still near,
  the run asks that page once it has stayed still for a moment.
- **Going far away.** More than "pages at once" + 2 pages outside the
  run's pages pauses it ("· paused"). Once the page stays still for
  1.2 s, the pill asks "Continue translating here?" with Continue and
  ✕. Continue carries on from this page; ✕ stops until the next manual
  run. Coming back near the run's pages resumes it by itself.
- **Stopping.** ✕ on the pill, "Stop continuous mode" in the page menu, or
  a new run in the same pane. Pages ahead that were never reached are let
  go: their requests stop.
- **Ends.** A page that fails stops the run, and its toast says so; that
  page's bubble offers a retry. A new layout (the book's text size
  changed) starts the run again from the page shown. Closing the pane, or
  opening another book in it, ends the run. At the book's end the pill
  says so.
- **Reading ahead in PDFs.** Pages not yet on screen are read on a thread,
  so asking ahead never stalls a frame.

## Fewer tokens

- **Placeholders.** Code blocks, long inline code, pictures, tables and
  long URLs are sent as `[[c1]]`-style tokens with tiny hints, never their
  content. The answer's tokens are put back as the real thing; any the
  model dropped go back at their place in the source. Summaries keep only
  the ones they mention.
- **Verse.** Preformatted blocks are treated as code only if they look
  like code, so poems are still translated.
- **PDF clean-up.** Page numbers and running heads are dropped, and words
  split across lines by hyphens are joined.
- **Size.** A page sends at most about 16,000 characters. Summaries ask
  for 40–220 words. Blank or picture-only pages make no request.

## Kept with the book

Results are saved in the library (`crates/library/src/ai_pages.rs`), in
the same sync-ready rows as marks: ids, stamps and tombstones, so they
survive restarts and can sync later. A row holds the task (with its
language), the text range it was made for, a key of the source text, the
result, the model, and when. A new result for the same task and range
replaces the old one. Deleting tombstones it.

- **Books** anchor a result to its text range (block and offset), not a
  page number. After a re-layout, its bubble shows on the page that now
  starts that range. If the page no longer covers exactly that text, the
  bubble is marked as made for another text size and offers a redo.
- **PDFs** anchor to the page.
- **Files outside the library** keep their results for the session only.
- **Removing a book** keeps its results for when it comes back, unless
  "Erase highlights, AI pages and reading position" is on. Then every
  result and mark goes, even ones removed earlier and kept for Undo: their
  text is blanked on disk, leaving only the tombstone a sync needs. Nothing
  erased comes back when the same file is added again, and Undo can't
  revive it. A book with only AI pages still offers the choice.

## Settings › AI › Pages

At the end of the AI tab, after the models:

| Row | Default |
|---|---|
| Summarize, Translate, Simplify models | Same as Explain words |
| Summary style | Paragraphs (or key points) |
| Translate to | Picked when first translating |
| Translating, Simplifying | Ask each time |
| Time on a page | 25 s |
| Pages at once | 5 |

Since 2026-10-22 the models sit in their own AI models tab. Pages keeps the
rows above, and **More than one page** holds Time on a page, Pages at once
and Pages per request. A task without its own model reads "Same as AI
Dictionary", or None while that has none either.

## Where it lives

| Part | Path |
|---|---|
| Prompts, placeholders, reading answers, PDF clean-up | `crates/assist/src/page.rs` |
| The request, partials and retries | `Assist::rewrite` in `crates/assist/src/service.rs` |
| Page types shared with the UI | `crates/study/src/page.rs` |
| Saved results | `crates/library/src/ai_pages.rs` |
| Runs, results, bubbles' state | `crates/ui-egui/src/page_ai/mod.rs` |
| Continuous mode | `crates/ui-egui/src/page_ai/follow.rs` |
| A book's results laid into its pages | `crates/epub/src/splice.rs`, `Flow::region` |
| A page's text, and results back to blocks | `crates/engine/src/page_ai/extract.rs` |
| Typesetting results off the frame | `crates/ui-egui/src/page_ai/cards.rs` |
| Drawing cards, bubbles and the pill | `crates/ui-egui/src/page_ai/paint.rs` |
| The page and bubble menus | `crates/ui-egui/src/page_ai/menu.rs` |
| Settings | `crates/settings/src/ai.rs`, `crates/ui-egui/src/settings_window/ai.rs` |

## Tests and shots

- `crates/assist/src/page_tests.rs`: placeholders kept, dropped and
  invented; answers read leniently; PDF clean-up.
- `crates/engine/src/page_ai/extract_tests.rs`: a page's text and blocks.
- `crates/ui-egui/tests/page_ai.rs`, headless, through the real menus. Without
  a model the menu offers setting one up; with one, the three tasks. A
  picked language is remembered. Marks keep their own menu. A kept result
  covers the page until the original is asked for. Asking how, and
  remembering continuous. A task set to "this page" doesn't ask. The pill
  stops a run. Going far away pauses, then Continue carries on there. A
  five-second stay asks exactly "pages at once" pages ahead. A book lays a
  kept result into its pages and takes it out again, and bigger type sets
  it again over more pages. Deleting a laid result gives the pages back;
  a redo keeps it laid in.
- `crates/epub/src/splice.rs`: results laid in, kept apart and grouped;
  every other page kept.
- With `explain=canned`, page runs answer at once with the page's own
  text in capitals, so tests need no AI.
- Shot options: `page_ai=3:simplify` keeps and shows a result on page 4
  (0-based, a language tag translates); `page_ai=follow:simplify` starts a
  canned continuous run on the first page; `right=x,y` right-clicks once
  open; `wait=frames` runs that many frames before the shot. For example:

```sh
python3 tools/ci.py shots \
  "ai-pdf pdf=sample:20 workers=0 page_ai=0:simplify" \
  "ai-menu pdf=sample:20 workers=0 provider=ollama right=600,400" \
  "ai-ask pdf=sample:20 workers=0 page_ai=follow:simplify goto=15 wait=110"
```

## Picked pages

Pages picked in the thumbnails ([Thumbnails](thumbnails.md)) are asked
together: one request reads a group of pages and gives one result for the
whole group, unlike continuous mode, which asks a page at a time.

- **Groups.** Picks are taken in reading order and cut into runs of
  consecutive pages, at most **Pages per request** each (Settings › AI ›
  Pages, 1–20, 5 by default). Eleven pages at 5 go as 5, 5 and 1; a gap in
  the picks starts a new group. Apple's on-device model reads at most 2
  pages for a summary and 1 otherwise, so its groups stay small.
- **At once.** Groups run as many at a time as the model allows: its
  "at once" setting and the room left on its keys. With 2, the first two
  groups start together and the third starts when one is done. A model
  busy elsewhere puts the group back in line, up to three tries.
- **Progress.** The pill reads "Summarizing pages · 2/3" and fills as
  groups are done; its × stops the groups left. Any other failure stops the
  batch and the toast adds "The pages left weren't asked." A text size
  change stops it too, since the page numbers move. Groups that were all
  blank pages toast once at the end.
- **Kept.** A group that is already kept is shown, not asked again. Groups
  are saved like single pages, so they come back with the book. A redo
  replaces the group's one result.

### How a group shows

- **PDF.** The result card sits on the group's first page and fits, with
  the font shrinking as usual, into the room of all its pages together.
  Each later page shows the next part of the same result. If the result
  runs out first, the pages left show a note: "Pages 3–7 are summarized
  together, starting on page 3."
- **Book.** The result is laid into the pages in place of the group's text,
  as with one page, so the group's pages shrink to the result's length.
  While it is written, every page of the group is covered as on a PDF:
  blank paper, then the words so far running across the pages.
- **Show original page** on any page of the group gives every page of the
  group back. A group's bubble tip names its pages: "Summary · pages 3–7".
- A group covers any single-page result on its pages; showing a single
  page's result again takes the group off.

### Saving groups

A PDF group is kept as a `pages` span (first and last page); a book's
group is kept as a block span from the first page's start to the last
page's end. Older versions of Mareader skip `pages` rows they can't read
and keep them untouched.

### Tests and shots

- `crates/ui-egui/tests/page_groups.rs`: eleven pages go as 5, 5 and 1; a
  group's later pages are covered and Show original gives them all back;
  Pages per request sets the size and gaps split; a redo keeps one result;
  a book lays a group in, and covers all its pages while it writes; the
  thumbnails menu asks the picked pages and keeps them picked.
- Shot option `page_ai=2-6:summary` asks pages 3–7 as picked pages, with
  canned answers. Add `page_ai_hold=true` to stop runs at their words so
  far, to see the writing state.
