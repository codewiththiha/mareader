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
