# Study mode

Study mode teaches words before or as you read them. Two modes, one shared
deck engine, one Space dashboard.

## Modes

| Mode | What happens | Best for |
|------|-------------|----------|
| **Off** | Read as usual. No gating, no shading for study. | Casual reading |
| **Proactive** | A batch of pages is locked until its deck is studied. A gate covers the stage with the batch's cards and a floor timer. | Learning before reading |
| **Reactive** | Read freely. Clicking a shaded word opens a recall popover: type what it means, get a correct/close/wrong verdict. | Learning in context |

## How it works

### Deck building

Each batch yields a deck: one card per `(lemma, class)`, ranked by how often the
word appears and how hard its CEFR band is, capped at the deck size. Memorized
words are dropped so a cached deck never goes stale. The card maker — AI model or
dictionary pack, chosen in Settings — writes each card's meaning, synonyms and
usage sentence.

### Word records

Every word the reader meets gets a record: `learning`, `memorized` or `missed`.
Records are global across books. A correct reactive answer, or marking a deck
card known, moves a word to `memorized`. `Unmemorize` puts it back.

### Floor timer (proactive)

The gate shows `sec_per_card × cards` seconds that must elapse before the batch
unlocks, when Strict is on. The timer counts down while the gate is open.

### Reactive recall

Click a shaded word to open the recall popover. Type the meaning — a synonym,
short explanation or translation. The judge scores it:

- **Correct** — the word becomes memorized.
- **Close** — right area, be more exact.
- **Wrong** — try again, up to three attempts. A hint follows the first miss.

The word itself is not an acceptable answer.

## Settings

Settings › Study mode:

- **Mode:** Off / Proactive / Reactive
- **Cards come from:** Auto / AI / Dictionary
- **Batch unit:** Pages / Characters
- **Pages per batch** (1–10), **Chars per batch** (500–8000)
- **Decks ahead** (0–3): decks prepared while reading
- **Seconds per card** (2–20): floor timer share
- **Cards per deck** (4–30)
- **Strict timer:** the floor timer must elapse before unlocking
- **Shade hard words:** highlight above-level words even in proactive
- **Ask before memorized:** a correct answer still asks once

Defaults: Proactive off, Auto source, Pages, 3 pages/batch, 1 deck ahead,
6 s/card, strict on, 12 cards/deck, shade off, ask before memorize on.

## The Space route

Space is a top-level page beside Library and Dictionary, reached from the View
menu. It shows the mode in force, memorized/learning/missed counts, batch and
deck settings, and a button to open Settings.

## Capability

- Proactive needs CEFR and a card source (AI or dictionary).
- Reactive needs CEFR and the AI model.
- Neither is offered for non-English books.
- Study mode can always be turned off from the View menu; progress persists.

## Where it lives

| Piece | Home |
|-------|------|
| StudyMode, StudyModeSettings, WordStatus | `crates/study/src/mode.rs` |
| build_batches, BatchCfg | `crates/study/src/batches.rs` |
| build_deck, Card, WordClass | `crates/study/src/deck.rs` |
| Verdict, Judged, heuristic judge | `crates/study/src/judge.rs` |
| CardMaker / StudyJudge traits + stand-ins | `crates/study/src/lib.rs` |
| Engine Mode state, per-book progress | `crates/engine/src/study/mode.rs` |
| study.mode / mark_known / unmemorize commands | `crates/engine/src/command_spec/acts.rs`, `execute/study.rs` |
| Settings tab | `crates/ui-egui/src/settings_window/study_mode.rs` |
| Gate overlay, deck, recall popover | `crates/ui-egui/src/study/mode.rs` |
| View menu toggle, Space route | `crates/ui-egui/src/view_menu.rs`, `crates/ui-egui/src/titlebar.rs`, `crates/engine/src/route.rs` |
| Space dashboard | `crates/ui-egui/src/app/frame/pages.rs` |
