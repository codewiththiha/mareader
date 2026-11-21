# Study mode

Study mode turns reading into vocabulary study. It comes from the lab's
`best-study-mode` prototype and is built in slices; this page describes what
has landed. Parity entries: `study.mode-proactive`, `study.mode-reactive`,
`study.space`, `study.mode-ai-cards`.

- **Proactive:** before each batch of pages, its hard words come up as cards.
  The pages unlock once the cards have had their time.
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
- **Turning the mode on mid-book** frees the pages behind; mid-batch, it frees
  the rest of that batch.

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
- **The floor:** seconds per card (6 by default) times the cards. Marking a
  card known gives its seconds back. With *Strict* on, Start waits for the
  floor.

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

*Cards from* picks Auto (AI first), AI or Dictionary. A pick never falls back
silently.

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
(`toggle`, `off`, `start`, `known`, `undo`, `next_batch`, `retry`,
`read_previous`). Reactive answers that it comes in a later update.

- **Planning:** once the layout is complete, every page's characters are
  counted off the frame and batches are planned from the settings. A new
  layout or batch size plans again, keeping unlocked text unlocked.
- **The gate:** the first locked page. The pane may show it, veiled with its
  deck, but no further: jumps and turns past it land on it with a toast.
  Read-aloud stops when it reaches it. `Session::study_gate` answers it.
- **Decks:** the gate batch's words are read, levelled, tagged when the
  tagger is there (a proper-noun tag drops the word) and given meanings by the
  dictionary pack in study's language, else Explain's, else the first ready
  pack. A silent tagger is given up after 10 s and the dictionary after 20 s.
  A deck with no new words unlocks its batch with a toast.
- **Known** saves the word as memorized and gives its seconds back; **Undo**
  puts back what the word was. **Start** keeps the rest as learning, unlocks
  the batch and shows it from its first page.
- **Kept:** a library book keeps its gate as `unlocked`, so every pane on it
  shares one gate. A file outside the library keeps its gate for the session.
- **Refused while on:** Explain, Look up and the page AI in that pane.
- **Turns itself off** with a toast when the pane's document closes or a need
  above is lost; another file opened in the pane ends it quietly.

## In the reader (`crates/ui-egui/src/study_mode`)

- **The page menu** opens with a Study section: the switch, Proactive or
  Reactive, and the pages per batch. When a need is missing the switch is
  off, the reason shows and a row opens the settings tab that fixes it.
  While a pane studies the AI tasks below it are disabled.
- **The view holds at the gate:** a strip scrolls no further than the end
  of the gate page, and a paged mode turns no further than it.
- **The veil:** frosted paper over every locked page on screen, opaque to the
  words. While the locked pages only peek in it says what unlocks them and
  offers *Study the words*; once they fill 30 % of the strip (or any of a
  paged spread) the veil covers the pane and holds the deck.
- **The deck:** the batch, its count of new words and a ring for the time
  floor. A card shows the word, its class and band, how often it appears,
  its other classes and its sentence from the book; the meaning waits for
  *reveal* (Space). *I already know this* (K) marks it known; the arrows
  step between cards. *Start reading* waits for the floor when Strict is on.
  The deck also says when it is being prepared and when it failed, with
  *Try again* and *Read previous pages*.
- **Word help is off:** the selection bar drops Explain and Look up, and
  CEFR shading hides unless Hints is on.
- **Shots:** `study_mode=proactive|reactive|off`, `study_gate=on` and
  `study_deck=reveal`.
