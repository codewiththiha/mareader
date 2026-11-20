//! Judging a recall attempt: three verdicts and why, in the answer's language.
//!
//! The real path sends word, sentence and attempt to the StudyJudge AI task.
//! This heuristic stands in for tests, shots and the offline demo, so every
//! state is real while verdicts stay approximate.

/// How an attempt landed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Correct,
    Close,
    Wrong,
}

/// A verdict plus one or two sentences of feedback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Judged {
    pub verdict: Verdict,
    pub feedback: String,
}

/// The entry a recall attempt is judged against.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecallTarget {
    pub lemma: String,
    /// Contextual meaning the card would show.
    pub meaning: String,
    pub synonyms: Vec<String>,
    /// Offline pack gloss, when the pack has one.
    pub gloss: String,
}

const STOP: &[&str] = &[
    "a", "an", "the", "of", "to", "in", "on", "at", "is", "are", "was", "were", "be", "been", "it",
    "its", "that", "this", "with", "for", "and", "or", "but", "as", "by", "from", "not", "no",
    "into", "one", "some", "very", "more", "than", "who", "what", "which", "when", "where", "how",
    "can", "has", "have", "had", "you", "your", "his", "her", "their", "them", "she", "he", "we",
    "our", "over",
];

/// Meaningful answer words: long enough, not stopwords.
fn words(s: &str) -> Vec<String> {
    s.chars()
        .map(|c| if c.is_alphabetic() { c } else { ' ' })
        .collect::<String>()
        .to_lowercase()
        .split_whitespace()
        .filter(|w| w.chars().count() > 2 && !STOP.contains(w))
        .map(str::to_owned)
        .collect()
}

/// A crude stem: drop the last three chars, floor of four.
fn stem(w: &str) -> String {
    let n = w.chars().count().saturating_sub(3).max(4);
    w.chars().take(n).collect()
}

fn near(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    let (sa, sb) = (stem(a), stem(b));
    if sa == sb {
        return true;
    }
    a.chars().count() > 4 && b.starts_with(&sa)
}

/// Judges `attempt` against `target`: meaning, not spelling, any language.
///
/// Typing the word itself is wrong; a synonym or gloss is correct; a word
/// from the meaning alone is close; anything else is wrong with a reason.
pub fn judge(attempt: &str, target: &RecallTarget) -> Judged {
    let mine = words(attempt);
    if mine.is_empty() {
        return Judged {
            verdict: Verdict::Wrong,
            feedback:
                "I couldn't read an answer there. Try a word or a short phrase, in any language."
                    .to_owned(),
        };
    }
    if mine.len() == 1 && mine.first().is_some_and(|m| near(m, &target.lemma)) {
        return Judged {
            verdict: Verdict::Wrong,
            feedback: "That's the word itself. Say what it means: a synonym, a short explanation, or a translation."
                .to_owned(),
        };
    }
    let strong: Vec<String> = target
        .synonyms
        .iter()
        .flat_map(|s| words(s))
        .chain(words(&target.gloss))
        .collect();
    let lemma_stem = stem(&target.lemma.to_lowercase());
    let weak: Vec<String> = words(&target.meaning)
        .into_iter()
        .filter(|w| stem(w) != lemma_stem)
        .collect();
    if let Some(hit) = mine.iter().find(|m| strong.iter().any(|s| near(m, s))) {
        return Judged {
            verdict: Verdict::Correct,
            feedback: format!(
                "Yes. In this sentence “{}” means: {} “{}” fits.",
                target.lemma, target.meaning, hit
            ),
        };
    }
    if let Some(hit) = mine.iter().find(|m| weak.iter().any(|s| near(m, s))) {
        return Judged {
            verdict: Verdict::Close,
            feedback: format!(
                "Close. “{hit}” points the right way, but be more exact. Here it means: {}",
                target.meaning
            ),
        };
    }
    let shown: String = attempt.chars().take(60).collect();
    Judged {
        verdict: Verdict::Wrong,
        feedback: format!(
            "Not quite. “{shown}” doesn't fit this sentence. {}",
            target.meaning
        ),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn target() -> RecallTarget {
        RecallTarget {
            lemma: "acknowledge".into(),
            meaning: "Accepted or admitted to be true.".into(),
            synonyms: vec!["recognize".into(), "admit".into()],
            gloss: "reconnu".into(),
        }
    }

    #[test]
    fn empty_is_wrong_with_a_reason() {
        let j = judge("   ", &target());
        assert_eq!(j.verdict, Verdict::Wrong);
        assert!(j.feedback.contains("couldn't read"));
    }

    #[test]
    fn repeating_the_word_is_wrong() {
        let j = judge("acknowledge", &target());
        assert_eq!(j.verdict, Verdict::Wrong);
        assert!(j.feedback.contains("word itself"));
    }

    #[test]
    fn a_synonym_is_correct() {
        let j = judge("I recognize it", &target());
        assert_eq!(j.verdict, Verdict::Correct);
        assert!(j.feedback.contains("Yes"));
    }

    #[test]
    fn a_gloss_is_correct_in_another_language() {
        let j = judge("reconnu", &target());
        assert_eq!(j.verdict, Verdict::Correct);
    }

    #[test]
    fn a_meaning_word_is_close() {
        let j = judge("admitted", &target());
        assert_eq!(j.verdict, Verdict::Correct);
        assert_eq!(judge("accepted", &target()).verdict, Verdict::Close);
    }

    #[test]
    fn nonsense_is_wrong_with_the_sentence_reason() {
        let j = judge("purple monkey dishwasher", &target());
        assert_eq!(j.verdict, Verdict::Wrong);
        assert!(j.feedback.contains("doesn't fit"));
    }
}
