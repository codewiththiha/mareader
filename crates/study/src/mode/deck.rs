//! The cards before a batch, and how long they may stay open (D1, D2, D4).
//!
//! One card stands for one key: the same word in the same role is one card
//! however often it shows, and the same lemma in another role is its own,
//! linked as "also as". Words already memorized are dropped here, when the
//! deck is built, so a cached meaning never brings one back.

use std::collections::{HashMap, HashSet};

use super::keys::Key;
use crate::class::Pos;

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

/// The other classes `key`'s lemma has among `keys`, as "also as" says.
pub fn also_as(keys: &[Key], key: &Key) -> Vec<Pos> {
    (keys.iter())
        .filter(|k| k.lemma == key.lemma && k.pos != key.pos)
        .filter_map(|k| k.pos)
        .collect()
}

/// A deck's most cards, a guard only: words mode keeps decks near Words per
/// deck, and a bigger deck comes in parts.
pub const MOST_CARDS: usize = 240;

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
        card.also = also_as(&keys, &card.key);
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

/// How long a deck may stay open, spent only while it shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Allowance {
    /// Seconds in all; none sets no limit.
    total: Option<f64>,
    spent: f64,
    /// When it last began to show, while it shows.
    since: Option<f64>,
}

impl Allowance {
    /// `per_card` seconds for each of `cards`, or none for no limit.
    pub fn new(per_card: Option<u32>, cards: usize) -> Self {
        let cards = u32::try_from(cards).unwrap_or(u32::MAX);
        Self {
            total: per_card.map(|s| f64::from(s.saturating_mul(cards))),
            spent: 0.0,
            since: None,
        }
    }

    pub fn limited(&self) -> bool {
        self.total.is_some()
    }

    /// The deck shows from `now`: its time runs.
    pub fn resume(&mut self, now: f64) {
        if self.since.is_none() {
            self.since = Some(finite(now));
        }
    }

    /// The deck went out of sight at `now`: its time stops.
    pub fn pause(&mut self, now: f64) {
        if let Some(at) = self.since.take() {
            self.spent += (finite(now) - at).max(0.0);
        }
    }

    pub fn running(&self) -> bool {
        self.since.is_some()
    }

    /// Seconds still to spend; none without a limit.
    pub fn left(&self, now: f64) -> Option<f64> {
        let running = self.since.map_or(0.0, |at| (finite(now) - at).max(0.0));
        let spent = self.spent + running;
        self.total.map(|total| (total - spent).max(0.0))
    }

    /// Every second is spent: the deck closes for good.
    pub fn spent(&self, now: f64) -> bool {
        self.left(now).is_some_and(|left| left <= 0.0)
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
    fn the_allowance_runs_only_while_the_deck_shows() {
        assert_eq!(Allowance::new(Some(10), 20).left(0.0), Some(200.0), "3:20");
        let mut a = Allowance::new(Some(10), 12);
        assert_eq!(a.left(0.0), Some(120.0));
        a.resume(10.0);
        assert_eq!(a.left(40.0), Some(90.0));
        a.pause(40.0);
        assert!(!a.running());
        assert_eq!(a.left(500.0), Some(90.0), "out of sight, it keeps");
        a.resume(500.0);
        a.resume(520.0);
        assert!(!a.spent(589.0));
        assert!(a.spent(590.0));
        assert_eq!(a.left(f64::NAN), Some(90.0), "a bad clock spends nothing");
    }

    #[test]
    fn no_limit_never_runs_out() {
        let mut a = Allowance::new(None, 12);
        a.resume(0.0);
        assert!(!a.limited());
        assert_eq!(a.left(1e9), None);
        assert!(!a.spent(1e9));
    }
}
