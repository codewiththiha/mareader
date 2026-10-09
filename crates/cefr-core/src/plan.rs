//! What one row's tokens decide: what to paint, what the dataset owes.

use crate::cache::LevelCache;
use crate::text::{
    MAX_WORD_CHARS, Span, hyphen_parts, lookup_candidates, sentence_around, tokenize,
};

/// One row's decision, from its tokens and what the cache already answers.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plan {
    /// The tokens whose band is known to be strictly above the threshold.
    pub hard: Vec<Span>,
    /// The dataset keys the cache cannot answer yet, deduplicated in order.
    pub misses: Vec<String>,
}

/// One hard word, resolved to what a painter and a click both need.
#[derive(Debug, Clone, PartialEq)]
pub struct Planned {
    pub start: usize,
    pub end: usize,
    pub word: String,
    /// The ±60-character window the word was decided in.
    pub context: String,
}

/// Plan `text`'s own tokenization.
pub fn plan(text: &str, cache: &LevelCache, threshold: u8, cap: usize) -> Plan {
    plan_tokens(text, &tokenize(text), cache, threshold, cap)
}

/// Plan tokens a caller already holds, so one row is tokenized once.
pub fn plan_tokens(
    text: &str,
    tokens: &[Span],
    cache: &LevelCache,
    threshold: u8,
    cap: usize,
) -> Plan {
    let chars: Vec<char> = text.chars().collect();
    let mut hard: Vec<Span> = Vec::new();
    let mut misses: Vec<String> = Vec::new();
    for token in tokens {
        // A token over the cap is not dataset material; asking wastes a round.
        if token.len() > MAX_WORD_CHARS {
            continue;
        }
        let Some(word) = slice(&chars, *token) else {
            continue;
        };
        let mut candidates = lookup_candidates(&word);
        candidates.extend(hyphen_parts(&word));
        for key in &candidates {
            if cache.get(key).is_none() && !misses.iter().any(|held| held == key) {
                misses.push(key.clone());
            }
        }
        if hard.len() < cap && cache.any_above(&candidates, threshold) {
            hard.push(*token);
        }
    }
    Plan { hard, misses }
}

/// Resolve spans to words, each with the sentence it was decided in.
pub fn planned(text: &str, spans: &[Span]) -> Vec<Planned> {
    let chars: Vec<char> = text.chars().collect();
    spans
        .iter()
        .filter_map(|span| {
            Some(Planned {
                start: span.start,
                end: span.end,
                word: slice(&chars, *span)?,
                context: sentence_around(text, span.start, span.end),
            })
        })
        .collect()
}

/// The characters a span names, or `None` when it runs past the text.
fn slice(chars: &[char], span: Span) -> Option<String> {
    (span.end <= chars.len()).then(|| chars[span.start..span.end].iter().collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::LevelCache;

    fn cache_of(entries: &[(&str, u8)]) -> LevelCache {
        let mut cache = LevelCache::default();
        for (key, band) in entries {
            cache.insert(key, *band);
        }
        cache
    }

    #[test]
    fn only_a_band_strictly_above_the_threshold_is_hard() {
        let text = "the run ephemeral";
        let cache = cache_of(&[("the", 1), ("run", 4), ("ephemeral", 6)]);
        let plan = plan(text, &cache, 4, 100);
        assert_eq!(plan.hard.len(), 1);
        assert_eq!(planned(text, &plan.hard)[0].word, "ephemeral");
        // Everything is answered, so nothing is owed.
        assert!(plan.misses.is_empty());
    }

    #[test]
    fn an_unanswered_word_is_owed_and_not_yet_hard() {
        let plan = plan("palimpsest", &LevelCache::default(), 4, 100);
        assert!(plan.hard.is_empty());
        assert_eq!(plan.misses, vec!["palimpsest".to_string()]);
    }

    #[test]
    fn a_contraction_is_owed_under_its_own_base() {
        let owed = plan("don't", &LevelCache::default(), 4, 100);
        assert_eq!(owed.misses, vec!["don't".to_string(), "not".to_string()]);
        // The base alone decides it: answering `not` marks the token.
        let answered = cache_of(&[("don't", 0), ("not", 5)]);
        assert_eq!(plan("don't", &answered, 4, 100).hard.len(), 1);
    }

    #[test]
    fn a_compound_is_owed_whole_and_in_its_parts() {
        let plan = plan("well-known", &LevelCache::default(), 4, 100);
        assert_eq!(
            plan.misses,
            vec![
                "well-known".to_string(),
                "well".to_string(),
                "known".to_string()
            ]
        );
    }

    #[test]
    fn a_repeated_key_is_owed_once() {
        let plan = plan("ephemeral ephemeral", &LevelCache::default(), 4, 100);
        assert_eq!(plan.misses, vec!["ephemeral".to_string()]);
    }

    #[test]
    fn the_cap_stops_the_painting_not_the_asking() {
        let text = "a b c d e f g h";
        let cache = cache_of(&[
            ("a", 6),
            ("b", 6),
            ("c", 6),
            ("d", 6),
            ("e", 6),
            ("f", 6),
            ("g", 6),
            ("h", 6),
        ]);
        let plan = plan(text, &cache, 4, 3);
        assert_eq!(plan.hard.len(), 3);
        // The words past the cap are still answered, so nothing is owed.
        assert!(plan.misses.is_empty());
    }

    #[test]
    fn a_token_past_the_dataset_length_is_neither_asked_nor_painted() {
        let long = "x".repeat(MAX_WORD_CHARS + 1);
        let plan = plan(&long, &LevelCache::default(), 4, 100);
        assert!(plan.hard.is_empty());
        assert!(plan.misses.is_empty());
    }

    #[test]
    fn planning_held_tokens_agrees_with_planning_the_text() {
        let text = "the ephemeral run";
        let tokens = tokenize(text);
        let cache = cache_of(&[("the", 1), ("ephemeral", 6), ("run", 2)]);
        assert_eq!(
            plan_tokens(text, &tokens, &cache, 4, 100),
            plan(text, &cache, 4, 100)
        );
    }

    #[test]
    fn a_planned_word_carries_the_sentence_it_was_decided_in() {
        let text = "Some prose before ephemeral and some after it.";
        let cache = cache_of(&[("ephemeral", 6)]);
        let plan = plan(text, &cache, 4, 100);
        let words = planned(text, &plan.hard);
        assert_eq!(words.len(), 1);
        assert_eq!(words[0].word, "ephemeral");
        assert!(words[0].context.contains("Some prose before"));
        assert!(words[0].context.contains("and some after it."));
        assert_eq!(
            &text
                .chars()
                .skip(words[0].start)
                .take(words[0].end - words[0].start)
                .collect::<String>(),
            "ephemeral"
        );
    }

    #[test]
    fn a_span_past_the_text_plans_nothing_instead_of_panicking() {
        assert!(slice(&['a'; 2], Span::new(0, 9)).is_none());
        assert!(planned("ab", &[Span::new(0, 9)]).is_empty());
    }
}
