//! Study mode state inside the engine: whether it is on, where each book's
//! lock stands, and which words are memorized, learning or missed.
//!
//! Mirrors the existing study state (`state.rs`): engine-owned, changed only
//! through commands, and kept with the books it came from.

use std::collections::HashMap;
use std::sync::Arc;

use mareader_library::{BookId, Spot};
use mareader_study::{CardMaker, StudyJudge, StudyMode, StudyModeSettings};

use crate::library::wall_ms;

/// One word the reader has met: its identity, its status, its level.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WordState {
    pub key: String,
    pub status: WordStatus,
    pub band: u8,
    pub first_seen: u64,
}

/// How a word is known: met and in rotation, explicitly memorized, or given up.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum WordStatus {
    #[default]
    Learning,
    Memorized,
    Missed,
}

/// Where a book's lock stands: pages up to this spot are free to read.
#[derive(Clone, Debug, PartialEq)]
pub struct StudyProgress {
    pub unlocked_to: Spot,
    pub mode: StudyMode,
}

/// A prepared deck's status while look-ahead works on it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DeckStatus {
    /// Being written: who is writing it and when the ask started.
    Loading { via: String, started_at: u64 },
    /// Ready to study: cards written and cached.
    Ready,
    /// The AI or the dictionary couldn't answer.
    Error,
}

/// One prepared deck, keyed loosely by the batch's end so the UI can find it.
#[derive(Clone, Debug, PartialEq)]
pub struct DeckEntry {
    pub book: BookId,
    /// The batch's end, as the mark layer keeps it.
    pub spot: Spot,
    pub status: DeckStatus,
}

/// The engine's study-mode state: mode, the words the reader has met, where
/// each book's lock stands, and any decks in flight.
#[derive(Default)]
pub struct Mode {
    pub mode: StudyMode,
    pub settings: StudyModeSettings,
    /// One entry per word the reader has met, global across books.
    pub words: HashMap<String, WordState>,
    /// Where each book's lock stands.
    pub progress: HashMap<BookId, StudyProgress>,
    /// Decks being prepared ahead, in the order they were asked.
    pub decks: Vec<DeckEntry>,
    /// Who writes cards, once a source is configured.
    pub card_maker: Option<Arc<dyn CardMaker>>,
    /// Who judges attempts, once a model is configured.
    pub judge: Option<Arc<dyn StudyJudge>>,
}

impl Mode {
    /// Turns study mode on or off, keeping every other setting.
    pub fn set_mode(&mut self, mode: StudyMode) {
        self.mode = mode;
        self.settings.mode = mode;
    }

    /// How a word is known, or none if the reader has never met it.
    pub fn word_status(&self, key: &str) -> Option<WordStatus> {
        self.words.get(key).map(|state| state.status)
    }

    /// Marks `key` memorized, creating its record when this is the first
    /// sighting of the word.
    pub fn mark_known(&mut self, key: &str, band: u8) {
        let wall = wall_ms();
        let state = self.words.entry(key.to_owned()).or_insert(WordState {
            key: key.to_owned(),
            status: WordStatus::Learning,
            band,
            first_seen: wall,
        });
        state.status = WordStatus::Memorized;
        state.band = band.max(state.band);
    }

    /// Puts a memorized word back into rotation.
    pub fn unmemorize(&mut self, key: &str) {
        if let Some(state) = self.words.get_mut(key) {
            state.status = WordStatus::Learning;
        }
    }

    /// Where `book`'s lock stands, or none when the book is not studied.
    pub fn book_progress(&self, book: BookId) -> Option<&StudyProgress> {
        self.progress.get(&book)
    }

    /// Records that the lock now stands at `spot` for `book`.
    pub fn set_unlock(&mut self, book: BookId, to: Spot) {
        self.progress.insert(
            book,
            StudyProgress {
                unlocked_to: to,
                mode: self.mode,
            },
        );
    }

    /// Remembers a deck is being prepared for the batch ending at `spot`.
    pub fn deck_loading(&mut self, book: BookId, spot: Spot, via: String) {
        let started = wall_ms();
        if let Some(entry) = self
            .decks
            .iter_mut()
            .find(|e| e.book == book && e.spot == spot)
        {
            entry.status = DeckStatus::Loading {
                via,
                started_at: started,
            };
        } else {
            self.decks.push(DeckEntry {
                book,
                spot,
                status: DeckStatus::Loading {
                    via,
                    started_at: started,
                },
            });
        }
    }

    /// Records that the deck for `spot` is ready or failed.
    pub fn deck_finished(&mut self, book: BookId, spot: Spot, ok: bool) {
        if let Some(entry) = self
            .decks
            .iter_mut()
            .find(|e| e.book == book && e.spot == spot)
        {
            entry.status = if ok {
                DeckStatus::Ready
            } else {
                DeckStatus::Error
            };
        }
    }

    /// Configures who writes cards (the AI, a dictionary pack, or nobody).
    pub fn set_card_maker(&mut self, card_maker: Arc<dyn CardMaker>) {
        self.card_maker = Some(card_maker);
    }

    /// Configures who judges attempts (the model, or nobody).
    pub fn set_judge(&mut self, judge: Arc<dyn StudyJudge>) {
        self.judge = Some(judge);
    }
}
