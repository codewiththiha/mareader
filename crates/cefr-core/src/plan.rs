//! The DOM-free half of a highlight walk: marks to paint, keys to ask.

use crate::cache::LevelCache;
use crate::text::{
    MAX_WORD_CHARS, Span, hyphen_parts, is_english_ascii, lookup_candidates, sentence_around,
};

/// One word to paint: where it sits, what it says, the window a click
/// sends.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedWord {
    /// The word's span, in characters, in the run the walk read.
    pub start: usize,
    pub end: usize,
    pub word: String,
    /// The ±60-character window around it; the tagger's context.
    pub context: String,
}

/// One walk's answer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Walk {
    /// The words above the reader's band, first in reading order, capped.
    pub paint: Vec<PlannedWord>,
    /// The keys no answer covers yet: what the dataset is owed.
    pub ask: Vec<String>,
}

/// Plan a pure walk over one run of text: same inputs, same plan.
pub fn walk(text: &str, tokens: &[Span], cache: &LevelCache, threshold: u8, cap: usize) -> Walk {
    let chars: Vec<char> = text.chars().collect();
    let known: Vec<(Span, Vec<String>)> = tokens
        .iter()
        .filter_map(|span| {
            if span.end > chars.len() || span.len() > MAX_WORD_CHARS {
                return None;
            }
            let word: String = chars[span.start..span.end].iter().collect();
            // The gate keeps a hand-built span list out of the dataset.
            is_english_ascii(&word).then(|| (*span, keys_of(&word)))
        })
        .collect();
    let ask = cache.misses(
        known
            .iter()
            .flat_map(|(_, keys)| keys.iter().map(String::as_str)),
    );
    let paint = known
        .iter()
        .filter(|(_, keys)| cache.any_above(keys, threshold))
        .take(cap)
        .map(|(span, _)| PlannedWord {
            start: span.start,
            end: span.end,
            word: chars[span.start..span.end].iter().collect(),
            context: sentence_around(text, span.start, span.end),
        })
        .collect();
    Walk { paint, ask }
}

/// The keys one token is probed as, most specific first: itself, its
/// base, its parts.
pub fn keys_of(word: &str) -> Vec<String> {
    let mut keys = lookup_candidates(word);
    keys.extend(hyphen_parts(word));
    keys
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::tokenize;

    /// The walk of `text` over a cache holding `bands`, capped at 200.
    fn walked(text: &str, bands: &[(&str, u8)], threshold: u8) -> Walk {
        let mut cache = LevelCache::default();
        for (word, band) in bands {
            cache.insert(word, *band);
        }
        walk(text, &tokenize(text), &cache, threshold, 200)
    }

    #[test]
    fn only_words_strictly_above_the_band_are_planned() {
        let plan = walked("run ephemeral", &[("run", 2), ("ephemeral", 6)], 4);
        assert_eq!(plan.paint.len(), 1);
        assert_eq!(plan.paint[0].word, "ephemeral");
        assert_eq!((plan.paint[0].start, plan.paint[0].end), (4, 13));
    }

    #[test]
    fn a_word_at_the_readers_own_band_is_not_planned() {
        assert!(walked("run", &[("run", 4)], 4).paint.is_empty());
        assert_eq!(walked("run", &[("run", 5)], 4).paint.len(), 1);
    }

    #[test]
    fn unanswered_keys_are_asked_once_each_in_reading_order() {
        let plan = walked("ephemeral ephemeral palimpsest", &[], 4);
        assert_eq!(
            plan.ask,
            vec!["ephemeral".to_string(), "palimpsest".to_string()]
        );
        // Nothing is decidable yet, so nothing is planned.
        assert!(plan.paint.is_empty());
        assert_eq!(walked("run", &[("run", 2)], 4).ask, Vec::<String>::new());
    }

    #[test]
    fn a_contraction_marks_through_its_base() {
        // The dataset stores no apostrophes: `don't` answers as `not`.
        let plan = walked("don't stop", &[("not", 6), ("stop", 1)], 4);
        assert_eq!(plan.paint.len(), 1);
        assert_eq!(plan.paint[0].word, "don't");
    }

    #[test]
    fn a_hyphenated_token_answers_through_its_parts() {
        let plan = walked("well-known fact", &[("known", 6)], 4);
        assert_eq!(plan.paint.len(), 1);
        assert_eq!(plan.paint[0].word, "well-known");
        assert!(
            walked("well-known", &[], 4)
                .ask
                .contains(&"well".to_string())
        );
    }

    #[test]
    fn the_cap_bounds_the_plan_but_not_the_ask() {
        let text = "alpha beta gamma delta";
        let bands = [("alpha", 6), ("beta", 6), ("gamma", 6), ("delta", 6)];
        let mut cache = LevelCache::default();
        for (word, band) in bands {
            cache.insert(word, band);
        }
        let plan = walk(text, &tokenize(text), &cache, 4, 2);
        assert_eq!(plan.paint.len(), 2);
        assert_eq!(plan.paint[0].word, "alpha");
        assert_eq!(plan.paint[1].word, "beta");
    }

    #[test]
    fn a_planned_word_carries_its_own_context_window() {
        let plan = walked("the ephemeral thing", &[("ephemeral", 6)], 4);
        assert_eq!(plan.paint[0].context, "the ephemeral thing");
    }

    #[test]
    fn a_token_the_dataset_cannot_hold_is_never_asked() {
        let long = "x".repeat(MAX_WORD_CHARS + 1);
        let plan = walked(&long, &[], 4);
        assert!(plan.ask.is_empty() && plan.paint.is_empty());
        // A span past the end of the text is refused, not sliced.
        let beyond = walk("run", &[Span::new(1, 9)], &LevelCache::default(), 4, 200);
        assert!(beyond.ask.is_empty() && beyond.paint.is_empty());
    }

    #[test]
    fn keys_are_probed_most_specific_first() {
        assert_eq!(keys_of("palimpsest"), vec!["palimpsest".to_string()]);
        assert_eq!(
            keys_of("don't"),
            vec!["don't".to_string(), "not".to_string()]
        );
        assert_eq!(
            keys_of("well-known"),
            vec![
                "well-known".to_string(),
                "well".to_string(),
                "known".to_string()
            ]
        );
    }
}
