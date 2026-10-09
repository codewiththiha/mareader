//! Words in a page or a block: spans, normal forms, lookup candidates.

/// A `[start, end)` span in CHARACTERS, the unit every spot counts in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Self {
        Self {
            start,
            end: end.max(start),
        }
    }

    pub fn len(&self) -> usize {
        self.end - self.start
    }

    pub fn is_empty(&self) -> bool {
        self.start == self.end
    }
}

/// Past this a token is not a word: the dataset tops out at 25.
pub const MAX_WORD_CHARS: usize = 32;

/// The apostrophes a token may carry, folded to the ASCII one.
const APOSTROPHES: [char; 2] = ['\'', '\u{2019}'];

/// The word a token's text is probed as: lowercased, ASCII apostrophe.
pub fn normalize(word: &str) -> String {
    word.chars()
        .map(|c| if APOSTROPHES.contains(&c) { '\'' } else { c })
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// The word spans of `text`: ASCII letter runs; one joiner inside.
pub fn tokenize(text: &str) -> Vec<Span> {
    let chars: Vec<char> = text.chars().collect();
    let n = chars.len();
    let mut spans = Vec::new();
    let mut i = 0;
    while i < n {
        if !chars[i].is_ascii_alphabetic() {
            i += 1;
            continue;
        }
        let start = i;
        let mut j = start + 1;
        while j < n {
            let c = chars[j];
            if c.is_ascii_alphabetic() {
                j += 1;
                continue;
            }
            // A connector stays inside the run only with a letter after it.
            let joins = matches!(c, '\'' | '\u{2019}' | '-')
                && j + 1 < n
                && chars[j + 1].is_ascii_alphabetic();
            if !joins {
                break;
            }
            j += 1;
        }
        spans.push(Span::new(start, j));
        i = j;
    }
    spans
}

/// Whether a raw word could be answerable: ASCII, letters inside,
/// dataset length.
pub fn is_english_ascii(word: &str) -> bool {
    !word.is_empty()
        && word.chars().count() <= MAX_WORD_CHARS
        && word
            .chars()
            .all(|c| c.is_ascii_alphabetic() || matches!(c, '\'' | '\u{2019}' | '-'))
        && word.chars().any(|c| c.is_ascii_alphabetic())
}

/// The expanded form of a contraction's ending (`don't` -> `not`).
pub fn contraction_base(word: &str) -> Option<&'static str> {
    let lower = normalize(word);
    cefr::tags::ABBREVIATION_MAPPING
        .iter()
        .find(|(suffix, _)| lower.ends_with(suffix))
        .map(|(_, base)| *base)
}

/// The keys a token answers as, most specific first: itself, then its
/// contraction base.
pub fn lookup_candidates(word: &str) -> Vec<String> {
    let normalized = normalize(word);
    let mut out = vec![normalized];
    if let Some(base) = contraction_base(word) {
        out.push(base.to_string());
    }
    out
}

/// The parts of a hyphenated token; empty when it has no hyphen.
pub fn hyphen_parts(word: &str) -> Vec<String> {
    let normalized = normalize(word);
    if !normalized.contains('-') {
        return Vec::new();
    }
    normalized
        .split('-')
        .filter(|p| !p.is_empty() && p.chars().any(|c| c.is_ascii_alphabetic()))
        .map(String::from)
        .collect()
}

/// The ±60-character window around `[start, end)`: the model's context.
pub fn sentence_around(text: &str, start: usize, end: usize) -> String {
    let total = text.chars().count();
    let (start, end) = (start.min(total), end.clamp(start, total));
    let from = start.saturating_sub(60);
    let to = (end + 60).min(total);
    let window: String = text.chars().skip(from).take(to - from).collect();
    window.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(text: &str) -> Vec<String> {
        tokenize(text)
            .into_iter()
            .map(|s| text.chars().skip(s.start).take(s.len()).collect::<String>())
            .collect()
    }

    #[test]
    fn plain_prose_tokenizes_in_order() {
        assert_eq!(
            words("The palimpsest was scraped clean."),
            vec!["The", "palimpsest", "was", "scraped", "clean"]
        );
    }

    #[test]
    fn apostrophes_stay_inside_a_word() {
        assert_eq!(words("don't stop"), vec!["don't", "stop"]);
        assert_eq!(words("it’s fine"), vec!["it’s", "fine"]);
        // A quote that OPENS is not a connector.
        assert_eq!(words("'tis said"), vec!["tis", "said"]);
        assert_eq!(words("the 'quoted' word"), vec!["the", "quoted", "word"]);
    }

    #[test]
    fn hyphens_join_a_real_word_but_not_a_dash_pair() {
        assert_eq!(words("a well-known fact"), vec!["a", "well-known", "fact"]);
        assert_eq!(words("mother-in-law"), vec!["mother-in-law"]);
        assert_eq!(words("a -- b"), vec!["a", "b"]);
        assert_eq!(words("state-"), vec!["state"]);
        assert_eq!(words("-start"), vec!["start"]);
    }

    #[test]
    fn non_ascii_text_stops_the_run() {
        // An accented letter is not dataset material; no fragment is kept.
        assert_eq!(words("café owner"), vec!["caf", "owner"]);
        assert_eq!(words("中文 only"), vec!["only"]);
        assert!(tokenize("中文").is_empty());
    }

    #[test]
    fn digits_and_underscores_split_words() {
        assert_eq!(words("16 horses"), vec!["horses"]);
        assert_eq!(words("x86_chip"), vec!["x", "chip"]);
    }

    #[test]
    fn spans_count_characters_not_bytes_or_units() {
        // One emoji is one character, four UTF-16 units.
        let text = "\u{1F600} palimpsest";
        let spans = tokenize(text);
        assert_eq!(spans.len(), 1);
        let span = spans[0];
        let word: String = text.chars().skip(span.start).take(span.len()).collect();
        assert_eq!(word, "palimpsest");
    }

    #[test]
    fn the_length_cap_keeps_dataset_sized_words() {
        assert!(is_english_ascii("palimpsest"));
        assert!(is_english_ascii("don't"));
        assert!(is_english_ascii("well-known"));
        assert!(!is_english_ascii(""));
        assert!(!is_english_ascii("café"));
        assert!(!is_english_ascii("中文"));
        // Numbers-only strings are not words.
        assert!(!is_english_ascii("12345"));
        // The cap is the dataset's longest entry, with headroom.
        assert!(is_english_ascii("demethylchlortetracycline"));
        assert!(!is_english_ascii(&"x".repeat(MAX_WORD_CHARS + 1)));
    }

    #[test]
    fn candidates_expand_contractions_but_nothing_else() {
        assert_eq!(
            lookup_candidates("palimpsest"),
            vec!["palimpsest".to_string()]
        );
        assert_eq!(
            lookup_candidates("don't"),
            vec!["don't".to_string(), "not".to_string()]
        );
        assert_eq!(
            lookup_candidates("it’s"),
            vec!["it's".to_string(), "is".to_string()]
        );
        // A possessive answers as its contraction base, not an absent form.
        assert_eq!(
            lookup_candidates("dog's"),
            vec!["dog's".to_string(), "is".to_string()]
        );
    }

    #[test]
    fn hyphen_parts_split_for_the_dataset() {
        assert_eq!(
            hyphen_parts("well-known"),
            vec!["well".to_string(), "known".to_string()]
        );
        assert!(hyphen_parts("palimpsest").is_empty());
        // A stray edge dash yields no empty part.
        assert_eq!(hyphen_parts("-known-"), vec!["known".to_string()]);
    }

    #[test]
    fn the_context_window_is_the_selection_pills_reach() {
        let text = "a".repeat(200);
        assert_eq!(sentence_around(&text, 100, 104).chars().count(), 124);
        // Clamped at both ends, and a backwards span never inverts.
        assert_eq!(sentence_around("short", 0, 5), "short");
        assert_eq!(sentence_around("short", 3, 2), "short");
    }
}
