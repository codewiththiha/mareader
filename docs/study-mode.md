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

A batch is whole pages, 1 to 10 (`MAX_PAGES`), and is stored as a text range,
so a re-typeset keeps it.

- **Pages mode** counts pages that carry text.
- **Characters mode** spends a budget, and a page joins when at least half of
  it fits.
- **Light pages** (under 140 characters: a blank leaf, a lone heading) use no
  quota and ride with a neighbour.
- **A short tail** (under 40% of the target) merges into the batch before it,
  as long as that batch stays within 10 pages.
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
  knows. `acknowledged` becomes `acknowledge` only when `acknowledge` is known.
  The CEFR store stays as it is.
- **The class** comes from the Penn tag of the dataset's sense.
- **Proper nouns get no card:** a capitalised word inside a sentence reads as
  a name.

### Decks (`deck.rs`)

- **What makes a card:** words above *My level*, one card per key, with its
  count ("appears 3×") and the book's sentence of its first sighting.
- **Also as:** the same lemma in another class is its own card, linked by
  "also as".
- **Memorized words** are dropped when the deck is built, so a cached meaning
  never brings one back.
- **Order:** by count, then band, then reading order. The deck is capped at
  *Cards per batch* (12 by default).
- **The allowance** (`Allowance`): with *Time limit on cards* on (the
  default), a deck has *Minutes per deck* (5 by default, 1 to 30). Its time
  runs only while the deck shows and is kept across visits. Nothing waits
  on it: *Read now* works at any time.

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
`use_dictionary`, `read_previous`, `recall`).

- **Planning:** once the layout is complete, every page's characters are
  counted off the frame and batches are planned from the settings. A new
  layout or batch size plans again, keeping unlocked text unlocked.
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
  deck's backs in one `StudyCards` request (a meaning in context, up to three
  synonyms and an example) on study's model, else Explain's, in study's
  language. A back is kept per word, sentence and writer for the session, so
  a deck made again asks only for the new ones. An AI deck that fails says
  why; `use_dictionary` has the dictionary write that batch's deck instead.
- **Look-ahead:** `study.mode.ahead` decks are made while the reader reads:
  the gate's and up to `ahead - 1` past it, each opened at once when its
  batch reaches the gate. At 0 the gate's deck waits until the reader nears
  it. A deck whose batch is no longer coming is dropped, cancelling its ask;
  a new source, level, card cap, model or language makes the decks not yet
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
  AI tasks below it are disabled.
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
  model, looking them up) over a thin track, filled as cards are written.
  After 7 s of the AI it says it is slow and offers *Use the dictionary
  instead*. With motion off the stack stands still.
- **Failed:** an alert mark says why, with no paper behind it, then
  *Try again*, *Use the dictionary* and *Read previous pages*.
- **The corner toast** (`study_mode/progress.rs`): while the AI writes a
  deck, the gate's or one made ahead, a ring sits in the pane's bottom
  left corner, rising over the page bar only while that is up, spinning while the words are found, filling as cards are written,
  red once writing failed. A hover widens it to say so (*Writing cards 3
  of 8 · pages 4–6*). A finished deck shows *Next cards ready* for 3 s.
  Dictionary cards come at once and never show it; the deck opens at the
  gate.
- **Thumbnails** (`thumbnails/study.rs`): the batch read stays bright with
  an accent bar beside it, the other pages dim (those before a locked start
  the most), and the pages past the gate wear a wash and a padlock.
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
  (size by pages or characters); Proactive (prepare ahead, time limit and
  minutes per deck, the lock, shading, words per deck, and *Cards from*
  only when both could write); Reactive (ask before memorizing); AI
  (study's model and language, desktop only, once a model is set up); and
  *Open Vocabs* (Space's words page). Highlights shows *Study recall*
  only when a model could judge.
- **Shots:** `settings=study` opens the tab,
  `study_mode=proactive|reactive|off`, `study_gate=on` (in the focused pane,
  or the one `study_focus=N` names, for split panes; `pdf=sample:30:beside`
  opens a book in a new pane beside),
  `study_deck=reveal` (each pane's card turned), `study_deck=drag:-140`
  (held mid-swipe) or `study_deck=scroll:80` (a long back scrolled),
  `study_cards=ready|ai|silent|failing` (a scripted AI that
  writes every back, for `ai` from a long JSON answer read as a model's
  would be, and judges "right" correct and "close" close, never answers,
  or fails) and `recall=word` or `recall=word:answer`, a recall on
  the first `word` of the page read. The shot applies them once the book and
  its word levels are in, then waits for the deck or the recall.

## Space (`crates/engine/src/space.rs`, `crates/ui-egui/src/space`)

Space is every studied word across books, on its own page beside the
library: the library bar's *Your words* button opens it, and its pressed
button, the Library button or Escape go back.

- **Rows:** *With books* lists a row per book that met the word; *Merged*
  folds a word's books into one row, newest first, its tries summed.
- **The rail:** the books with their counts, B1 to C2, *Learned by* (Any,
  Cards, Recall) and the part of speech. No level picked shows every level.
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
