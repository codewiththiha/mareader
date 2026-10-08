//! The shared half of a highlight walk: what marks, and what is owed.

use crate::cache::LevelCache;
use crate::text::{MAX_WORD_CHARS, Span, hyphen_parts, lookup_candidates, sentence_around};

/// One word a painter measures: its span, text and sentence window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedWord {
    pub start: usize,
    pub end: usize,
    pub word: String,
    pub context: String,
}

/// One walk's answer.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Walk {
    /// The tokens above the reader's band, in reading order, capped.
    pub paint: Vec<PlannedWord>,
    /// The dataset keys the cache has never been asked, deduplicated.
    pub ask: Vec<String>,
}

/// The words that already mark, and the keys to ask for. Pure.
pub fn walk(
    text: &str,
    chars: &[char],
    tokens: &[Span],
    cache: &LevelCache,
    threshold: u8,
    cap: usize,
) -> Walk {
    let mut known: Vec<(Span, Vec<String>)> = Vec::with_capacity(tokens.len());
    for token in tokens {
        // Span length is a character count, the dataset's own unit.
        if token.len() > MAX_WORD_CHARS || token.end > chars.len() {
            continue;
        }
        let word: String = chars[token.start..token.end].iter().collect();
        known.push((*token, keys_of(&word)));
    }
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

/// The keys one token is probed as, most specific first.
pub fn keys_of(word: &str) -> Vec<String> {
    let mut keys = lookup_candidates(word);
    keys.extend(hyphen_parts(word));
    keys
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::tokenize;

    /// The walk of `text` against a cache holding `bands`.
    fn walked(text: &str, bands: &[(&str, u8)], threshold: u8) -> Walk {
        let mut cache = LevelCache::default();
        for (word, band) in bands {
            cache.insert(word, *band);
        }
        let tokens = tokenize(text);
        let chars: Vec<char> = text.chars().collect();
        walk(text, &chars, &tokens, &cache, threshold, 200)
    }

    #[test]
    fn only_words_strictly_above_the_band_are_planned() {
        let walk = walked("run ephemeral", &[("run", 2), ("ephemeral", 6)], 4);
        assert_eq!(walk.paint.len(), 1);
        assert_eq!(walk.paint[0].word, "ephemeral");
        // Both were already answered, so nothing is owed.
        assert!(walk.ask.is_empty());
    }

    #[test]
    fn unanswered_words_are_asked_once_each() {
        let walk = walked("ephemeral ephemeral palimpsest", &[], 4);
        assert_eq!(
            walk.ask,
            vec!["ephemeral".to_string(), "palimpsest".to_string()]
        );
        // Nothing is decidable yet, so nothing is planned.
        assert!(walk.paint.is_empty());
    }

    #[test]
    fn a_contraction_asks_its_base_and_a_hyphen_asks_its_parts() {
        let walk = walked("don't well-known", &[], 4);
        assert!(walk.ask.iter().any(|k| k == "not"));
        assert!(walk.ask.iter().any(|k| k == "well"));
        assert!(walk.ask.iter().any(|k| k == "known"));
    }

    #[test]
    fn an_answered_contraction_base_marks_the_token() {
        // The dataset stores no apostrophes: `don't` marks through `not`.
        let walk = walked("don't stop", &[("not", 6), ("stop", 1)], 4);
        assert_eq!(walk.paint.len(), 1);
        assert_eq!(walk.paint[0].word, "don't");
    }

    #[test]
    fn the_cap_bounds_the_plan() {
        let text = "alpha beta gamma delta";
        let bands = [("alpha", 6), ("beta", 6), ("gamma", 6), ("delta", 6)];
        let mut cache = LevelCache::default();
        for (word, band) in bands {
            cache.insert(word, band);
        }
        let tokens = tokenize(text);
        let chars: Vec<char> = text.chars().collect();
        let walk = walk(text, &chars, &tokens, &cache, 4, 2);
        assert_eq!(walk.paint.len(), 2);
        assert_eq!(walk.paint[0].word, "alpha");
        assert_eq!(walk.paint[1].word, "beta");
    }

    #[test]
    fn a_planned_word_carries_its_own_sentence_window() {
        let walk = walked("the ephemeral thing", &[("ephemeral", 6)], 4);
        assert_eq!(walk.paint[0].context, "the ephemeral thing");
        assert_eq!((walk.paint[0].start, walk.paint[0].end), (4, 13));
    }

    #[test]
    fn a_token_longer_than_the_dataset_is_never_asked() {
        let long = "x".repeat(MAX_WORD_CHARS + 1);
        let walk = walked(&long, &[], 4);
        assert!(walk.ask.is_empty());
        assert!(walk.paint.is_empty());
    }

    #[test]
    fn one_token_yields_its_keys_in_probe_order() {
        assert_eq!(keys_of("palimpsest"), vec!["palimpsest".to_string()]);
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
