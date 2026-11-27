//! A batch's words as read: each form beside the sentence it stands in.
//!
//! A word is hard exactly when the book's shading marks it: its band is
//! [`word_band`]'s, never a shorter word's found by cutting an ending.

use std::collections::HashMap;

use mareader_cefr::{MAX_WORD_CHARS, candidates, normalize, tokenize, word_band};
use mareader_study::mode::{bases, lemma, proper};

/// A sentence shown on a card is cut to this many characters.
const SENTENCE_CHARS: usize = 280;

/// One word where it stands on a page.
#[derive(Clone, Debug, PartialEq)]
pub struct Read {
    /// As printed.
    pub form: String,
    /// Lower case, as the levels are keyed.
    pub norm: String,
    pub sentence: String,
    /// Character span in the extracted page and stable book location.
    pub chars: std::ops::Range<usize>,
    pub spot: Option<mareader_study::mode::Location>,
}

impl Read {
    /// Every key a level may be filed under: its own, its parts, its bases.
    pub fn keys(&self) -> Vec<String> {
        let mut out = candidates(&self.form);
        for base in bases(&self.norm) {
            if !out.contains(&base) {
                out.push(base);
            }
        }
        out
    }

    /// Its band as the book's shading reads it, from the bands answered.
    pub fn band(&self, bands: &HashMap<String, u8>) -> u8 {
        let chars: Vec<char> = self.form.chars().collect();
        word_band(&chars, |k| bands.get(k).copied(), |_| {})
    }

    /// Its card's word: a base of the same `band`, else itself as read.
    ///
    /// A base at another band is another word: "aft" is not "after".
    pub fn lemma(&self, bands: &HashMap<String, u8>, band: u8) -> String {
        lemma(&self.norm, |base: &str| bands.get(base) == Some(&band))
    }
}

/// The words of `text` in reading order; a name inside a sentence is left out.
pub fn words(text: &str) -> Vec<Read> {
    let chars: Vec<char> = text.chars().collect();
    let stops = stops(&chars);
    let mut out = Vec::new();
    let mut opened = None;
    for span in tokenize(text) {
        if span.len() > MAX_WORD_CHARS {
            continue;
        }
        let Some(form) = chars.get(span.start..span.end) else {
            continue;
        };
        let form: String = form.iter().collect();
        let (from, to) = sentence(&stops, span.start, chars.len());
        let opens = opened != Some(from);
        opened = Some(from);
        if proper(&form, opens) {
            continue;
        }
        out.push(Read {
            norm: normalize(&form),
            chars: span.start..span.end,
            spot: None,
            sentence: clip(&chars, (from, to), span.start),
            form,
        });
    }
    out
}

/// The sentence of `text` holding character `at`, cut as a card's is.
pub fn sentence_of(text: &str, at: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    let found = sentence(&stops(&chars), at, chars.len());
    clip(&chars, found, at)
}

/// Where each sentence starts: past a stop and the space after it.
fn stops(chars: &[char]) -> Vec<usize> {
    let mut out = vec![0];
    for (i, pair) in chars.windows(2).enumerate() {
        if let [stop, space] = pair
            && matches!(*stop, '.' | '!' | '?' | '…')
            && space.is_whitespace()
        {
            out.push(i.saturating_add(2));
        }
    }
    out
}

/// The sentence holding character `at`, as a start and an end.
fn sentence(stops: &[usize], at: usize, len: usize) -> (usize, usize) {
    let next = stops.partition_point(|&s| s <= at);
    let from = next.checked_sub(1).and_then(|i| stops.get(i)).copied();
    let to = stops.get(next).copied().unwrap_or(len);
    (from.unwrap_or(0), to.min(len))
}

/// The sentence's text, cut around `at` when it runs long.
fn clip(chars: &[char], (from, to): (usize, usize), at: usize) -> String {
    let from = from.max(at.saturating_sub(SENTENCE_CHARS / 2));
    let to = to.min(from.saturating_add(SENTENCE_CHARS));
    let text: String = chars.get(from..to).unwrap_or_default().iter().collect();
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn each_word_keeps_its_own_sentence() {
        let got = words("The cat sat. Dogs ran far!  Birds flew");
        let of = |form: &str| {
            let found = got.iter().find(|r| r.form == form).unwrap();
            found.sentence.clone()
        };
        assert_eq!(of("cat"), "The cat sat.");
        assert_eq!(of("ran"), "Dogs ran far!");
        assert_eq!(of("flew"), "Birds flew");
        let second = sentence_of("The cat sat. Dogs ran far!", 14);
        assert_eq!(second, "Dogs ran far!");
        let wrapped = sentence_of("Boats \n unloaded rice.", 9);
        assert_eq!(wrapped, "Boats unloaded rice.");
    }

    #[test]
    fn a_name_inside_a_sentence_is_left_out_but_an_opening_word_is_not() {
        let got = words("Rivers meet where Alice waits.");
        let forms: Vec<&str> = got.iter().map(|r| r.form.as_str()).collect();
        assert_eq!(forms, ["Rivers", "meet", "where", "waits"]);
        assert_eq!(got[0].norm, "rivers");
    }

    #[test]
    fn a_long_sentence_is_cut_around_its_word() {
        let long = format!("{} needle {}", "a ".repeat(400), "b ".repeat(400));
        let got = words(&long);
        let needle = got.iter().find(|r| r.form == "needle").unwrap();
        assert!(needle.sentence.contains("needle"));
        assert!(needle.sentence.chars().count() <= SENTENCE_CHARS);
    }

    #[test]
    fn keys_hold_the_form_and_its_bases_once_each() {
        let read = &words("Walking")[0];
        let keys = read.keys();
        assert_eq!(keys.first().map(String::as_str), Some("walking"));
        assert!(keys.iter().any(|k| k == "walk"));
        let unique: std::collections::HashSet<&String> = keys.iter().collect();
        assert_eq!(unique.len(), keys.len());
        assert!(words("").is_empty());
    }

    fn bands(of: &[(&str, u8)]) -> HashMap<String, u8> {
        of.iter().map(|(k, b)| ((*k).to_owned(), *b)).collect()
    }

    #[test]
    fn a_word_keeps_its_own_band_and_folds_only_to_a_base_as_hard() {
        let known = bands(&[("after", 1), ("aft", 6), ("badger", 6), ("badge", 4)]);
        let read = &words("After badgers")[..];
        assert_eq!(read[0].band(&known), 1, "after is not aft");
        assert_eq!(read[0].lemma(&known, 1), "after");
        let known = bands(&[("badgers", 6), ("badger", 6), ("badge", 4)]);
        assert_eq!(read[1].band(&known), 6);
        assert_eq!(read[1].lemma(&known, 6), "badger");
        let unlisted = bands(&[("badger", 6)]);
        assert_eq!(read[1].band(&unlisted), 0, "unshaded, so no card");
    }
}
