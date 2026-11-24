//! A dictionary deck's lookups: rounds of words until the deck is full,
//! keeping only the words a dictionary has a meaning for.

use std::collections::HashMap;

use mareader_dict::Entry;
use mareader_study::mode::{Card, Classes, Missing};

use super::{DeckJob, MEANING_WAIT, Stage};
use crate::study_mode::Setup;
use crate::study_mode::backs::{Shown, class_in, meaning_of};
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
            self.setup = Some(Setup::Writer);
            return Err(Missing::Cards.reason().to_owned());
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
        for (word, entries) in &found {
            if let Some(class) = class_in(entries) {
                self.dictionary.entry(word.clone()).or_insert(class);
            }
        }
        let round = self.deck.cards.iter().take(self.upto).skip(self.looked);
        let room = self.max.saturating_sub(self.kept.len());
        let dictionary = &self.dictionary;
        let meaning = |card: &Card| {
            let entries = found.get(&card.key.lemma).map_or(&[][..], Vec::as_slice);
            let heard = Classes {
                ai: None,
                tagger: card.key.pos,
                dictionary: dictionary.get(&card.key.lemma).copied(),
            };
            meaning_of(entries, heard.class())
        };
        let kept = with_meanings(round, meaning, room);
        self.kept.extend(kept);
        self.looked = self.upto;
        self.look_up(meanings)?;
        Ok(true)
    }

    /// Settles the dictionary cards kept; no AI names their classes.
    pub(super) fn finish(&mut self) {
        let kept = std::mem::take(&mut self.kept).into_iter();
        self.settling = kept.map(|shown| (shown, None)).collect();
        self.stage = Stage::Settle;
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
