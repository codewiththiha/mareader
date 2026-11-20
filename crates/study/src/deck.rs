//! Building a flash-card deck from the hard words a batch found.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::mode::word_key;

/// One hard word the tagger found, before dedupe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub lemma: String,
    /// Coarse class: noun, verb, adjective, adverb.
    pub pos: String,
    /// CEFR band 1–6.
    pub band: u8,
    /// The book's own sentence, first time this key appears.
    pub sentence: String,
}

/// One card in the deck: lemma and class, ranked.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Card {
    pub lemma: String,
    pub pos: String,
    pub band: u8,
    /// How many times this key appeared in the batch.
    pub count: usize,
    pub sentence: String,
    /// Other classes of the same lemma in this deck.
    pub also: Vec<String>,
}

/// The deck for a stretch of pages, and what was skipped building it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Deck {
    pub cards: Vec<Card>,
    /// Candidates before the cap.
    pub found: usize,
    /// Memorized keys dropped at show time.
    pub skipped_known: usize,
}

/// Dedupes by (lemma, class), drops memorized keys, ranks and caps.
///
/// Ranking: most repeated first, then hardest, then first-seen order.
/// Memorized words are filtered here, at show time, so a cached answer
/// never goes stale when a word is marked known elsewhere.
pub fn build_deck(candidates: &[Candidate], known: &HashSet<String>, max: usize) -> Deck {
    let mut found: HashMap<String, (Candidate, usize, usize)> = HashMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut skipped = HashSet::new();
    for (i, c) in candidates.iter().enumerate() {
        let key = word_key(&c.lemma, &c.pos);
        if known.contains(&key) {
            skipped.insert(key);
            continue;
        }
        match found.get_mut(&key) {
            Some(hit) => hit.1 = hit.1.saturating_add(1),
            None => {
                found.insert(key.clone(), (c.clone(), 1, i));
                order.push(key);
            }
        }
    }
    let mut cards: Vec<Card> = order
        .iter()
        .filter_map(|k| found.get(k))
        .map(|(c, count, _at)| Card {
            lemma: c.lemma.clone(),
            pos: c.pos.clone(),
            band: c.band,
            count: *count,
            sentence: c.sentence.clone(),
            also: Vec::new(),
        })
        .collect();
    let found_n = cards.len();
    cross_link(&mut cards);
    cards.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then(b.band.cmp(&a.band))
            .then(a.lemma.cmp(&b.lemma))
            .then(a.pos.cmp(&b.pos))
    });
    cards.truncate(max.max(1));
    Deck {
        cards,
        found: found_n,
        skipped_known: skipped.len(),
    }
}

/// Notes each card's lemma appearing in another class in this deck.
fn cross_link(cards: &mut [Card]) {
    let snapshot: Vec<(String, String, String)> = cards
        .iter()
        .map(|c| (c.lemma.to_lowercase(), c.pos.to_lowercase(), c.pos.clone()))
        .collect();
    for (i, card) in cards.iter_mut().enumerate() {
        let lemma = card.lemma.to_lowercase();
        let pos = card.pos.to_lowercase();
        let mut also: Vec<String> = snapshot
            .iter()
            .enumerate()
            .filter(|(j, (l, p, _))| *j != i && *l == lemma && *p != pos)
            .map(|(_, (_, _, raw))| raw.clone())
            .collect();
        also.sort();
        also.dedup();
        card.also = also;
    }
}

/// The timer floor for a deck: seconds per card times its size.
pub fn deck_seconds(cards: usize, sec_per_card: u32) -> u32 {
    (cards as u32).saturating_mul(sec_per_card)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn cand(lemma: &str, pos: &str, band: u8) -> Candidate {
        Candidate {
            lemma: lemma.into(),
            pos: pos.into(),
            band,
            sentence: format!("A sentence with {lemma}."),
        }
    }

    #[test]
    fn same_key_collapses_to_one_card() {
        let cands = vec![
            cand("fortune", "noun", 4),
            cand("fortune", "noun", 4),
            cand("fortune", "noun", 4),
        ];
        let deck = build_deck(&cands, &HashSet::new(), 12);
        assert_eq!(deck.cards.len(), 1);
        assert_eq!(deck.cards[0].count, 3);
        assert_eq!(deck.found, 1);
    }

    #[test]
    fn same_lemma_other_pos_is_its_own_card() {
        let cands = vec![cand("visit", "verb", 3), cand("visit", "noun", 4)];
        let deck = build_deck(&cands, &HashSet::new(), 12);
        assert_eq!(deck.cards.len(), 2);
        assert!(deck.cards.iter().all(|c| c.also.len() == 1));
    }

    #[test]
    fn memorized_keys_are_dropped_at_show_time() {
        let cands = vec![cand("fortune", "noun", 4), cand("merely", "adverb", 5)];
        let known = HashSet::from([word_key("fortune", "noun")]);
        let deck = build_deck(&cands, &known, 12);
        assert_eq!(deck.cards.len(), 1);
        assert_eq!(deck.cards[0].lemma, "merely");
        assert_eq!(deck.skipped_known, 1);
    }

    #[test]
    fn rank_is_count_then_band_then_cap() {
        let cands = vec![
            cand("rare", "adjective", 6),
            cand("common", "noun", 4),
            cand("common", "noun", 4),
            cand("mid", "verb", 5),
        ];
        let deck = build_deck(&cands, &HashSet::new(), 2);
        assert_eq!(deck.cards.len(), 2);
        assert_eq!(deck.cards[0].lemma, "common");
        assert_eq!(deck.cards[1].lemma, "rare");
        assert_eq!(deck.found, 3);
    }

    #[test]
    fn timer_scales_with_deck_and_known_marks_shrink_it() {
        assert_eq!(deck_seconds(12, 6), 72);
        assert_eq!(deck_seconds(0, 6), 0);
    }
}
