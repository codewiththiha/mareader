//! Deck building: hard words deduped by lemma and word class.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

/// A coarse word class; the second half of a word's study key.
#[derive(
    Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize,
)]
#[serde(rename_all = "snake_case")]
pub enum WordClass {
    Noun,
    Verb,
    Adjective,
    Adverb,
    #[default]
    Other,
}

impl WordClass {
    /// The class a tagger kind names, else [`WordClass::Other`].
    pub fn of(kind: &str) -> Self {
        match kind.trim().to_ascii_lowercase().as_str() {
            "noun" | "n" => Self::Noun,
            "verb" | "v" => Self::Verb,
            "adjective" | "adj" => Self::Adjective,
            "adverb" | "adv" => Self::Adverb,
            _ => Self::Other,
        }
    }

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

/// One word to study: a normalised lemma in one class.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct WordKey {
    pub lemma: String,
    pub class: WordClass,
}

impl WordKey {
    pub fn new(lemma: impl Into<String>, class: WordClass) -> Self {
        Self {
            lemma: lemma.into().trim().to_lowercase(),
            class,
        }
    }

    /// The stable `lemma|class` form, as caches and records key it.
    pub fn id(&self) -> String {
        format!("{}|{}", self.lemma, self.class.label())
    }

    /// Parses an [`WordKey::id`] back, if it has that shape.
    pub fn parse(id: &str) -> Option<Self> {
        let (lemma, class) = id.rsplit_once('|')?;
        Some(Self::new(lemma, WordClass::of(class)))
    }
}

/// One sighting of a hard word in a batch's text.
#[derive(Clone, Debug, PartialEq)]
pub struct SeenWord {
    pub key: WordKey,
    /// CEFR band 1 (A1) to 6 (C2); 0 is unknown.
    pub band: u8,
    /// The book's own sentence, first occurrence kept.
    pub sentence: String,
    /// Proper nouns are skipped.
    pub proper: bool,
}

/// One flash card before its meaning is written.
#[derive(Clone, Debug, PartialEq)]
pub struct CardCandidate {
    pub key: WordKey,
    pub band: u8,
    /// Appearances in the batch.
    pub count: usize,
    pub sentence: String,
    /// Same lemma in other classes, in this deck.
    pub also: Vec<WordClass>,
}

/// A built deck and what was set aside.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Deck {
    pub cards: Vec<CardCandidate>,
    /// Candidates before the cap.
    pub found: usize,
    /// Distinct memorized words skipped.
    pub skipped_known: usize,
}

/// Builds a deck from `seen` in reading order.
pub fn build_deck(
    seen: &[SeenWord],
    level_band: u8,
    known: &HashSet<WordKey>,
    max_cards: usize,
) -> Deck {
    let max_cards = max_cards.clamp(1, 30);
    let mut found: HashMap<WordKey, CardCandidate> = HashMap::new();
    let mut order: Vec<WordKey> = Vec::new();
    let mut known_seen: HashSet<WordKey> = HashSet::new();
    for word in seen {
        if word.proper || word.band <= level_band || word.band == 0 {
            continue;
        }
        if known.contains(&word.key) {
            known_seen.insert(word.key.clone());
            continue;
        }
        match found.get_mut(&word.key) {
            Some(hit) => hit.count = hit.count.saturating_add(1),
            None => {
                order.push(word.key.clone());
                found.insert(
                    word.key.clone(),
                    CardCandidate {
                        key: word.key.clone(),
                        band: word.band,
                        count: 1,
                        sentence: word.sentence.clone(),
                        also: Vec::new(),
                    },
                );
            }
        }
    }
    let mut cards: Vec<CardCandidate> =
        order.iter().filter_map(|k| found.get(k).cloned()).collect();
    let lemmas: Vec<(String, WordClass)> = cards
        .iter()
        .map(|c| (c.key.lemma.clone(), c.key.class))
        .collect();
    for card in &mut cards {
        card.also = lemmas
            .iter()
            .filter(|(lemma, class)| *lemma == card.key.lemma && *class != card.key.class)
            .map(|(_, class)| *class)
            .collect();
    }
    cards.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| b.band.cmp(&a.band))
            .then_with(|| {
                let pa = order.iter().position(|k| k == &a.key).unwrap_or(usize::MAX);
                let pb = order.iter().position(|k| k == &b.key).unwrap_or(usize::MAX);
                pa.cmp(&pb)
            })
    });
    let found_len = cards.len();
    cards.truncate(max_cards);
    Deck {
        cards,
        found: found_len,
        skipped_known: known_seen.len(),
    }
}

/// Whether `attempt` only repeats the word itself.
pub fn repeats_word(attempt: &str, lemma: &str) -> bool {
    let words: Vec<String> = attempt
        .to_lowercase()
        .split(|c: char| !c.is_alphabetic() && c != '\'')
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect();
    let lemma = lemma.trim().to_lowercase();
    words.len() == 1 && words.first().map(|w| w.as_str()) == Some(lemma.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen(lemma: &str, class: WordClass, band: u8) -> SeenWord {
        SeenWord {
            key: WordKey::new(lemma, class),
            band,
            sentence: format!("A {lemma} sentence."),
            proper: false,
        }
    }

    #[test]
    fn same_lemma_and_class_is_one_card() {
        let words = vec![
            seen("fortune", WordClass::Noun, 4),
            seen("fortune", WordClass::Noun, 4),
            seen("fortune", WordClass::Noun, 4),
        ];
        let deck = build_deck(&words, 3, &HashSet::new(), 12);
        assert_eq!(deck.cards.len(), 1);
        assert_eq!(deck.cards.first().map(|c| c.count), Some(3));
    }

    #[test]
    fn same_lemma_other_class_is_its_own_card() {
        let words = vec![
            seen("visit", WordClass::Verb, 3),
            seen("visit", WordClass::Noun, 4),
        ];
        let deck = build_deck(&words, 2, &HashSet::new(), 12);
        assert_eq!(deck.cards.len(), 2);
        assert!(deck.cards.iter().all(|c| !c.also.is_empty()));
    }

    #[test]
    fn memorized_and_easy_words_are_set_aside() {
        let mut known = HashSet::new();
        known.insert(WordKey::new("fortune", WordClass::Noun));
        let words = vec![
            seen("fortune", WordClass::Noun, 4),
            seen("easy", WordClass::Noun, 2),
            seen("hard", WordClass::Noun, 6),
        ];
        let deck = build_deck(&words, 3, &known, 12);
        assert_eq!(deck.cards.len(), 1);
        assert_eq!(deck.skipped_known, 1);
    }

    #[test]
    fn ranking_is_most_repeated_then_hardest_and_capped() {
        let mut words = vec![seen("rare", WordClass::Noun, 6)];
        words.extend((0..3).map(|_| seen("often", WordClass::Verb, 4)));
        let deck = build_deck(&words, 3, &HashSet::new(), 1);
        assert_eq!(deck.found, 2);
        assert_eq!(
            deck.cards.first().map(|c| c.key.lemma.as_str()),
            Some("often")
        );
    }

    #[test]
    fn keys_round_trip_and_repeats_are_caught() {
        let key = WordKey::new(" Acknowledge ", WordClass::Verb);
        assert_eq!(key.id(), "acknowledge|verb");
        assert_eq!(WordKey::parse("acknowledge|verb"), Some(key));
        assert!(repeats_word("acknowledge", "acknowledge"));
        assert!(!repeats_word("to accept as true", "acknowledge"));
    }
}
