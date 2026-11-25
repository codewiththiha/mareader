//! An AI deck's backs the AI dictionary already keeps.
//!
//! A card whose lemma and class were saved before, in the study language,
//! takes that entry and costs no request; the verb "book" never stands in
//! for the noun. A card with no class is written as usual, and so is
//! every card when the AI dictionary doesn't answer in time.

use mareader_dict::{Saved, fold, language_code};
use mareader_study::mode::{Pos, Written};

use super::{DeckJob, MEANING_WAIT, Stage};
use crate::study_mode::backs::Writer;
use crate::word_list::Meanings;

/// Synonyms a kept back shows at most, as a written one has.
const SYNONYMS: usize = 3;

impl DeckJob {
    /// Asks the AI dictionary about the unwritten cards with a class;
    /// true when it was asked.
    pub(super) fn ask_kept(&mut self, writer: &Writer, meanings: &mut dyn Meanings) -> bool {
        let Some(language) = writer.cards.language() else {
            return false;
        };
        let mut words: Vec<String> = Vec::new();
        for (card, written) in self.deck.cards.iter().zip(&self.written) {
            let open = written.is_none() && card.key.pos.is_some();
            if open && !words.contains(&card.key.lemma) {
                words.push(card.key.lemma.clone());
            }
        }
        if words.is_empty() {
            return false;
        }
        let Some(ticket) = meanings.ask_kept(words, language_code(language)) else {
            return false;
        };
        self.stage = Stage::Reusing {
            ticket,
            since: None,
        };
        true
    }

    /// Takes the kept backs, then asks the writer for the rest.
    pub(super) fn reusing(&mut self, meanings: &mut dyn Meanings, now: f64) -> bool {
        let Stage::Reusing { ticket, since } = &mut self.stage else {
            return false;
        };
        let waited = now - *since.get_or_insert(now);
        let reply = meanings.take_kept(*ticket);
        if reply.is_none() && waited <= MEANING_WAIT {
            return false;
        }
        let kept = reply.map(|r| r.saved).unwrap_or_default();
        for (card, slot) in self.deck.cards.iter().zip(&mut self.written) {
            let Some(pos) = card.key.pos.filter(|_| slot.is_none()) else {
                continue;
            };
            let lemma = fold(&card.key.lemma);
            let same = |s: &&Saved| fold(&s.word) == lemma && s.pos == pos.name();
            *slot = kept.iter().find(same).map(|s| back(s, pos));
        }
        match self.writer.clone() {
            Some(writer) => self.write_rest(&writer),
            None => self.finish_written(),
        }
        true
    }
}

/// A kept entry as a deck card's back.
fn back(saved: &Saved, pos: Pos) -> Written {
    Written {
        pos: Some(pos),
        meaning: saved.meaning.clone(),
        synonyms: saved.synonyms.iter().take(SYNONYMS).cloned().collect(),
        usage: saved.usages.first().cloned().unwrap_or_default(),
        book_only: false,
    }
}
