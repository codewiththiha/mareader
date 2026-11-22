//! A dictionary deck's lookups: rounds of words until the deck is full,
//! keeping only the words a dictionary has a meaning for.

use std::collections::HashMap;

use mareader_dict::Entry;
use mareader_study::mode::Card;

use super::{DeckJob, MEANING_WAIT, Stage};
use crate::study_mode::backs::{Shown, meaning_of};
use crate::word_list::Meanings;

/// Words asked of the dictionaries at a time, so word cards still answer.
const LOOKUP_ROUND: usize = 48;
/// The error card's words when the dictionary never answered.
const DICTIONARY_SILENT: &str = "The dictionary didn’t answer in time.";

impl DeckJob {
    /// Asks the dictionaries for the next round of words; once the deck is
    /// full or every word is asked, finishes.
    pub(super) fn look_up(&mut self, meanings: &mut dyn Meanings) -> Result<(), String> {
        let from = self.looked;
        let upto = (from + LOOKUP_ROUND).min(self.deck.cards.len());
        if self.kept.len() >= self.max || from >= upto {
            self.finish();
            return Ok(());
        }
        let Some(to) = self.to.clone() else {
            // With no pack at all, as in a headless run, the words come bare.
            let bare = |c: &Card| Shown::new(c.clone(), String::new());
            let cards = self.deck.cards.iter().take(self.max);
            self.kept = cards.map(bare).collect();
            self.finish();
            return Ok(());
        };
        let mut words: Vec<String> = Vec::new();
        for card in self.deck.cards.iter().take(upto).skip(from) {
            if !words.contains(&card.key.lemma) {
                words.push(card.key.lemma.clone());
            }
        }
        let ticket = meanings.ask_words(words, vec![to], false);
        self.upto = upto;
        self.stage = Stage::Meanings {
            ticket,
            since: None,
        };
        Ok(())
    }

    /// Takes a round's meanings, then asks the next round or finishes.
    pub(super) fn meanings(
        &mut self,
        meanings: &mut dyn Meanings,
        now: f64,
    ) -> Result<bool, String> {
        let Stage::Meanings { ticket, since } = &mut self.stage else {
            return Ok(false);
        };
        let waited = now - *since.get_or_insert(now);
        let Some(reply) = meanings.take_words(*ticket) else {
            if waited <= MEANING_WAIT {
                return Ok(false);
            }
            if self.kept.is_empty() {
                return Err(DICTIONARY_SILENT.to_owned());
            }
            self.finish();
            return Ok(true);
        };
        let found = reply.found.into_iter().map(|f| (f.word, f.own));
        let found: HashMap<String, Vec<Entry>> = found.collect();
        let round = self.deck.cards.iter().take(self.upto).skip(self.looked);
        let room = self.max.saturating_sub(self.kept.len());
        let meaning = |card: &Card| {
            let entries = found.get(&card.key.lemma).map_or(&[][..], Vec::as_slice);
            meaning_of(entries, card.key.pos)
        };
        let kept = with_meanings(round, meaning, room);
        self.kept.extend(kept);
        self.looked = self.upto;
        self.look_up(meanings)?;
        Ok(true)
    }

    /// Hands over the dictionary cards kept.
    pub(super) fn finish(&mut self) {
        let deck = std::mem::take(&mut self.deck);
        let cards = std::mem::take(&mut self.kept);
        self.done(cards, deck.found, deck.skipped_known);
    }
}

/// The cards of `round` with a meaning, `room` at most, in deck order.
fn with_meanings<'a>(
    round: impl Iterator<Item = &'a Card>,
    meaning: impl Fn(&Card) -> String,
    room: usize,
) -> Vec<Shown> {
    let shown = round.map(|card| Shown::new(card.clone(), meaning(card)));
    shown.filter(|s| !s.meaning.is_empty()).take(room).collect()
}

#[cfg(test)]
mod tests {
    use mareader_study::mode::Key;

    use super::*;

    #[test]
    fn dictionary_cards_are_only_words_it_has() {
        let card = |word: &str| Card {
            key: Key::new(word, None),
            form: word.to_owned(),
            sentence: String::new(),
            band: 5,
            count: 1,
            also: Vec::new(),
        };
        let cards = [card("ponder"), card("xyz"), card("arcane"), card("lintel")];
        let meaning = |c: &Card| match c.key.lemma.as_str() {
            "xyz" => String::new(),
            word => format!("{word}, meant"),
        };
        let kept = with_meanings(cards.iter(), meaning, 2);
        let words: Vec<&str> = kept.iter().map(|s| s.card.key.lemma.as_str()).collect();
        assert_eq!(words, ["ponder", "arcane"], "only words it has");
        assert!(kept.iter().all(|s| !s.meaning.is_empty()));
    }
}
