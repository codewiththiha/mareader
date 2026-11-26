//! Selection AI answers saved with their book, separate from highlights.

use std::hash::{BuildHasher, RandomState};

use serde::{Deserialize, Serialize};

use crate::clock::{Field, Stamp};
use crate::extra::Extra;
use crate::id::{AiSelectionId, BookId};
use crate::library::Library;
use crate::marks::Spot;

/// A completed answer and the selection it was asked about.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AiSelection {
    pub id: AiSelectionId,
    pub book: Field<BookId>,
    /// A stable skill key, including custom skills.
    pub skill: String,
    /// The skill's name at the time of the request.
    pub name: String,
    pub spot: Spot,
    /// The selected text, kept to detect a changed passage.
    pub source: String,
    /// The translation target at the time of the request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    pub result: String,
    /// Work was still running when this record was saved.
    #[serde(default)]
    pub pending: bool,
    pub by: String,
    pub added: u64,
    pub alive: Field<bool>,
    #[serde(flatten)]
    pub extra: Extra,
}

/// A completed selection answer to save.
#[derive(Clone, Debug, PartialEq)]
pub struct NewAiSelection {
    pub skill: String,
    pub name: String,
    pub spot: Spot,
    pub source: String,
    pub language: Option<String>,
    pub result: String,
    pub pending: bool,
    pub by: String,
}

impl Library {
    /// Changes when a selection answer is saved or removed.
    pub fn ai_selections_stamp(&self) -> u64 {
        self.ai_selections.stamp()
    }

    /// A book's saved answers, oldest first.
    pub fn ai_selections(&self, book: BookId) -> Vec<&AiSelection> {
        let mut out: Vec<_> = self
            .ai_selections
            .values()
            .filter(|s| s.alive.value && s.book.value == book)
            .collect();
        out.sort_by_key(|s| (s.added, s.id));
        out
    }

    pub fn ai_selection(&self, id: AiSelectionId) -> Option<&AiSelection> {
        self.ai_selections.get(&id).filter(|s| s.alive.value)
    }

    /// Saves an independent answer; overlapping selections are not replaced.
    pub fn add_ai_selection(
        &mut self,
        book: BookId,
        new: NewAiSelection,
        wall_ms: u64,
    ) -> Option<AiSelectionId> {
        self.book(book)?;
        let at = self.clock.tick(wall_ms);
        let id = self.new_ai_selection_id(at);
        let selection = AiSelection {
            id,
            book: Field::new(book, at),
            skill: new.skill,
            name: new.name,
            spot: new.spot,
            source: new.source,
            language: new.language,
            result: new.result,
            pending: new.pending,
            by: new.by,
            added: wall_ms,
            alive: Field::new(true, at),
            extra: Extra::default(),
        };
        self.ai_selections.insert(id, selection);
        Some(id)
    }

    /// Removes or restores an answer, unless its book's data was erased.
    pub fn set_ai_selection_alive(&mut self, id: AiSelectionId, alive: bool, wall_ms: u64) -> bool {
        let Some(selection) = self.ai_selections.get(&id) else {
            return false;
        };
        let erased = alive && self.erased(selection.book.value, selection.alive.at);
        if selection.alive.value == alive || self.book(selection.book.value).is_none() || erased {
            return false;
        }
        let at = self.clock.tick(wall_ms);
        if let Some(selection) = self.ai_selections.get_mut(&id) {
            selection.alive = Field::new(alive, at);
        }
        true
    }

    /// Marks a failed write for retry with the current record contents.
    pub fn retry_ai_selection(&mut self, id: AiSelectionId) {
        let _ = self.ai_selections.get_mut(&id);
    }

    pub(crate) fn move_ai_selections(&mut self, gone: BookId, keeper: BookId, at: Stamp) {
        for selection in self
            .ai_selections
            .values_mut_where(|s| s.book.value == gone)
        {
            selection.book = Field::new(keeper, at);
        }
    }

    fn new_ai_selection_id(&self, at: Stamp) -> AiSelectionId {
        let state = RandomState::new();
        let seed = (at.ms, at.counter, self.device().0);
        let half = |salt: u8| u128::from(state.hash_one((seed, salt)));
        let mut id = AiSelectionId(half(1) << 64 | half(2));
        while self.ai_selections.contains_key(&id) {
            id = AiSelectionId(id.0.wrapping_add(1));
        }
        id
    }
}

#[cfg(test)]
#[path = "ai_selections_tests.rs"]
mod tests;
