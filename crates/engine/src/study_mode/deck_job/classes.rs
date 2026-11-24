//! Each finished card's class, by the one rule ([`Classes::class`]).
//!
//! The AI's class leads, then the tagger's, then the dictionary's. On an
//! AI deck, cards neither named are looked up once; a dictionary silent
//! for [`MEANING_WAIT`] seconds leaves them without. A class is part of
//! a card's key, so settling may join two cards into one, or meet a word
//! memorized under that class.

use std::collections::HashMap;

use mareader_study::mode::{Classes, Key, Via, also_as};

use super::{DeckJob, MEANING_WAIT, Stage};
use crate::study_mode::backs::{Shown, class_in};
use crate::study_mode::known::Known;
use crate::word_list::Meanings;

impl DeckJob {
    /// Pairs the AI's backs with their cards, then settles their classes.
    pub(super) fn finish_written(&mut self) {
        let mut backs = std::mem::take(&mut self.written).into_iter();
        let cards = std::mem::take(&mut self.deck.cards).into_iter();
        let cards = cards.map(|card| {
            let back = backs.next().flatten();
            let named = back.as_ref().and_then(|b| b.pos);
            (Shown::written(card, back), named)
        });
        self.settling = cards.collect();
        self.stage = Stage::Settle;
    }

    /// Asks the dictionary for the classes still unnamed, else finishes.
    pub(super) fn settle(&mut self, meanings: &mut dyn Meanings) -> bool {
        let mut unnamed: Vec<String> = Vec::new();
        for (shown, named) in &self.settling {
            let key = &shown.card.key;
            let none = named.is_none() && key.pos.is_none();
            let asked = self.dictionary.contains_key(&key.lemma) || unnamed.contains(&key.lemma);
            if none && !asked {
                unnamed.push(key.lemma.clone());
            }
        }
        let asking = self.via == Via::Ai && !unnamed.is_empty();
        match (asking, self.to.clone()) {
            (true, Some(to)) => {
                let ticket = meanings.ask_words(unnamed, vec![to], false);
                self.stage = Stage::Classes {
                    ticket,
                    since: None,
                };
            }
            _ => self.finish_settled(),
        }
        true
    }

    /// Takes the dictionary's classes in, or gives up on them in time.
    pub(super) fn classes(&mut self, meanings: &mut dyn Meanings, now: f64) -> bool {
        let Stage::Classes { ticket, since } = &mut self.stage else {
            return false;
        };
        let waited = now - *since.get_or_insert(now);
        match meanings.take_words(*ticket) {
            Some(reply) => {
                for found in reply.found {
                    if let Some(class) = class_in(&found.own) {
                        self.dictionary.entry(found.word).or_insert(class);
                    }
                }
            }
            None if waited <= MEANING_WAIT => return false,
            None => {}
        }
        self.finish_settled();
        true
    }

    fn finish_settled(&mut self) {
        let cards = std::mem::take(&mut self.settling);
        let dictionary = &self.dictionary;
        let heard = cards.into_iter().map(|(shown, named)| {
            let classes = Classes {
                ai: named,
                tagger: shown.card.key.pos,
                dictionary: dictionary.get(&shown.card.key.lemma).copied(),
            };
            (shown, classes)
        });
        let (cards, known) = settled(heard.collect(), &self.known);
        let deck = std::mem::take(&mut self.deck);
        self.done(cards, deck.found, deck.skipped_known.saturating_add(known));
    }
}

/// Each card keyed by its class, cards then alike joined, memorized
/// ones left out; and how many were left out.
pub(super) fn settled(cards: Vec<(Shown, Classes)>, known: &Known) -> (Vec<Shown>, usize) {
    let mut out: Vec<Shown> = Vec::new();
    let mut at: HashMap<Key, usize> = HashMap::new();
    let mut memorized = 0;
    for (mut shown, classes) in cards {
        shown.card.key.pos = classes.class();
        if known.has(&shown.card.key) {
            memorized += 1;
            continue;
        }
        match at.get(&shown.card.key).and_then(|&i| out.get_mut(i)) {
            Some(kept) => join(kept, shown),
            None => {
                at.insert(shown.card.key.clone(), out.len());
                out.push(shown);
            }
        }
    }
    let keys: Vec<Key> = out.iter().map(|s| s.card.key.clone()).collect();
    for shown in &mut out {
        shown.card.also = also_as(&keys, &shown.card.key);
    }
    (out, memorized)
}

/// `more` counted into `kept`; its back stands in for a missing one.
fn join(kept: &mut Shown, more: Shown) {
    let card = &mut kept.card;
    card.count = card.count.saturating_add(more.card.count);
    card.band = card.band.max(more.card.band);
    if kept.meaning.is_empty() {
        kept.meaning = more.meaning;
        kept.synonyms = more.synonyms;
        kept.usage = more.usage;
    }
}

#[cfg(test)]
#[allow(clippy::indexing_slicing)]
mod tests {
    use mareader_study::mode::{Card, Pos};

    use super::*;

    fn shown(lemma: &str, tagger: Option<Pos>, meaning: &str) -> Shown {
        let card = Card {
            key: Key::new(lemma, tagger),
            form: lemma.to_owned(),
            sentence: String::new(),
            band: 5,
            count: 1,
            also: Vec::new(),
        };
        Shown::new(card, meaning.to_owned())
    }

    fn heard(ai: Option<Pos>, tagger: Option<Pos>, dictionary: Option<Pos>) -> Classes {
        Classes {
            ai,
            tagger,
            dictionary,
        }
    }

    #[test]
    fn each_card_keeps_the_first_class_named_ai_tagger_dictionary() {
        let (n, v, a) = (Some(Pos::Noun), Some(Pos::Verb), Some(Pos::Adjective));
        let cards = vec![
            (shown("vex", v, "annoy"), heard(n, v, a)),
            (shown("brisk", None, "quick"), heard(None, None, a)),
            (shown("ponder", v, "think"), heard(None, v, n)),
            (shown("xyz", None, "?"), heard(None, None, None)),
        ];
        let (out, memorized) = settled(cards, &Known::default());
        let classes: Vec<Option<Pos>> = out.iter().map(|s| s.card.key.pos).collect();
        assert_eq!(classes, [n, a, v, None]);
        assert_eq!(memorized, 0);
    }

    #[test]
    fn cards_the_same_once_settled_join_and_memorized_ones_leave() {
        let (n, v) = (Some(Pos::Noun), Some(Pos::Verb));
        let cards = vec![
            (shown("visit", v, "go see"), heard(None, v, n)),
            (shown("visit", None, ""), heard(None, None, v)),
            (shown("visit", n, "a trip"), heard(None, n, n)),
            (shown("solace", None, "comfort"), heard(None, None, n)),
        ];
        let known = Known::of([Key::new("solace", n)]);
        let (out, memorized) = settled(cards, &known);
        assert_eq!(memorized, 1, "known under the class it settled on");
        assert_eq!(out.len(), 2, "the classless visit joined the verb");
        assert_eq!(out[0].card.count, 2);
        assert_eq!(out[0].meaning, "go see");
        assert_eq!(out[0].card.also, [Pos::Noun]);
        assert_eq!(out[1].card.also, [Pos::Verb]);
    }
}
