//! Answered words and the hard ones in a text.

use std::collections::HashMap;

use crate::text::{MAX_WORD_CHARS, Span, keys, normalize_chars, tokenize_chars};

/// More answers than this and the cache starts over.
pub const CACHE_CAP: usize = 8_192;

/// A dataset level as a band, 1 (A1) to 6 (C2); 0 is unknown.
pub fn band(level: Option<f64>) -> u8 {
    level.map_or(0, cefr::level::level_band)
}

/// Each answered key's band; a word the dataset lacks is 0.
#[derive(Clone, Debug, Default)]
pub struct Levels(HashMap<Box<str>, u8>);

impl Levels {
    pub fn get(&self, key: &str) -> Option<u8> {
        self.0.get(key).copied()
    }

    pub fn insert(&mut self, key: &str, band: u8) {
        if self.0.len() >= CACHE_CAP && !self.0.contains_key(key) {
            self.0.clear();
        }
        self.0.insert(key.into(), band.min(6));
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// A text's hard words and the keys still to ask about.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Plan {
    /// Words with a key above the level, at most `cap` of them.
    pub hard: Vec<Span>,
    /// The highest known band for each hard span, in the same order.
    pub bands: Vec<u8>,
    /// Unanswered keys, each once; the cap doesn't cut them.
    pub misses: Vec<String>,
}

/// Which words of `text` are above `level`, from what `levels` knows.
pub fn plan(text: &str, levels: &Levels, level: u8, cap: usize) -> Plan {
    plan_chars(&text.chars().collect::<Vec<char>>(), levels, level, cap)
}

/// As [`plan`], over characters a caller already holds.
pub fn plan_chars(chars: &[char], levels: &Levels, level: u8, cap: usize) -> Plan {
    let mut out = Plan::default();
    for span in tokenize_chars(chars) {
        if span.len() > MAX_WORD_CHARS {
            continue;
        }
        let Some(word) = chars.get(span.start..span.end) else {
            continue;
        };
        let misses = &mut out.misses;
        let highest = word_band(
            word,
            |k| levels.get(k),
            |key| {
                if !misses.contains(&key) {
                    misses.push(key);
                }
            },
        );
        if highest > level && out.hard.len() < cap {
            out.hard.push(span);
            out.bands.push(highest);
        }
    }
    out
}

/// A word's band as shading reads it: the highest band among its keys.
///
/// Each key `levels` can't answer goes to `missed`.
pub fn word_band(
    word: &[char],
    levels: impl Fn(&str) -> Option<u8>,
    mut missed: impl FnMut(String),
) -> u8 {
    let mut highest = 0;
    for key in keys(normalize_chars(word.iter().copied())) {
        match levels(&key) {
            Some(b) => highest = highest.max(b),
            None => missed(key),
        }
    }
    highest
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn levels(of: &[(&str, u8)]) -> Levels {
        let mut l = Levels::default();
        for (k, b) in of {
            l.insert(k, *b);
        }
        l
    }

    #[test]
    fn only_a_band_above_the_level_is_hard() {
        let known = levels(&[("the", 1), ("run", 4), ("ephemeral", 6)]);
        let p = plan("the run ephemeral", &known, 4, 100);
        assert_eq!(p.hard, [Span { start: 8, end: 17 }]);
        assert_eq!(p.bands, [6]);
        assert!(p.misses.is_empty());
    }

    #[test]
    fn unanswered_keys_are_asked_once_and_not_yet_hard() {
        let p = plan("ephemeral ephemeral well-known", &Levels::default(), 4, 100);
        assert!(p.hard.is_empty());
        assert_eq!(p.misses, ["ephemeral", "well-known", "well", "known"]);
    }

    #[test]
    fn a_hard_part_or_base_makes_the_word_hard() {
        let known = levels(&[("ill-starred", 0), ("ill", 2), ("starred", 6)]);
        assert_eq!(plan("ill-starred", &known, 4, 9).hard.len(), 1);
    }

    #[test]
    fn the_cap_stops_the_shading_not_the_asking() {
        let known = levels(&[("a", 6), ("b", 6), ("c", 6)]);
        let p = plan("a b c d", &known, 4, 2);
        assert_eq!(p.hard.len(), 2);
        assert_eq!(p.misses, ["d"]);
    }

    #[test]
    fn bands_are_whole_levels_and_unknown_is_zero() {
        assert_eq!(band(None), 0);
        assert_eq!(band(Some(f64::NAN)), 0);
        assert_eq!(band(Some(4.6)), 5);
        let mut full = Levels::default();
        for i in 0..CACHE_CAP {
            full.insert(&i.to_string(), 1);
        }
        full.insert("one more", 9);
        assert_eq!((full.len(), full.get("one more")), (1, Some(6)));
    }

    #[test]
    fn a_word_s_band_never_comes_from_a_shorter_word_inside_it() {
        let known = levels(&[("after", 1), ("aft", 6)]);
        let after: Vec<char> = "After".chars().collect();
        let mut missed = Vec::new();
        assert_eq!(word_band(&after, |k| known.get(k), |k| missed.push(k)), 1);
        assert!(missed.is_empty());
        assert!(plan("After", &known, 5, 9).hard.is_empty());
    }
}
