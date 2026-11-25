//! AI answers as the AI dictionary keeps them, and the replace offers
//! an Explain answer can raise.
//!
//! A deck's backs go in quietly: a meaning kept before stays. An Explain
//! answer goes in the same way, but when a different meaning was kept
//! for its word, class and language, the card offers to replace it, and
//! nothing changes without that click. Words only their book uses, as
//! names are, stay out.

use std::collections::HashMap;

use mareader_dict::{Reply, Saved, Ticket, language_code};
use mareader_study::Pos;
use mareader_study::mode::{Wanted, Written};

use crate::study::asking::Said;
use crate::study::state::Key;

/// Explain filings whose replies are awaited at most.
const AWAITED: usize = 16;

/// A deck card's back as the AI dictionary keeps it, under its lemma.
pub fn of_back(wanted: &Wanted, back: &Written, (by, language): (&str, &str)) -> Option<Saved> {
    let pos = back.pos.or(wanted.key.pos)?;
    let usages = Some(back.usage.trim()).filter(|u| !u.is_empty());
    let saved = Saved {
        synonyms: back.synonyms.clone(),
        usages: usages.into_iter().map(str::to_owned).collect(),
        model: by.to_owned(),
        ..entry(&wanted.key.lemma, pos, language, &back.meaning)?
    };
    (!back.book_only).then_some(saved)
}

/// An Explain answer as the AI dictionary keeps it, under the word asked.
pub fn of_said(said: &Said, language: &str) -> Option<Saved> {
    let info = &said.info;
    let pos = Pos::read(&info.pos)?;
    let saved = Saved {
        synonyms: info.synonyms.clone(),
        usages: info.usages.clone(),
        model: said.by.clone(),
        ..entry(&said.word, pos, language, &info.meaning)?
    };
    (!info.book_only).then_some(saved)
}

/// The keyed part of an entry; none without a word or a meaning.
fn entry(word: &str, pos: Pos, language: &str, meaning: &str) -> Option<Saved> {
    let (word, meaning) = (word.trim(), meaning.trim());
    if word.is_empty() || meaning.is_empty() {
        return None;
    }
    Some(Saved {
        word: word.to_owned(),
        pos: pos.name().to_owned(),
        lang: language_code(language),
        meaning: meaning.to_owned(),
        added: crate::library::wall_ms(),
        ..Saved::default()
    })
}

/// A kept meaning that differs from a new answer; the reader may swap.
#[derive(Clone, Debug, PartialEq)]
pub struct Offer {
    pub new: Saved,
    pub kept: Saved,
}

/// Explain answers sent to the AI dictionary, and the offers raised.
#[derive(Default)]
pub struct Filing {
    sent: Vec<(Ticket, Key, Saved)>,
    offers: HashMap<Key, Offer>,
}

impl Filing {
    /// Waits for `ticket`'s reply; a newer answer drops the card's offer.
    pub fn sent(&mut self, ticket: Ticket, key: Key, saved: Saved) {
        self.offers.remove(&key);
        self.sent.retain(|(_, k, _)| *k != key);
        if self.sent.len() >= AWAITED {
            self.sent.remove(0);
        }
        self.sent.push((ticket, key, saved));
    }

    /// Takes in the replies `take` has; true when one raised an offer.
    pub fn collect(&mut self, mut take: impl FnMut(Ticket) -> Option<Reply>) -> bool {
        let mut raised = false;
        for (ticket, key, new) in std::mem::take(&mut self.sent) {
            let Some(reply) = take(ticket) else {
                self.sent.push((ticket, key, new));
                continue;
            };
            if let Some(kept) = reply.saved.into_iter().next() {
                self.offers.insert(key, Offer { new, kept });
                raised = true;
            }
        }
        raised
    }

    pub fn offer(&self, key: Key) -> Option<&Offer> {
        self.offers.get(&key)
    }

    pub fn take_offer(&mut self, key: Key) -> Option<Offer> {
        self.offers.remove(&key)
    }
}

#[cfg(test)]
#[path = "ai_dict_tests.rs"]
mod tests;
