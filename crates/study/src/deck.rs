//! Building a study deck from the hard words in a batch's pages.
//!
//! Pure: one card per `(lemma, class)`, the most repeated and hardest first,
//! capped, and memorized words dropped at show time so a cached answer never
//! goes stale. See the plan, decision D4.

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

/// A word's coarse class, as the CEFR tagger names it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WordClass {
    #[default]
    Noun,
    Verb,
    Adjective,
    Adverb,
    Other,
}

impl WordClass {
    pub const ALL: [WordClass; 5] = [
        Self::Noun,
        Self::Verb,
        Self::Adjective,
        Self::Adverb,
        Self::Other,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Noun => "noun",
            Self::Verb => "verb",
            Self::Adjective => "adjective",
            Self::Adverb => "adverb",
            Self::Other => "other",
        }
    }
}

/// A hard word the reader has met: its identity, its class in context, its
/// CEFR band. `key` is `lemma|class` and is the dedupe key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HardWord {
    pub key: String,
    pub lemma: String,
    pub class: WordClass,
    /// CEFR band, 1 (A1) to 6 (C2).
    pub band: u8,
}

/// One card: the word, the book's own sentence where it first shows on these
/// pages, and how often the word appears (collapsed duplicates count).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeckCard {
    pub key: String,
    pub lemma: String,
    pub class: WordClass,
    pub band: u8,
    pub count: usize,
    /// The first sentence that uses this word on these pages.
    pub sentence: String,
    /// Other classes of the same lemma in this deck ("also as a noun").
    pub also: Vec<String>,
}

impl DeckCard {
    /// Whether another card is the same lemma in another class.
    fn has_other_class(&self, other: &DeckCard) -> bool {
        self.lemma == other.lemma && self.class != other.class
    }
}

/// One hard sighting with the sentence it first appears in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HardSpan {
    pub word: HardWord,
    /// The sentence this word first appears in, on these pages.
    pub sentence: String,
}

/// A built deck: the cards to study, plus bookkeeping the UI reports.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Deck {
    pub cards: Vec<DeckCard>,
    /// Distinct hard words found before the cap, including ones skipped as
    /// already memorized.
    pub found: usize,
    /// Hard words skipped because they are already memorized.
    pub skipped_known: usize,
}

impl Deck {
    pub fn is_empty(&self) -> bool {
        self.cards.is_empty()
    }
}

/// Default cards per deck; 4 to 30 is the allowed range.
pub const DEFAULT_MAX_CARDS: usize = 12;
pub const MAX_CARDS_MIN: usize = 2;
pub const MAX_CARDS_MAX: usize = 30;

/// Plans a deck: one card per `(lemma, class)`, most repeated and hardest
/// first, at most `max`. Words in `known` are skipped and counted.
///
/// The same surface form in two classes (e.g. "visit" verb vs "visit" noun)
/// is two cards, cross-linked. The same class repeated collapses to one card
/// whose count is its appearances.
pub fn build_deck(spans: &[HardSpan], known: &HashSet<String>, max: usize) -> Deck {
    let mut found: Vec<DeckCard> = Vec::new();
    let mut skipped = 0usize;
    let mut seen_keys: HashSet<String> = HashSet::new();

    for span in spans {
        let is_known = known.contains(&span.word.key);
        if is_known {
            skipped = skipped.saturating_add(1);
        }
        if seen_keys.insert(span.word.key.clone()) {
            if !is_known {
                found.push(DeckCard {
                    key: span.word.key.clone(),
                    lemma: span.word.lemma.clone(),
                    class: span.word.class,
                    band: span.word.band,
                    count: 1,
                    sentence: span.sentence.clone(),
                    also: Vec::new(),
                });
            }
        } else if !is_known
            && let Some(card) = found.iter_mut().find(|card| card.key == span.word.key)
        {
            card.count = card.count.saturating_add(1);
        }
    }

    // Cross-link same-lemma classes; collect immutably.
    let also_by_card: Vec<Vec<String>> = found
        .iter()
        .map(|card| {
            let mut also: Vec<String> = Vec::new();
            for other in &found {
                if card.has_other_class(other) {
                    also.push(other.class.label().to_owned());
                }
            }
            also.sort();
            also.dedup();
            also
        })
        .collect();
    for (card, also) in found.iter_mut().zip(also_by_card) {
        card.also = also;
    }

    // Sort by count then band; stable sort breaks ties.
    found.sort_by(|a, b| b.count.cmp(&a.count).then(b.band.cmp(&a.band)));

    let max = max.clamp(MAX_CARDS_MIN, MAX_CARDS_MAX);
    if found.len() > max {
        found.truncate(max);
    }

    Deck {
        cards: found,
        found: seen_keys.len(),
        skipped_known: skipped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn word(lemma: &str, class: WordClass, band: u8) -> HardWord {
        HardWord {
            key: format!("{}|{}", lemma, class.label()),
            lemma: lemma.to_owned(),
            class,
            band,
        }
    }

    fn span(lemma: &str, class: WordClass, band: u8, sentence: &str) -> HardSpan {
        HardSpan {
            word: word(lemma, class, band),
            sentence: sentence.to_owned(),
        }
    }

    fn first_card(deck: &Deck) -> Option<&DeckCard> {
        deck.cards.first()
    }

    fn second_card(deck: &Deck) -> Option<&DeckCard> {
        deck.cards.get(1)
    }

    fn find_card(deck: &Deck, class: WordClass) -> Option<&DeckCard> {
        deck.cards.iter().find(|c| c.class == class)
    }

    #[test]
    fn repeated_word_in_one_class_collapses_to_one_card_with_its_count() {
        let spans = vec![
            span("fortune", WordClass::Noun, 4, "She made a fortune."),
            span("fortune", WordClass::Noun, 4, "A fortune changed hands."),
            span("fortune", WordClass::Noun, 4, "He lost his fortune."),
        ];
        let deck = build_deck(&spans, &HashSet::new(), 30);
        assert_eq!(deck.cards.len(), 1);
        assert_eq!(first_card(&deck).map(|c| c.lemma.as_str()), Some("fortune"));
        assert_eq!(first_card(&deck).map(|c| c.count), Some(3));
        assert_eq!(deck.found, 1);
        assert_eq!(deck.skipped_known, 0);
    }

    #[test]
    fn same_lemma_two_classes_is_two_cards_cross_linked() {
        let spans = vec![
            span("visit", WordClass::Verb, 3, "We visit her every spring."),
            span("visit", WordClass::Noun, 4, "Visiting was her amusement."),
        ];
        let deck = build_deck(&spans, &HashSet::new(), 30);
        assert_eq!(deck.cards.len(), 2);
        let verb = find_card(&deck, WordClass::Verb);
        let noun = find_card(&deck, WordClass::Noun);
        assert!(
            verb.map(|c| c.also.iter().any(|a| a == "noun"))
                .unwrap_or(false)
        );
        assert!(
            noun.map(|c| c.also.iter().any(|a| a == "verb"))
                .unwrap_or(false)
        );
        assert_eq!(deck.found, 2);
    }

    #[test]
    fn memorized_words_are_dropped_at_show_time_and_counted() {
        let mut known = HashSet::new();
        known.insert("acknowledge|verb".to_owned());
        let spans = vec![
            span("acknowledge", WordClass::Verb, 4, "She acknowledged it."),
            span("possession", WordClass::Noun, 4, "In possession of goods."),
        ];
        let deck = build_deck(&spans, &known, 30);
        assert_eq!(deck.cards.len(), 1);
        assert_eq!(
            first_card(&deck).map(|c| c.lemma.as_str()),
            Some("possession")
        );
        assert_eq!(deck.skipped_known, 1);
        assert_eq!(deck.found, 2);
    }

    #[test]
    fn most_repeated_and_hardest_first_with_a_cap() {
        let spans = vec![
            span("common", WordClass::Adjective, 2, "A common sight."),
            span("common", WordClass::Adjective, 2, "A common error."),
            span("common", WordClass::Adjective, 2, "Quite common."),
            span("rare", WordClass::Adjective, 6, "A rare bird."),
            span("rare", WordClass::Adjective, 6, "As rare as rain."),
            span("rare", WordClass::Adjective, 6, "Very rare indeed."),
            span("tiresome", WordClass::Adjective, 6, "What a tiresome man."),
        ];
        let deck = build_deck(&spans, &HashSet::new(), 2);
        assert_eq!(deck.cards.len(), 2);
        // rare (3×, band 6) beats common (3×, band 2); both beat tiresome (1×).
        assert_eq!(first_card(&deck).map(|c| c.lemma.as_str()), Some("rare"));
        assert_eq!(second_card(&deck).map(|c| c.lemma.as_str()), Some("common"));
        assert_eq!(deck.found, 3);
    }

    #[test]
    fn the_cap_is_clamped_to_the_allowed_range() {
        let spans = vec![span("fortune", WordClass::Noun, 4, "A fortune.")];
        let deck = build_deck(&spans, &HashSet::new(), 0);
        assert_eq!(deck.cards.len(), 1);
        let deck = build_deck(&spans, &HashSet::new(), 100);
        assert_eq!(deck.cards.len(), 1);
    }

    #[test]
    fn an_empty_batch_yields_an_empty_deck() {
        let deck = build_deck(&[], &HashSet::new(), 12);
        assert!(deck.is_empty());
        assert_eq!(deck.found, 0);
    }
}
