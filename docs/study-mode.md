# Study mode

Study mode turns the words above your level into words you keep. It has
two ways in, and both write the same records; Space is where they gather.

Turn it on per pane from the page menu's **Study mode** section, and set
how it behaves in **Settings › Study mode**. Progress is kept per book, so
turning a mode off never loses words, and reopening a book resumes where
its lock stood.

## Proactive: study first, then read

Reading is split into batches — whole pages, either a page count (1–10)
or a character budget converted back to whole pages. Short pages under
140 characters ride along, and a short final batch merges into the one
before it when it fits.

Before each batch, a deck of its hard words gates the pages. Words are
deduped by lemma and coarse class, memorized words (in any book) are
skipped, and the deck is ranked by how often a word appears, then by how
hard it is, capped by **Words per deck**.

Each card shows the word in its own sentence. Reveal its meaning, then
mark **I know this** or **Still learning**. A timer floor of
*seconds per card × cards not yet known* runs while you study; with
**Strict timer** on, marking a word known takes its seconds off the
wait. Finishing the deck records every word and unlocks the batch.
Turning forward past the gate is refused until then.

## Reactive: recall as you read

Read freely. Clicking a shaded word opens a recall popover instead of
the word card: type what the word means, in any language. The judge
checks meaning, not spelling — **correct**, **almost** or **not quite**,
three attempts, a hint after the first miss. **I don't know** reveals
the meaning and records the word as missed. A correct answer can add
the word to memorized (Settings can stop asking).

## What each mode needs

Both modes need word levels (Settings › Vocabulary) and an English
book. Proactive also needs an AI model or a dictionary pack to write
its cards; reactive needs an AI model, because a dictionary can't judge
an answer. A mode that can't run stays off and says why. **Card source**
chooses who writes cards: Auto prefers AI, then the dictionary.

## Space

**Space**, beside the Library, lists every study word in every book:
learning, memorized and missed, with its book, meaning and how it was
learned. Search, filter by status, sort by recent, A–Z, hardest, most
tries or book. Mark words memorized, un-memorize them, forget a
selection, or export the list as CSV. The summary counts each status
and your first-try recall rate.

Words are kept with their books in the library and sync like marks;
un-memorizing a word in Space makes it shade — and appear in decks —
again.
