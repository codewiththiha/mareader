//! The cards before a batch, and the time they ask (decisions D1, D2, D4).
//!
//! One card stands for one key: the same word in the same role is one card
//! however often it shows, and the same lemma in another role is its own,
//! linked as "also as". Words already memorized are dropped here, when the
//! deck is built, so a cached meaning never brings one back.

use std::collections::{HashMap, HashSet};

use super::keys::{Key, Pos};

/// A hard word where it stands, given in reading order.
#[derive(Clone, Debug, PartialEq)]
pub struct Sighting {
    pub key: Key,
    /// The word as the book spells it.
    pub form: String,
    /// Its CEFR band, 1 (A1) to 6 (C2).
    pub band: u8,
    /// The book's own sentence around it.
    pub sentence: String,
}

/// One word to learn before the batch.
#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    pub key: Key,
    /// The form and sentence of its first sighting.
    pub form: String,
    pub sentence: String,
    pub band: u8,
    /// Sightings in these pages.
    pub count: usize,
    /// The same lemma's other classes in this deck.
    pub also: Vec<Pos>,
}

/// A batch's cards, most frequent first, and what was left out.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Deck {
    pub cards: Vec<Card>,
    /// Cards found before the cap.
    pub found: usize,
    /// Distinct keys skipped as memorized.
    pub skipped_known: usize,
}

/// The deck of words above `level`, at most `max` cards.
///
/// Ranked by sightings, then band, then first sighting.
pub fn build(
    sightings: impl IntoIterator<Item = Sighting>,
    level: u8,
    known: impl Fn(&Key) -> bool,
    max: usize,
) -> Deck {
    let mut cards: Vec<Card> = Vec::new();
    let mut at: HashMap<Key, usize> = HashMap::new();
    let mut skipped: HashSet<Key> = HashSet::new();
    for s in sightings {
        if s.band <= level {
            continue;
        }
        if known(&s.key) {
            skipped.insert(s.key);
            continue;
        }
        if let Some(card) = at.get(&s.key).and_then(|&i| cards.get_mut(i)) {
            card.count = card.count.saturating_add(1);
            card.band = card.band.max(s.band);
            continue;
        }
        at.insert(s.key.clone(), cards.len());
        cards.push(Card {
            key: s.key,
            form: s.form,
            sentence: s.sentence,
            band: s.band,
            count: 1,
            also: Vec::new(),
        });
    }
    let keys: Vec<Key> = cards.iter().map(|c| c.key.clone()).collect();
    for card in &mut cards {
        card.also = (keys.iter())
            .filter(|k| k.lemma == card.key.lemma && k.pos != card.key.pos)
            .filter_map(|k| k.pos)
            .collect();
    }
    // A stable sort keeps reading order among equals.
    cards.sort_by(|a, b| b.count.cmp(&a.count).then(b.band.cmp(&a.band)));
    let found = cards.len();
    cards.truncate(max);
    Deck {
        cards,
        found,
        skipped_known: skipped.len(),
    }
}

/// The time a deck asks before Start: seconds per card, less known cards.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Floor {
    per_card: f64,
    total: f64,
    started: f64,
    credit: f64,
}

impl Floor {
    /// A floor for `cards` cards, counting from `now` in seconds.
    pub fn new(cards: usize, per_card: u32, now: f64) -> Self {
        let per_card = f64::from(per_card);
        Self {
            per_card,
            total: per_card * cards as f64,
            started: finite(now),
            credit: 0.0,
        }
    }

    /// A card marked known gives its seconds back.
    pub fn known(&mut self) {
        self.credit += self.per_card;
    }

    /// An undone known card takes its seconds again.
    pub fn unknown(&mut self) {
        self.credit = (self.credit - self.per_card).max(0.0);
    }

    pub fn total(&self) -> f64 {
        self.total
    }

    /// Seconds still to go, never below zero.
    pub fn left(&self, now: f64) -> f64 {
        let spent = (finite(now) - self.started).max(0.0);
        (self.total - spent - self.credit).max(0.0)
    }

    /// How much of the floor has passed, 0 to 1.
    pub fn progress(&self, now: f64) -> f32 {
        if self.total <= 0.0 {
            return 1.0;
        }
        (1.0 - self.left(now) / self.total).clamp(0.0, 1.0) as f32
    }

    /// Start waits while strict and time is left.
    pub fn locked(&self, strict: bool, now: f64) -> bool {
        strict && self.left(now) > 0.0
    }
}

fn finite(at: f64) -> f64 {
    if at.is_finite() { at } else { 0.0 }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn seen(lemma: &str, pos: Pos, band: u8) -> Sighting {
        Sighting {
            key: Key::new(lemma, Some(pos)),
            form: lemma.to_owned(),
            band,
            sentence: format!("A {lemma} here."),
        }
    }

    fn names(d: &Deck) -> Vec<String> {
        d.cards.iter().map(|c| c.key.id()).collect()
    }

    #[test]
    fn one_card_per_key_ranked_by_count_band_then_order() {
        let words = [
            seen("fortune", Pos::Noun, 4),
            seen("vex", Pos::Verb, 6),
            seen("design", Pos::Noun, 5),
            seen("fortune", Pos::Noun, 4),
            seen("solace", Pos::Noun, 6),
        ];
        let d = build(words, 3, |_| false, 12);
        assert_eq!(
            names(&d),
            ["fortune|noun", "vex|verb", "solace|noun", "design|noun"]
        );
        assert_eq!(d.cards[0].count, 2);
        assert_eq!(d.cards[0].sentence, "A fortune here.");
    }

    #[test]
    fn easy_and_memorized_words_stay_out() {
        let words = [
            seen("settle", Pos::Verb, 3),
            seen("vex", Pos::Verb, 6),
            seen("vex", Pos::Verb, 6),
            seen("caprice", Pos::Noun, 6),
        ];
        let d = build(words, 3, |k| k.lemma == "vex", 12);
        assert_eq!(names(&d), ["caprice|noun"]);
        assert_eq!(d.skipped_known, 1);
    }

    #[test]
    fn a_lemma_in_two_roles_is_two_linked_cards() {
        let words = [seen("visit", Pos::Verb, 4), seen("visit", Pos::Noun, 5)];
        let d = build(words, 3, |_| false, 12);
        assert_eq!(d.cards.len(), 2);
        assert_eq!(d.cards[0].also, [Pos::Verb]);
        assert_eq!(d.cards[1].also, [Pos::Noun]);
    }

    #[test]
    fn the_cap_cuts_cards_but_counts_them() {
        let words = (0..20).map(|i| seen(&format!("w{i}"), Pos::Noun, 5));
        let d = build(words, 3, |_| false, 4);
        assert_eq!((d.cards.len(), d.found), (4, 20));
        assert_eq!(
            build(Vec::<Sighting>::new(), 3, |_| false, 4),
            Deck::default()
        );
    }

    #[test]
    fn the_floor_counts_down_and_known_cards_give_time_back() {
        let mut f = Floor::new(4, 6, 100.0);
        assert_eq!(f.total(), 24.0);
        assert_eq!(f.left(110.0), 14.0);
        f.known();
        assert_eq!(f.left(110.0), 8.0);
        assert!(f.locked(true, 110.0));
        assert!(!f.locked(false, 110.0));
        assert_eq!(f.left(200.0), 0.0);
        assert_eq!(f.progress(200.0), 1.0);
        assert_eq!(f.left(f64::NAN), 18.0);
        assert_eq!(Floor::new(0, 6, 0.0).progress(0.0), 1.0);
    }

    #[test]
    fn an_undone_known_card_takes_its_time_again() {
        let mut f = Floor::new(4, 6, 0.0);
        f.known();
        f.unknown();
        assert_eq!(f.left(0.0), 24.0);
        f.unknown();
        assert_eq!(f.left(0.0), 24.0);
    }
}
