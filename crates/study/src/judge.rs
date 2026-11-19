//! Reactive recall: a judged answer and its limits.

use serde::{Deserialize, Serialize};

/// Attempts before the answer is shown and the word is missed.
pub const MAX_ATTEMPTS: u8 = 3;

/// How well an answer fits the word in its sentence.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Correct,
    Close,
    Wrong,
}

impl Verdict {
    pub fn label(self) -> &'static str {
        match self {
            Self::Correct => "Correct",
            Self::Close => "Almost",
            Self::Wrong => "Not quite",
        }
    }

    /// Parses a model's verdict word, forgiving case and drift.
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "correct" | "right" | "yes" => Some(Self::Correct),
            "close" | "almost" | "partial" => Some(Self::Close),
            "wrong" | "incorrect" | "no" => Some(Self::Wrong),
            _ => None,
        }
    }
}

/// A judged attempt: the verdict and why, in the answer's language.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Judged {
    pub verdict: Verdict,
    pub feedback: String,
}

impl Judged {
    /// Feedback cut to two short sentences' room.
    pub fn clamped(mut self) -> Self {
        if let Some((byte, _)) = self.feedback.char_indices().nth(600) {
            self.feedback.truncate(byte);
            self.feedback.push('…');
        }
        self.feedback = self.feedback.trim().to_owned();
        self
    }
}

/// Attempts left after `used`, never below zero.
pub fn attempts_left(used: u8) -> u8 {
    MAX_ATTEMPTS.saturating_sub(used)
}

/// Whether a hint is due: after the first miss only.
pub fn hint_due(used: u8, last: Verdict) -> bool {
    used == 1 && last != Verdict::Correct
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdicts_parse_and_label() {
        assert_eq!(Verdict::parse(" Close "), Some(Verdict::Close));
        assert_eq!(Verdict::parse("maybe"), None);
        assert_eq!(Verdict::Correct.label(), "Correct");
    }

    #[test]
    fn attempts_and_hints_follow_the_rules() {
        assert_eq!(attempts_left(1), 2);
        assert_eq!(attempts_left(9), 0);
        assert!(hint_due(1, Verdict::Wrong));
        assert!(!hint_due(2, Verdict::Wrong));
        assert!(!hint_due(1, Verdict::Correct));
    }

    #[test]
    fn long_feedback_is_cut() {
        let j = Judged {
            verdict: Verdict::Wrong,
            feedback: "x".repeat(700),
        }
        .clamped();
        assert_eq!(j.feedback.chars().count(), 601);
    }
}
