# Study mode

Study mode turns reading into vocabulary study. It comes from the lab's
`best-study-mode` prototype and is built in slices; this page describes what
has landed. Parity entries: `study.mode-proactive`, `study.mode-reactive`,
`study.space`, `study.mode-ai-cards`.

- **Proactive:** before each batch of pages, its hard words come up as cards.
  *Read now* unlocks the pages whenever the reader likes.
- **Reactive:** read freely, and recall what a shaded word means when you
  click it. The AI judges the answer.

## The rules (`crates/study/src/mode`)

Everything below is pure: the engine feeds it pages, levels and answers.

### Batches (`batches.rs`)

A batch is whole pages, and is stored as a text range, so a re-typeset keeps
it. A word is never studied after its page: a deck holds the words of its own
pages.

- **Words mode** (the default; an old *characters* setting reads as it)
  counts each page's hard words and breaks the pages the way line breaking
  spaces a paragraph's words (`even`, after Knuth and Plass). It keeps the
  sum of (words − Words per deck)² least over all the pages, not just the
  next batch, and among equal sums takes the fewest batches. At 12, pages of
  11, 2, 11 and 2 words make two batches of 13, not four. A batch of several
  pages stays under 1.5 times the target, the card requests' rule
  (`chunks`); a page with more stands alone. A batch spans up to 20 pages
  (`MAX_WORD_PAGES`). A word on several of its pages counts once; met again
  in a later batch, it counts again.
- **Pages mode** counts pages that carry text, 1 to 10 (`MAX_PAGES`).
- **In pages mode, light pages** (under 140 characters: a blank leaf, a lone
  heading) don't count and ride with a neighbour, and **a short tail** (under
  40% of the batch before it) merges into that batch, as long as it stays
  within 10 pages.
- **On a re-typeset,** unlocked progress rounds up to the end of its batch, so
  unlocked text never locks again.
- **Turning the mode on mid-book** starts the first batch at the page shown
  (`plan_from`). The pages before it read freely unless the lock is on. The
  start is kept as a book spot, so a re-typeset keeps it.

### Keys (`keys.rs`)

One card stands for one key: a lemma and its coarse class (noun, verb,
adjective or adverb).

- **The lemma** comes from folding common endings (`-ed`, `-ing`, `-s`,
  `-ies`, `-er`, `-est`, doubled consonants) against the words the level data
  knows. `acknowledged` becomes `acknowledge` only when `acknowledge` is known
  at the same band. A base at another band is another word, so `after` never
  becomes `aft`. The CEFR store stays as it is.
- **The class** comes from the Penn tag of the dataset's sense.
- **Proper nouns get no card:** a capitalised word inside a sentence reads as
  a name.

### Decks (`deck.rs`)

- **What makes a card:** the words the book's CEFR shading marks above *My
  level*, by the same rule (`mareader_cefr::word_band`). A name inside a
  sentence is the one shaded word left out. One card per key, with its
  count ("appears 3×") and the book's sentence of its first sighting.
- **Also as:** the same lemma in another class is its own card, linked by
  "also as".
- **Memorized words** are dropped when the deck is built, so a cached meaning
  never brings one back.
- **Order:** by count, then band, then reading order. A deck holds every
  word of its pages; 240 (`MOST_CARDS`) is a guard, not a cut.
- **Parts** (`open.rs`): a deck over 1.5 times *Words per deck* (the
  `chunks` rule) comes in parts of about that many cards, back to back
  before its pages. Words mode makes one only for a crowded page. Each part
  has its own time; the pages chip reads `4 – 6 · 1/2`, and the primary
  button says *Next part* (no ask) until the last part, where it is *Read
  now* again. A part whose time is spent turns to the next. Finishing saves
  every part's cards as learning, and a deck kept to open again holds them
  all.
- **The allowance** (`Allowance`): with *Time limit on cards* on (the
  default), a deck has *Seconds per card* (25 by default, 5 to 120) for
  each card it opens with: 12 cards at 25 s get 5:00, 20 at 10 s get 3:20.
  Its time runs only while the deck shows and is kept across visits.
  Nothing waits on it: *Read now* works at any time. The old fixed
  *Minutes per deck* key is let go on load.

### Recall (`recall.rs`)

- **Verdicts:** correct, close or wrong.
- **Tries:** up to three answers are judged. A hint shows after the first
  miss.
- **I don't know** shows the meaning and saves the word as missed.
- **A correct answer** can ask "Add to memorized?".
- **A failed judge** costs no try.

### What each mode needs (`mode.rs`)

| Mode | Needs |
|---|---|
| Proactive | word levels, an English book, and an AI model **or** an English dictionary pack |
| Reactive | word levels, an English book, and an AI model |

*Cards from* picks Auto (AI first), AI or Dictionary, and counts only while
both are there: with just one, that one writes every deck. The AI counts
once its writer is wired, so `mareader-cli`, which has none, uses a
dictionary pack. With neither, a deck fails and says what it needs; no card
is made without a meaning.

## What is kept (`crates/library/src/study_words.rs`)

- **Words** (kind `word`): one per lemma and class for the reader, in every
  book. The id is a hash of both, so two machines agree unasked. Status is
  learning, memorized or missed; a card never lowers memorized.
- **Sightings** (kind `sighting`): a word met in one book, with its form,
  sentence, meaning, who asked it and the last tries. The id hashes the book
  and the word.
- **The gate** is `unlocked` on the book's position record: the text read
  past its cards, as a `Spot`. Older builds keep it as an unknown field.
- **Removal:** forgetting a word is a tombstone that Undo lifts. Erasing a
  book's data takes its sightings and gate; the words stay the reader's.
- **Sync:** both kinds are synced records and merge per field, newest wins.

## In the session (`crates/engine/src/study_mode`)

Each pane runs its own mode; `study.mode` turns it on and works the deck
(`toggle`, `off`, `start`, `reopen`, `known`, `undo`, `next_batch`, `retry`,
`use_dictionary`, `skip_rest`, `read_previous`, `recall`).

- **Planning:** once the layout is complete, every page's characters and
  words are read off the frame and batches are planned from the settings. A
  new layout or batch size plans again, keeping unlocked text unlocked.
- **Counting, in words mode** (`counts.rs`, `run_words.rs`): the levels of
  each page's words are asked in rounds of whole pages from the run's first
  page, and a page counts its distinct lemmas above the level, less
  memorized ones. The pane holds where it is until 60 pages (`WINDOW`), or
  the book, are counted; the pages past the count stand as pages-mode
  batches. Each later round plans again from the first batch not in use (one
  passed, the gate's with a deck, or one with a deck made ahead). Without the
  word data, words mode plans by pages.
- **The gate:** the first locked page. The pane may show it, veiled with its
  deck, but no further: jumps and turns past it land on it with a toast.
  Read-aloud stops when it reaches it. `Session::study_gate` answers it.
- **Decks:** the gate batch's words are read, levelled, tagged when the
  tagger is there (a proper-noun tag drops the word) and given meanings by the
  dictionary pack in study's language, else Explain's, else the first ready
  pack. A dictionary deck holds only words the pack has: they are looked up
  48 at a time, most seen first, until the deck is full, so no card has an
  empty back. A silent tagger is given up after 10 s and a silent
  dictionary after 20 s, ending the deck with the cards found (or an error
  if none).
  A deck with no new words unlocks its batch with a toast.
- **AI cards:** *Cards from* picks who writes the backs: Auto (the AI when
  study has a model, else the dictionary), AI or Dictionary. The AI writes a
  deck's backs in `StudyCards` requests (the word's common meaning, its
  class, up to three synonyms, one example that uses the word itself, and
  whether only this book uses it, as a name does) on study's model, else
  Explain's, in study's language. The sentence goes along only so the AI
  knows which word and class are meant; the meaning never retells the
  story. Each request carries about the model's **Cards per request**
  (AI models, default 10): a deck is cut evenly into `words / limit`
  requests, rounded half up (`mode::chunks`), so with 10, 13 words go in
  one request, 15 in 8 and 7, and 32 in 11, 11 and 10. All are asked at
  once and the model's queue runs as many as its At once lets in (with
  At once 2 or more, a task of the Ahead class, as study cards are by
  default, leaves one place free for the reader's own asks). A back is
  kept per word, sentence and writer for the session, so a deck made again
  asks only for the new ones, and a deck whose one request failed asks
  only that request's words when tried again. An AI deck that fails says
  why; `use_dictionary` has the dictionary write that batch's deck instead.
- **The AI dictionary** (`deck_job/reuse.rs`, `docs/study.md`): before the
  AI is asked, the cards with a tagger class look in it, and one kept for
  the same lemma, class and study language becomes the back (three
  synonyms and one example at most); only the rest are written. Written
  backs that aren't book-only go in, and a meaning kept before stays.
- **Half written** (`deck_job/early.rs`, `open.rs`): an AI deck opens once
  half its cards (rounded up) have backs and requests are still out. Those
  cards' classes come from the tagger and the AI only. The same job writes
  on behind the deck (`Rest`); its cards join the last part as they land,
  minus any the deck has (same key, or same lemma where a class is
  missing), with time added for them. A muted line under the header counts
  them. If the rest fails, the line says why and offers *Try again*
  (`retry`), *Finish with the dictionary* (`use_dictionary`) and *Skip the
  rest* (`skip_rest`). Read now drops a rest still being written.
- **Look-ahead:** `study.mode.ahead` decks are made while the reader reads:
  the gate's and up to `ahead - 1` past it, each opened at once when its
  batch reaches the gate. At 0 the gate's deck waits until the reader nears
  it. A deck whose batch is no longer coming is dropped, cancelling its ask;
  a new source, level, Words per deck, model or language makes the decks not yet
  open again, and words memorized since a deck was made leave it as it opens.
- **Known** saves the word as memorized; **Undo** puts back what the word
  was. **Start** (Read now) keeps the rest as learning, unlocks the batch and
  shows it from its first page.
- **The time limit:** with the limit on, Read now or a spent allowance
  closes that batch's deck for good, the latter with a toast. With it off,
  the deck is kept, and coming into its pages from the ones before offers it
  again in a toast whose *Open cards* sends `reopen`.
- **Read back** (`read_previous`) shows the page just before the gate (or
  before a reopened deck's batch) and stops the deck's time. Coming back to
  the gate shows the veil, not the deck, and a toast offers the cards with
  the time left (*Open the cards for pages 3–7? · 2:10 left*).
- **The lock** (`study.mode.lock`, off by default) keeps the pane at or
  after the start: jumps and turns below it land on it with a toast, and
  Read back stops there. `Session::study_floor` answers it.
- **Kept:** a library book keeps its gate as `unlocked`, so every pane on it
  shares one gate. A file outside the library keeps its gate for the session,
  and its words marked known leave the run's later decks though none is saved.
- **Reactive recall:** `recall` with `ask` works one recall at a time: `open`
  (`page`, `from`, `to`) on a word of a Reactive pane, `answer` (`text`),
  `again`, `reveal`, `memorized`, `learning`, `missed` or `close`. Opening
  it keys and bands the word from the levels and asks the card writer for
  its back (kept like a card's), which gives the hint after a miss and the
  meaning once revealed. Each answer goes to the `Judge`: `StudyJudge` on
  study's model, a closed schema of verdict and feedback. A judge that fails
  costs no try and keeps the answer. With `study.mode.ask_memorize` off, a
  correct answer memorizes at once. The word is saved met by Reactive, with
  its tries and whether the first was right; a reveal saves it as missed,
  and closing a revealed recall does too.
- **Refused while on:** Explain, Look up and the page AI in that pane.
- **Turns itself off** with a toast when the pane's document closes or a need
  above is lost; another file opened in the pane ends it quietly.

## In the reader (`crates/ui-egui/src/study_mode`)

- **The page menu** opens with a Study section: the switch, Proactive or
  Reactive, and the pages per batch. When a need is missing the switch is
  off, the reason shows and a row opens the settings tab that fixes it.
  *Study settings…* opens Settings › Study mode, and goes with the tab
  when neither an AI nor a dictionary is set up. While a pane studies the
  AI tasks below it are disabled. Turning study on, or changing its mode,
  closes the menu, so the deck's *Preparing the words* shows clear.
- **The view holds at the gate:** a strip scrolls no further than the end
  of the gate page, and a paged mode turns no further than it. With the lock
  on it also holds at the top of the start page, and a frost covers the
  pages before it (`gate::StudySpan`, `gate::frost`).
- **The veil:** frosted paper over every locked page on screen, opaque to the
  words. While the locked pages only peek in it says what unlocks them and
  offers *Study the words*; once they fill 30 % of the strip (or any of a
  paged spread) the veil covers the pane and holds the deck. After a Read
  back it says the cards are waiting and offers *Open the cards* instead.
- **The deck:** flash cards centred in the pane, both ways. Small blocks
  sit in its top corners: the pages as bare numbers (*1 – 2*) on the left;
  the time left, when the deck has a limit, and a shuffle that only
  reorders what is shown on the right. A card's front holds the word
  alone, its band in the top right corner; nothing from the book's
  sentence, which would give it away. Its back gives the meaning and the
  word class, and an AI card adds its synonyms and an example of its own.
  *Click to turn, drag to move on* shows on hover until the reader first
  turns, swipes or steps a card, and never again (a flag kept with the
  window's memory).
  The card (`study_mode/flip_card.rs`) is painted as shapes, tessellated
  and turned in perspective about its upright axis. Its texts are noted to
  `machine_fonts::note`, as the scan of drawn text never sees a mesh, so a
  word in any script gets the machine's fallback faces. A click, Space or
  Enter turns it over; a drag past 28 % of its width or a flick moves on,
  rubber-banded at either end, and the arrows step too. Two cards peek out
  beneath. With motion off it changes at once. Under the card, round step
  buttons flank *3 / 9*; then *I already know this* (K, its key cap shown)
  marks the word known, beside an undo once one is marked; then *Read
  back*, which shows the page before the batch, sits beside *Read now*,
  which asks first while time is left, since the cards then close for
  good; *Turn off study mode* always asks (`study_mode/confirm.rs`). The
  deck tells the engine as it appears and goes (`Action::Shown`), so its
  time runs only on screen.
- **Short and narrow panes** (`study_mode/tight.rs`, `rows.rs`,
  `card_face.rs`): the deck fits the pane. A pane tall and wide enough
  keeps the roomy deck, its card shrinking to 190 pt first. Shorter, or
  narrower than 300 pt (where the roomy rows would overflow), it turns
  compact and gives the card the room: the row over the card goes (the
  pages and shuffle drop), the card shows the time left top left, *3 / 9*
  top middle (while the corner pills leave room) and the band top right,
  the steps become round arrows on the card's edges, and one footer row
  holds the known check, undo, a bare *Read back* arrow and *Read now*
  (kept off the corner marks while it fits, its icon dropped when narrow);
  *Turn off study mode* is left to the page menu's switch, and the deck
  rises closer to the pane's top. The card is planned tall enough for the
  deck's longest back to read easily (up to 320 pt), taking the height left
  in a short pane; a narrow card never goes below its usual shape, a wide
  one down to 150 pt. The back always holds all an AI wrote: crowded, its
  text steps down in size (meaning 20 to 13 pt) and moves under the corner
  pills; past the smallest step it scrolls with the wheel under the pills,
  soft edges showing there is more. Shorter still, the compact deck
  scrolls with the wheel. What the deck takes besides its card is measured
  per layout, so the card fills exactly what is left.
- **Waiting:** while the deck is made (`study_mode/making.rs`), three blank
  cards the size of the real ones drift in a loose stack over the frost,
  their lines shimmering as if written; no paper sits behind them. Beneath,
  it says which stage it is at (finding the words, writing them with the
  model and how many are written, waiting while the model is busy with
  other work, looking them up) over a thin track filled as far as the
  deck is made.
  After 7 s of the AI it says it is slow and offers *Use the dictionary
  instead*. With motion off the stack stands still.
- **Failed:** an alert mark says why, with no paper behind it, then
  *Try again*, *Use the dictionary* and *Read previous pages*. A failure
  another try can't mend leads with its setup instead (`run::Setup`, set
  by the deck job): nobody to write the cards gives *Set up AI…* (Settings
  › AI) and, without a pack, *Get a dictionary…*, with no *Try again*,
  since a new model or pack makes the deck again by itself; a model turned
  away over its keys or settings gives *Check the model…* (AI models);
  missing word levels give *Get word data…* (Vocabulary). Those two keep
  *Try again* beside, as fixing them changes no recipe.
- **The corner stack** (`crate::activity`, the AI's work in one place):
  while the AI writes a deck, the gate's or one made ahead, and its own
  waiting card is out of view, a ring sits in the pane's bottom left
  corner, level with the page counter across the pane (no room added under
  it), rising over the page bar while that is up; toasts keep the right,
  filled as far as the deck is made. Each job is a line in the stack, the
  newest in front and up to two peeking behind; a hover fans them out to
  say so (*Writing cards 3 of 8 · pages 4–6*) with *Cancel*, which asks to
  turn study mode off. Lines fan up from the corner, ring first, then the
  words and buttons. Where a low deck covers the corner, it glides up
  above it, never past the pane's middle and never to the top (`seen::deck`
  marks where the deck drew, and that its job is in view). The gate's deck, finished while away, stays as *Cards ready* with
  *View* until seen; one made ahead says *Next cards ready* for 3 s. A
  failure shows red for 3 s, longer while hovered. A failed or stopped deck
  also shows a toast. Dictionary cards come at once and never show it.
- **One stack, or one a pane** (`engine::activity::place`): the app's own work,
  as organizing the library, is global. Alone, it stacks once, at the
  workspace's bottom left, however many panes show. Once a pane's job
  needs its corner, every pane's corner shows the global lines, and the
  owning pane its own in front of them; when that job's visual is back in
  view, the one workspace stack returns. On the library page, its own
  card says how organizing goes, so the stack leaves it out.
- **The AI Dictionary** joins it (`docs/study.md`, The glow): each word
  asked is a line while its glow and card are both out of view, and so
  does each skill card (`docs/selection-ai.md`) while its card is away.
- **The page AI** joins it (`docs/page-ai.md`, `activity::pages`): a page
  rewrite is a line while its bubble and words are off screen
  (`seen::page_runs`), picked pages one line counting their groups, and a
  continuous run one, with *Continue* when it asks; it leaves the stack
  while a page of its spins its bubble in view. Cancel stops a
  rewrite at once (`page_ai::Action::StopRun`) with a toast. The old pill
  at the pane's bottom is gone.
- **The judge** joins it (`Key::Judge`): while it checks a recall's
  answer and the recall's word is off screen, a line says *Checking your
  answer · “brisk”* (*Waiting for the model* while queued). View goes back
  to the word; Cancel asks to turn study mode off, as a deck's does.
- **In sight** (`crate::sight`): one rule says whether a visual is on
  screen, a rect touching its pane's room; the read-aloud player asks it
  too, of its pane above the page bar's band (`sight::reading_room`). A
  visual in a page calls `seen::watch(ctx, key, (room, rect))` with where
  it drew, and stamps only while that shows; one on screen whenever drawn,
  as an open card or the library page, calls `seen::in_view`.
- **Adding a job.** Both rules and every state come with the stack; a new
  job needs: a `Key` variant; its `Kind` in the `kinds!` list; its arms in
  `scope`, `kind`, `stop_word`, the words and `ending` (one line an arm,
  each calling the owner's own helper); a reader of it in
  `Session::activities`; `seen::watch` from its visual; and its View and
  Cancel in `app/activity.rs::press_activity`. The compiler asks for every
  arm. The UI tests' `jobs::scene` asks for a scene with the job at work
  and its visual in view; one test walks `Kind::ALL` through them. It
  fails if `Session::activities` misses the job, or if a visual in view
  leaves its line in the stack.
  Closing the recall drops the judge, with a *Stopped checking* toast.
- **Toasts rise above it** (`stack::toast_lift`): a toast never covers
  a stack, nor waits behind it, so a failure's toast shows with its red
  line.
- **Progress is real** (`DeckJob::made`, `deck_job/progress.rs`): the
  track and the ring show the same share, counted from real steps only.
  Finding the words is the first 10%: their levels looked up, then their
  classes tagged. Writing is the rest, one step per card, cached cards
  first; cards arrive one by one as the model streams each request's
  answer. Before
  the first card nothing is known, so the fill holds still; a soft pulse
  at its head (steady with reduced motion) says the work goes on. The fill
  eases to each new step.
- **Thumbnails** (`thumbnails/study.rs`, from `Session::study_stands`):
  pages that read freely show as they are. A locked page stands one of
  three ways: **shut** (its cards not ready: no deck yet, a deck waiting in
  line for the model, a failed one, or a page before a locked start) is
  dimmed under a padlock; **writing** (the AI writes its cards now) is
  dimmed under a padlock and wears the AI glow; **ready** (its cards wait
  at the gate) keeps the padlock, undimmed. The dictionary writes at once,
  so its locked pages are always ready. The batch whose deck the reader
  faces has an accent bar beside it. The glow is the one page AI runs show
  ([Thumbnails](thumbnails.md#the-ai-glow)).
- **Word help is off:** the selection bar drops Explain and Look up, and
  CEFR shading hides unless Hints is on.
- **Reactive:** shading stays, a click on a shaded word opens the recall
  popover beside it and a rest looks nothing up. The popover shows the band
  and the tries used, asks what the word means here, and answers *Almost* or
  *Not quite* with the AI's feedback and a hint, *Correct* with
  *Add to memorized?*, the meaning and synonyms once revealed, or why the
  judge failed with *Retry*. Escape closes it. With *Ask before adding to
  memorized* off, a correct answer is memorized at once.
- **Solved and missed:** a word recalled takes a reserved look until the
  run ends, a green underline when answered (`highlights.solved`) and a red
  one when missed (`highlights.missed`); it opens no recall again. No pen
  may take either look (`HighlightSettings::reserved`), and Settings ›
  Highlights › Other marks shows them as *Study recall*.
- **Settings › Study mode** (`settings_window/study.rs`): listed only when
  an AI model or a dictionary pack could write cards. The mode, with a note
  naming the Vocabulary level it uses (set only in Vocabulary); Batches
  (size by words or pages, pages per batch, words per deck up to 50);
  Proactive (prepare ahead, time limit and seconds per card, the lock,
  shading, and *Cards from* only when both could write); Reactive (ask before memorizing); AI
  (study's model and language, desktop only, once a model is set up); and
  *Open Vocabs* (Space's words page). Highlights shows *Study recall*
  only when a model could judge.
- **Shots:** `settings=study` opens the tab,
  `study_mode=proactive|reactive|off`, `study_gate=on` (in the focused pane,
  or the one `study_focus=N` names, for split panes; `pdf=sample:30:beside`
  opens a book in a new pane beside),
  `study_deck=reveal` (each pane's card turned), `study_deck=drag:-140`
  (held mid-swipe) or `study_deck=scroll:80` (a long back scrolled),
  `study_cards=ready|ai|silent|failing|keyless` (a scripted AI that
  writes every back, for `ai` from a long JSON answer read as a model's
  would be, and judges "right" correct and "close" close, never answers,
  fails, or fails for want of an API key) and `recall=word` or `recall=word:answer`, a recall on
  the first `word` of the page read. The shot applies them once the book and
  its word levels are in, then waits for the deck or the recall.

## Space (`crates/engine/src/space.rs`, `crates/ui-egui/src/space`)

Space is every studied word across books, on its own page beside the
library: the library bar's *Your words* button opens it, and its pressed
button, the Library button or Escape go back.

- **Rows:** *With books* lists a row per book that met the word; *Merged*
  folds a word's books into one row, newest first, its tries summed.
- **The rail:** the books with their counts, B1 to C2, *Learned by* (Any,
  Cards, Recall), *Meaning from* (Any, Dictionary, AI) and the part of
  speech. No level picked shows every level. Words kept before a row knew
  who wrote its meaning show only under Any.
- **The head:** a search over the word, its meaning and its sentence;
  *Export CSV*; the status tabs, each counted after every other filter.
- **Stats:** memorized words of those met, first-try answers of the
  reactive answers, and a bar per level's memorized words.
- **Toolbar:** group by none, book, level, month or status (never book when
  merged; levels go hardest first), sort by recent, A–Z, hardest, most tries
  or book, and a list or cards.
- **A row** shows the word, its class and level, its meaning, its status,
  *card* or its tries, *first try*, its books and the day it was last met.
  Its chevron opens the sentence. *Memorize* and *Un-memorize* act on the
  word in every book; *Forget* drops it, so it is carded again when met.
- **CSV** (`mareader-space.csv`): a line per row shown under the header
  `lemma,pos,level,meaning,context,book,status,date`, every cell quoted;
  books are joined by `; ` and the date is in UTC.
- **Scripts:** `space.word` with `action` memorize, unmemorize or forget and
  `word` as `lemma|class`.
- **Shots:** `words=demo` studies eight made-up words in the first books,
  `route=space` opens the page and `space_view=cards` draws cards.
