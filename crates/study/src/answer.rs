//! An explained word: part of speech, meaning, synonyms and usages.

/// Bounds on one answer, applied once as it arrives.
const MAX_MEANING_CHARS: usize = 1_200;
const MAX_SYNONYMS: usize = 16;
const MAX_SYNONYM_CHARS: usize = 80;
const MAX_USAGES: usize = 8;
const MAX_USAGE_CHARS: usize = 400;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct WordInfo {
    /// Part of speech: "noun", "verb".
    pub pos: String,
    /// The word's common meaning for its class.
    pub meaning: String,
    pub synonyms: Vec<String>,
    /// Example sentences.
    pub usages: Vec<String>,
    /// The word only matters in its book, as a name does.
    pub book_only: bool,
}

impl WordInfo {
    /// The answer within the bounds above; cut text ends in "…".
    pub fn clamped(self) -> Self {
        let many = |v: Vec<String>, n: usize, max: usize| -> Vec<String> {
            v.into_iter().take(n).map(|s| clip(s, max)).collect()
        };
        Self {
            pos: clip(self.pos, MAX_SYNONYM_CHARS),
            meaning: clip(self.meaning, MAX_MEANING_CHARS),
            synonyms: many(self.synonyms, MAX_SYNONYMS, MAX_SYNONYM_CHARS),
            usages: many(self.usages, MAX_USAGES, MAX_USAGE_CHARS),
            book_only: self.book_only,
        }
    }
}

/// Whether `sentence` shows one of `words` in some form: one of its words
/// begins with a word's stem. Words not in Latin letters always pass, as
/// their forms can't be told apart this way.
pub fn shows_word(sentence: &str, words: &[&str]) -> bool {
    let Some(stems) = words.iter().copied().map(stem).collect::<Option<Vec<_>>>() else {
        return true;
    };
    let lower = sentence.to_lowercase();
    let mut said = lower.split(|c: char| !c.is_alphabetic());
    stems.is_empty() || said.any(|w| stems.iter().any(|s| w.starts_with(s.as_str())))
}

/// Endings dropped before a stem is cut, so "wandered" finds "wanders".
const ENDINGS: [&str; 4] = ["ing", "ed", "es", "s"];

/// What every form of `word` begins with: its longest run of letters,
/// lowercased, less one ending and then its last letter, keeping three;
/// none when its letters aren't Latin.
fn stem(word: &str) -> Option<String> {
    let lower = word.to_lowercase();
    let runs = lower.split(|c: char| !c.is_alphabetic());
    let longest = runs.max_by_key(|w| w.chars().count())?;
    // Basic Latin through Latin Extended-B.
    if longest.is_empty() || longest.chars().any(|c| u32::from(c) >= 0x250) {
        return None;
    }
    let cut = |e: &str| longest.strip_suffix(e).filter(|b| b.chars().count() >= 3);
    let bare = ENDINGS.into_iter().find_map(cut).unwrap_or(longest);
    let n = bare.chars().count();
    Some(bare.chars().take(if n > 3 { n - 1 } else { n }).collect())
}

/// `synonyms` less repeats, and any that only say the meaning or one of
/// `words` again.
pub fn fresh_synonyms(synonyms: Vec<String>, meaning: &str, words: &[&str]) -> Vec<String> {
    let plain = |s: &str| s.trim().trim_end_matches(['.', ',', ';']).to_lowercase();
    let said = std::iter::once(meaning).chain(words.iter().copied());
    let mut seen: Vec<String> = said.map(plain).collect();
    let fresh = |s: &String| {
        let p = plain(s);
        let new = !p.is_empty() && !seen.contains(&p);
        seen.push(p);
        new
    };
    synonyms.into_iter().filter(fresh).collect()
}

/// `s` cut to `max` characters, the cut marked.
fn clip(mut s: String, max: usize) -> String {
    if let Some((byte, _)) = s.char_indices().nth(max) {
        s.truncate(byte);
        s.push('…');
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_example_must_show_the_word_in_some_form() {
        let wherefore = ["wherefore"];
        let why = shows_word("Why did he leave?", &wherefore);
        assert!(!why, "a stand-in");
        assert!(shows_word("Wherefore art thou Romeo?", &wherefore));
        let wanders = shows_word("She wanders off.", &["wandered"]);
        assert!(wanders, "another form");
        assert!(shows_word("He is studying.", &["studies"]));
        let ran = shows_word("They ran home.", &["run", "ran"]);
        assert!(ran, "the form or its lemma");
        assert!(!shows_word("They sprinted home.", &["run"]));
        let japanese = shows_word("とても速く走る。", &["速い"]);
        assert!(japanese, "other scripts pass");
    }

    #[test]
    fn synonyms_that_repeat_the_meaning_or_the_word_go() {
        let said = ["why", "For what reason", "why", "wherefore.", " "];
        let said = said.map(String::from).to_vec();
        let kept = fresh_synonyms(said, "why", &["wherefore"]);
        assert_eq!(kept, ["For what reason"]);
    }

    #[test]
    fn long_answers_are_cut_and_marked() {
        let info = WordInfo {
            pos: "noun".into(),
            meaning: "é".repeat(MAX_MEANING_CHARS + 5),
            synonyms: vec!["s".repeat(90); 20],
            usages: vec!["fine".into()],
            book_only: true,
        }
        .clamped();
        assert_eq!(info.meaning.chars().count(), MAX_MEANING_CHARS + 1);
        assert!(info.meaning.ends_with('…'));
        assert_eq!(info.synonyms.len(), MAX_SYNONYMS);
        assert!(info.synonyms.iter().all(|s| s.chars().count() == 81));
        assert_eq!(info.usages, ["fine"]);
        assert_eq!(info.pos, "noun");
        assert!(info.book_only, "the flag survives");
    }
}
