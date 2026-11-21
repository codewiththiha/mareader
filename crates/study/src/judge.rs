//! The StudyJudge verdict: what a reader's attempt at a word means.
//!
//! A canned, heuristic stand-in for the AI judge is here so tests and shots
//! can exercise every state without calling a model. The real task sends the
//! word, its sentence and the reader's attempt to the model and reads back
//! `{ verdict, feedback }`. See the plan, decision D6.

use serde::{Deserialize, Serialize};

/// The three verdicts: correct, close (right area, too vague), wrong.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Correct,
    Close,
    #[default]
    Wrong,
}

impl Verdict {
    pub const ALL: [Verdict; 3] = [Self::Correct, Self::Close, Self::Wrong];

    pub fn label(self) -> &'static str {
        match self {
            Self::Correct => "Correct",
            Self::Close => "Close",
            Self::Wrong => "Not quite",
        }
    }
}

impl std::fmt::Display for Verdict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

impl Verdict {
    /// A correct answer earns its word.
    pub fn is_correct(self) -> bool {
        matches!(self, Self::Correct)
    }
}

/// A verdict with the reason, in the language the reader answered in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Judged {
    pub verdict: Verdict,
    pub feedback: String,
}

impl Judged {
    pub fn correct(feedback: impl Into<String>) -> Self {
        Self {
            verdict: Verdict::Correct,
            feedback: feedback.into(),
        }
    }
    pub fn close(feedback: impl Into<String>) -> Self {
        Self {
            verdict: Verdict::Close,
            feedback: feedback.into(),
        }
    }
    pub fn wrong(feedback: impl Into<String>) -> Self {
        Self {
            verdict: Verdict::Wrong,
            feedback: feedback.into(),
        }
    }
}

/// What the judge compares the attempt against: the entry's own data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JudgedEntry {
    pub lemma: String,
    pub synonyms: Vec<String>,
    pub meaning: String,
}

/// A heuristic stand-in for the StudyJudge AI task: it scores an attempt
/// against the entry's synonyms and meaning. The real model judges meaning,
/// not spelling, in any language, and never returns the full meaning unless
/// the reader asks to reveal it.
pub fn judge(attempt: &str, entry: &JudgedEntry) -> Judged {
    let words = |text: &str| -> Vec<String> {
        let stop = [
            "a", "an", "the", "of", "to", "in", "on", "at", "is", "are", "was", "were", "be",
            "been", "it", "its", "that", "this", "with", "for", "and", "or", "but", "as", "by",
            "from", "not", "no", "into", "one", "some", "very", "more", "than", "who", "what",
            "which", "when", "where", "how", "can", "has", "have", "had", "you", "your", "his",
            "her", "their", "them", "they", "she", "he", "we", "our", "over",
        ];
        text.to_lowercase()
            .replace(|c: char| !c.is_alphanumeric() && c != '\'', " ")
            .split_whitespace()
            .filter(|w| w.len() > 2 && !stop.contains(w))
            .map(str::to_owned)
            .collect()
    };

    let stem = |w: &str| -> String {
        let len = w.len();
        let cut = len.saturating_sub(3).max(4);
        w.get(..cut).unwrap_or(w).to_owned()
    };
    let near = |a: &str, b: &str| -> bool {
        let sa = stem(a);
        let sb = stem(b);
        a == b
            || sa == sb
            || (a.len() > 4 && b.starts_with(sa.as_str()))
            || (b.len() > 4 && a.starts_with(sb.as_str()))
    };

    let mine = words(attempt);
    if mine.is_empty() {
        return Judged::wrong(
            "I couldn't read an answer there. Try a word or a short phrase, in any language.",
        );
    }
    // Typing the word itself is not an answer.
    if mine.len() == 1
        && let Some(first) = mine.first()
        && near(first, &entry.lemma)
    {
        return Judged::wrong(
            "That is the word itself. Say what it means: a synonym, a short explanation, or a translation."
                .to_string(),
        );
    }
    let strong: Vec<String> = entry
        .synonyms
        .iter()
        .flat_map(|s| words(s))
        .chain(words(&entry.meaning))
        .collect();
    let hit = mine.iter().find(|m| strong.iter().any(|s| near(m, s)));
    if let Some(hit) = hit {
        return Judged::correct(format!(
            "Yes. Here “{}” means: {} “{}” fits.",
            entry.lemma, entry.meaning, hit
        ));
    }
    let weak: Vec<String> = words(&entry.meaning);
    let hit = mine.iter().find(|m| weak.iter().any(|s| near(m, s)));
    if let Some(hit) = hit {
        return Judged::close(format!(
            "Close. “{}” points the right way, but be more exact. Here it means: {}",
            hit, entry.meaning
        ));
    }
    let shown = attempt
        .trim()
        .get(..60.min(attempt.len()))
        .unwrap_or(attempt.trim());
    Judged::wrong(format!(
        "Not quite. “{}” does not fit this sentence. Here it means: {}",
        shown, entry.meaning
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> JudgedEntry {
        JudgedEntry {
            lemma: "acknowledge".to_owned(),
            synonyms: vec![
                "recognized".to_owned(),
                "accepted".to_owned(),
                "admitted".to_owned(),
            ],
            meaning: "Accepted or admitted to be true.".to_owned(),
        }
    }

    #[test]
    fn a_synonym_is_correct() {
        let got = judge("recognized", &entry());
        assert_eq!(got.verdict, Verdict::Correct);
    }

    #[test]
    fn the_word_itself_is_wrong() {
        let got = judge("acknowledge", &entry());
        assert_eq!(got.verdict, Verdict::Wrong);
    }

    #[test]
    fn a_vague_related_word_is_close() {
        let got = judge("admission", &entry());
        assert_eq!(got.verdict, Verdict::Close);
    }

    #[test]
    fn unrelated_is_wrong() {
        let got = judge("apple pie", &entry());
        assert_eq!(got.verdict, Verdict::Wrong);
    }

    #[test]
    fn an_empty_attempt_is_wrong_with_a_helpful_message() {
        let got = judge("", &entry());
        assert_eq!(got.verdict, Verdict::Wrong);
        assert!(got.feedback.contains("couldn't read"));
    }

    #[test]
    fn meaning_is_never_given_unless_asked() {
        let got = judge("something unrelated", &entry());
        assert!(!got.feedback.contains("Accepted or admitted to be true"));
    }
}
