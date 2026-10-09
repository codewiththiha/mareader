//! How tight a word fit the ask, and the near-miss.

use crate::entry::WordMatch;

/// How `word` answered `ask`, best fit first. Case-blind.
pub fn classify(ask: &str, word: &str) -> Option<WordMatch> {
    let ask = fold(ask);
    let word = fold(word);
    if ask.is_empty() || word.is_empty() {
        return None;
    }
    if word == ask {
        return Some(WordMatch::Exact);
    }
    if word.starts_with(&ask) {
        return Some(WordMatch::Prefix);
    }
    if word.contains(&ask) {
        return Some(WordMatch::Substring);
    }
    (edit_distance(&ask, &word, 2) <= 2).then_some(WordMatch::Fuzzy)
}

/// Case-folded, apostrophe-unified compare form.
pub fn fold(word: &str) -> String {
    word.chars()
        .map(|c| match c {
            '\u{2019}' => '\'',
            other => other,
        })
        .flat_map(char::to_lowercase)
        .collect()
}

/// Levenshtein distance, giving up past `cap` (the answer is `cap + 1`).
pub fn edit_distance(a: &str, b: &str, cap: usize) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    if a.len().abs_diff(b.len()) > cap {
        return cap + 1;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            cur[j + 1] = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
        if *prev.iter().min().unwrap_or(&0) > cap {
            return cap + 1;
        }
    }
    prev[b.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tightest_fit_wins() {
        assert_eq!(classify("run", "run"), Some(WordMatch::Exact));
        assert_eq!(classify("Run", "run"), Some(WordMatch::Exact));
        assert_eq!(classify("run", "runner"), Some(WordMatch::Prefix));
        assert_eq!(classify("run", "sunrunner"), Some(WordMatch::Substring));
        assert_eq!(classify("colour", "color"), Some(WordMatch::Fuzzy));
        assert_eq!(classify("a", "supercalifragilistic"), None);
    }

    #[test]
    fn apostrophes_compare_as_one() {
        assert_eq!(fold("it\u{2019}s"), "it's");
        assert_eq!(classify("it's", "it\u{2019}s"), Some(WordMatch::Exact));
    }

    #[test]
    fn distance_gives_up_past_the_cap() {
        assert_eq!(edit_distance("run", "run", 2), 0);
        assert_eq!(edit_distance("run", "ran", 2), 1);
        assert_eq!(edit_distance("kitten", "sitting", 2), 3);
        assert_eq!(edit_distance("a", "abcdef", 2), 3);
    }
}
