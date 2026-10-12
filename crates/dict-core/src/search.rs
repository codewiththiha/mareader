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
    if word.is_ascii() && !word.bytes().any(|b| b.is_ascii_uppercase()) {
        return word.to_string();
    }
    let mut out = String::with_capacity(word.len());
    for c in word.chars() {
        let mapped = match c {
            '\u{2019}' => '\'',
            other => other,
        };
        for lower in mapped.to_lowercase() {
            out.push(lower);
        }
    }
    out
}

/// Stack capacity before `edit_distance` falls back to a heap row.
const STACK_CAP: usize = 64;

/// Levenshtein distance, giving up past `cap` (the answer is `cap + 1`).
pub fn edit_distance(a: &str, b: &str, cap: usize) -> usize {
    if a == b {
        return 0;
    }
    if a.is_ascii() && b.is_ascii() {
        return edit_distance_slice(a.as_bytes(), b.as_bytes(), cap);
    }
    let mut a_buf = ['\0'; STACK_CAP];
    let mut b_buf = ['\0'; STACK_CAP];
    if let (Some(a_chars), Some(b_chars)) = (fill_chars(a, &mut a_buf), fill_chars(b, &mut b_buf)) {
        return edit_distance_slice(a_chars, b_chars, cap);
    }
    let a_vec: Vec<char> = a.chars().collect();
    let b_vec: Vec<char> = b.chars().collect();
    edit_distance_slice(&a_vec, &b_vec, cap)
}

fn fill_chars<'a>(s: &str, buf: &'a mut [char; STACK_CAP]) -> Option<&'a [char]> {
    let mut len = 0;
    for ch in s.chars() {
        if len >= STACK_CAP {
            return None;
        }
        buf[len] = ch;
        len += 1;
    }
    Some(&buf[..len])
}

fn edit_distance_slice<T: Copy + Eq>(a: &[T], b: &[T], cap: usize) -> usize {
    if a.len().abs_diff(b.len()) > cap {
        return cap + 1;
    }
    if b.len() < STACK_CAP {
        let mut prev = [0usize; STACK_CAP];
        let mut cur = [0usize; STACK_CAP];
        for (j, slot) in prev[..=b.len()].iter_mut().enumerate() {
            *slot = j;
        }
        return run_rows(a, b, cap, &mut prev[..=b.len()], &mut cur[..=b.len()]);
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    run_rows(a, b, cap, &mut prev, &mut cur)
}

fn run_rows<T: Copy + Eq>(
    a: &[T],
    b: &[T],
    cap: usize,
    prev: &mut [usize],
    cur: &mut [usize],
) -> usize {
    for (i, &ca) in a.iter().enumerate() {
        cur[0] = i + 1;
        let mut row_min = cur[0];
        for (j, &cb) in b.iter().enumerate() {
            let cost = usize::from(ca != cb);
            let cell = (prev[j] + cost).min(prev[j + 1] + 1).min(cur[j] + 1);
            cur[j + 1] = cell;
            row_min = row_min.min(cell);
        }
        prev.copy_from_slice(cur);
        if row_min > cap {
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
        // A far ask finds no fit past the two-edit forgive.
        assert_eq!(classify("zz", "supercalifragilistic"), None);
        assert_eq!(
            classify("a", "supercalifragilistic"),
            Some(WordMatch::Substring)
        );
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
