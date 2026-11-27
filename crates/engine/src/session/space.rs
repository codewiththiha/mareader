//! Space's asks of studied words: memorize, un-memorize and forget.

use mareader_library::{Library, WordId, WordStatus};

use super::{Done, Session};
use crate::library::wall_ms;
use crate::space::Action;

impl Session {
    /// Acts on the words in every book; a word already so is left alone.
    pub(crate) fn space(&mut self, action: Action) -> Done {
        if let Action::RemoveAiSelections(ids) = action {
            let asks: Vec<_> = self.skills.asks().iter()
                .filter(|a| a.pinned.is_some_and(|id| ids.contains(&id)))
                .map(|a| a.id).collect();
            let mut changed = 0;
            for ask in asks { changed += usize::from(self.skill_remove(ask)); }
            let at = wall_ms();
            changed += ids.into_iter().filter(|id| self.library.model.set_ai_selection_alive(*id, false, at)).count();
            return if changed > 0 { Done::changed() } else { Done::quiet() };
        }
        let (model, at) = (&mut self.library.model, wall_ms());
        let changed = match action {
            Action::Memorize(ids) => model.set_word_status(&ids, WordStatus::Memorized, at),
            Action::Unmemorize(mut ids) => {
                ids.retain(|id| memorized(model, *id));
                model.set_word_status(&ids, WordStatus::Learning, at)
            }
            Action::Forget(ids) => model.set_words_alive(&ids, false, at),
            Action::RemoveAiSelections(_) => 0,
            Action::RemoveOccurrences(items) => items
                .into_iter()
                .filter(|(book, word)| model.remove_word_from_book(*book, *word, at))
                .count(),
        };
        match changed {
            0 => Done::quiet(),
            _ => Done::changed(),
        }
    }
}

fn memorized(model: &Library, id: WordId) -> bool {
    let word = model.word(id);
    word.is_some_and(|w| w.status.value == WordStatus::Memorized)
}
