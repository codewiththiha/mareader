//! Study mode: the proactive gate, the deck, and memorized words.

use std::collections::{HashMap, HashSet};

use mareader_study::{
    Batch, BatchCfg, Candidate, Deck, Mode, build_batches, build_deck, range_label,
};
use mareader_workspace::PaneId;

/// What a script or the menu asks of study mode.
#[derive(Clone, Debug, PartialEq)]
pub enum Action {
    /// Turns the switch: off, proactive or reactive.
    Set(Mode),
    /// The deck's "I already know this": key is lemma|pos.
    MarkKnown(String),
    /// Starts reading once the timer allows (or Strict is off).
    StartReading,
    /// Dismisses the gate to scroll back; the gate returns at the end.
    ReadBack,
    /// Closes the open gate without unlocking (Escape).
    CloseGate,
    /// Reactive: a finished recall attempt's outcome.
    RecallDone {
        key: String,
        memorized: bool,
        missed: bool,
    },
    /// Puts a finished deck's batch behind the reader.
    UnlockBatch,
}

/// One pane's open gate: the next locked batch and its deck.
#[derive(Clone, Debug, PartialEq)]
pub struct Gate {
    pub pane: PaneId,
    pub batch: Batch,
    pub deck: Deck,
    /// Wall milliseconds the gate opened, for the timer floor.
    pub opened_ms: u64,
    /// Wall milliseconds the timer last stepped a whole second.
    pub last_tick_ms: u64,
    /// Study seconds each card adds to the floor.
    pub sec_per_card: u32,
    /// Seconds still locked; shrinks when a card is marked known.
    pub left_secs: u32,
    /// Cards already marked known in this gate.
    pub known: HashSet<String>,
}

/// Where a book's proactive lock stands, and the words already learned.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Progress {
    /// First page not yet unlocked; pages before it are free.
    pub unlocked_to: usize,
}

/// Engine-owned study-mode state: batches, the gate, memorized keys.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    mode: Mode,
    /// Last planned sizes, so a layout change re-plans once.
    sizes: Vec<usize>,
    batches: Vec<Batch>,
    /// Unlocked pages per content key (book or session document).
    progress: HashMap<u128, Progress>,
    /// Global memorized keys: lemma|pos, never shaded or carded again.
    memorized: HashSet<String>,
    gate: Option<Gate>,
    /// Reactive answers this session: key → (solved, missed).
    recall: HashMap<String, (bool, bool)>,
}

impl State {
    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = mode;
        if mode == Mode::Off {
            self.gate = None;
        }
    }

    pub fn batches(&self) -> &[Batch] {
        &self.batches
    }

    pub fn gate(&self) -> Option<&Gate> {
        self.gate.as_ref()
    }

    pub fn memorized(&self) -> &HashSet<String> {
        &self.memorized
    }

    pub fn is_memorized(&self, key: &str) -> bool {
        self.memorized.contains(key)
    }

    pub fn mark_known(&mut self, key: impl Into<String>) -> bool {
        let key = key.into();
        let fresh = self.memorized.insert(key.clone());
        if let Some(gate) = self.gate.as_mut()
            && gate.known.insert(key.clone())
        {
            gate.left_secs = gate.left_secs.saturating_sub(gate.sec_per_card);
            gate.deck
                .cards
                .retain(|c| mareader_study::word_key(&c.lemma, &c.pos) != key);
        }
        fresh
    }

    /// Cards left in the open gate's deck; 0 with no gate.
    pub fn deck_len(&self) -> usize {
        self.gate.as_ref().map_or(0, |g| g.deck.cards.len())
    }

    pub fn forget(&mut self, key: &str) -> bool {
        self.memorized.remove(key)
    }

    pub fn recall(&self, key: &str) -> Option<(bool, bool)> {
        self.recall.get(key).copied()
    }

    pub fn note_recall(&mut self, key: impl Into<String>, memorized: bool, missed: bool) {
        self.recall.insert(key.into(), (memorized, missed));
    }

    /// Unlocked page for `book`; zero when none is recorded.
    pub fn unlocked_to(&self, book: u128) -> usize {
        self.progress.get(&book).map_or(0, |p| p.unlocked_to)
    }

    pub fn set_unlocked(&mut self, book: u128, page: usize) {
        self.progress.entry(book).or_default().unlocked_to = page;
    }

    /// Re-plans batches when the page sizes changed; returns true if they did.
    pub fn plan(&mut self, sizes: Vec<usize>, cfg: &BatchCfg) -> bool {
        let changed = self.sizes != sizes || self.batches.is_empty();
        if changed {
            self.sizes = sizes;
            self.batches = build_batches(&self.sizes, cfg);
        }
        // Round any unlock up to a batch end so reflow keeps progress.
        for progress in self.progress.values_mut() {
            if progress.unlocked_to > 0
                && let Some(b) = self
                    .batches
                    .iter()
                    .find(|b| b.from < progress.unlocked_to && progress.unlocked_to <= b.to + 1)
            {
                progress.unlocked_to = b.to + 1;
            }
        }
        changed
    }

    /// The batch that gates `book` now: the first with `from >= unlocked`.
    pub fn gate_batch(&self, book: u128) -> Option<&Batch> {
        let unlocked = self.unlocked_to(book);
        self.batches.iter().find(|b| b.from >= unlocked)
    }

    /// Whether a jump to `page` is past the proactive lock.
    pub fn blocks(&self, book: u128, page: usize) -> bool {
        self.mode == Mode::Proactive && page >= self.unlocked_to(book) && !self.batches.is_empty()
    }

    /// Opens the gate for the batch locking `book`, building `deck` for it.
    pub fn open_gate(
        &mut self,
        pane: PaneId,
        book: u128,
        deck: Deck,
        sec_per_card: u32,
        now_ms: u64,
    ) -> Option<Gate> {
        let batch = self.gate_batch(book)?.clone();
        let left_secs = mareader_study::deck_seconds(deck.cards.len(), sec_per_card);
        self.gate = Some(Gate {
            pane,
            batch,
            deck,
            opened_ms: now_ms,
            last_tick_ms: now_ms,
            sec_per_card,
            left_secs,
            known: HashSet::new(),
        });
        self.gate.clone()
    }

    pub fn close_gate(&mut self) {
        self.gate = None;
    }

    /// Unlocks the open gate's batch for `book` and closes it.
    pub fn unlock_gate(&mut self, book: u128) -> Option<Gate> {
        let gate = self.gate.take()?;
        self.set_unlocked(book, gate.batch.to + 1);
        Some(gate)
    }

    /// Seconds left on the open gate's timer, or 0 with none.
    pub fn gate_left(&self) -> u32 {
        self.gate.as_ref().map_or(0, |g| g.left_secs)
    }

    /// Steps the open gate's timer; true when a whole second elapsed.
    pub fn tick(&mut self, now_ms: u64) -> bool {
        let Some(gate) = self.gate.as_mut() else {
            return false;
        };
        if gate.left_secs == 0 {
            return false;
        }
        let elapsed = now_ms.saturating_sub(gate.last_tick_ms);
        if elapsed < 1_000 {
            return false;
        }
        let secs = (elapsed / 1_000) as u32;
        gate.left_secs = gate.left_secs.saturating_sub(secs);
        gate.last_tick_ms = gate.last_tick_ms.saturating_add(u64::from(secs) * 1_000);
        true
    }

    /// Whether Start reading may run now: timer done, or not strict.
    pub fn may_start(&self, strict: bool) -> bool {
        !strict || self.gate_left() == 0
    }

    /// Drops batches and the open gate so the next plan starts clean.
    pub fn reset_plan(&mut self) {
        self.sizes.clear();
        self.batches.clear();
        self.gate = None;
    }
}

impl Gate {
    /// The label the deck header shows: "pages 4–6".
    pub fn range(&self) -> String {
        range_label(&self.batch)
    }
}

/// Candidates for a batch from page texts and the level cache.
///
/// `bands` maps normalized keys to CEFR bands, as `Vocab::bands` answers.
/// POS is left as `any` until the tagger runs; the key still dedupes.
pub fn candidates_in(
    texts: &[(usize, String)],
    bands: &HashMap<String, u8>,
    level: u8,
    known: &HashSet<String>,
) -> Vec<Candidate> {
    let mut out = Vec::new();
    for (page, text) in texts {
        let _ = page;
        for (word, sentence) in hard_words(text, bands, level) {
            let lemma = mareader_cefr::normalize(&word);
            let key = mareader_study::word_key(&lemma, "any");
            if known.contains(&key) {
                continue;
            }
            out.push(Candidate {
                lemma,
                pos: "any".to_owned(),
                band: bands
                    .get(&mareader_cefr::normalize(&word))
                    .copied()
                    .unwrap_or(level + 1),
                sentence,
            });
        }
    }
    out
}

/// Hard words of one page's text with the sentence each first sits in.
fn hard_words(text: &str, bands: &HashMap<String, u8>, level: u8) -> Vec<(String, String)> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for span in mareader_cefr::tokenize(text) {
        let Some(word) = chars.get(span.start..span.end) else {
            continue;
        };
        let word: String = word.iter().collect();
        let mut highest = 0;
        for key in mareader_cefr::candidates(&word) {
            if let Some(b) = bands.get(&key) {
                highest = highest.max(*b);
            }
        }
        if highest <= level {
            continue;
        }
        let lemma = mareader_cefr::normalize(&word);
        if !seen.insert(lemma.clone()) {
            continue;
        }
        out.push((word, sentence_at(text, span.start)));
    }
    out
}

/// The sentence holding the byte-ish char index `at`.
fn sentence_at(text: &str, at: usize) -> String {
    let mut pos = 0;
    for part in text.split_inclusive(['.', '!', '?']) {
        let len = part.chars().count();
        if at < pos + len {
            return part.trim().to_owned();
        }
        pos += len;
    }
    text.trim().to_owned()
}

/// Builds the deck for a batch's candidates, dropping memorized keys.
pub fn deck_for(candidates: Vec<Candidate>, known: &HashSet<String>, max: usize) -> Deck {
    build_deck(&candidates, known, max)
}

/// The batch cfg from settings dials.
pub fn cfg_of(unit: mareader_study::BatchUnit, pages: usize, chars: usize) -> BatchCfg {
    BatchCfg {
        unit,
        pages: pages.max(1),
        chars,
    }
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
