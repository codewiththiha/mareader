//! Recalling a word in Reactive mode: tries, a hint, and the outcome (D6).
//!
//! The reader types what a shaded word means, in any language, and a judge
//! answers correct, close or wrong. Up to [`MAX_TRIES`] answers are judged;
//! after the first miss a hint shows; "I don't know" reveals the meaning and
//! saves the word as missed. A judge that fails costs no try.

use serde::{Deserialize, Serialize};

pub const MAX_TRIES: u8 = 3;

/// What the judge made of an answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    Correct,
    Close,
    Wrong,
}

impl Verdict {
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_lowercase().as_str() {
            "correct" => Some(Self::Correct),
            "close" => Some(Self::Close),
            "wrong" => Some(Self::Wrong),
            _ => None,
        }
    }
}

/// Where a recall stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Phase {
    #[default]
    Input,
    Judging,
    /// A close or wrong verdict, with another try or the answer to see.
    Result,
    /// Correct: asks whether to add the word to memorized.
    Ask,
    Revealed,
    /// The judge failed; the answer is kept and nothing was counted.
    Failed,
}

/// How a recall ended, which becomes the word's status.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outcome {
    Learning,
    Memorized,
    Missed,
}

/// A finished recall, as it is saved.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Done {
    pub outcome: Outcome,
    pub attempts: u8,
    pub first_try: bool,
}

/// One word's recall, from the first answer to its outcome.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Recall {
    pub phase: Phase,
    pub attempts: u8,
    pub verdict: Option<Verdict>,
    pub feedback: String,
}

impl Recall {
    /// Sends `answer` to be judged; false when there is nothing to send.
    pub fn check(&mut self, answer: &str) -> bool {
        let ready = self.phase == Phase::Input && self.can_try();
        if !ready || answer.trim().is_empty() {
            return false;
        }
        self.phase = Phase::Judging;
        true
    }

    /// The judge's answer; done at once when correct and not asking.
    pub fn judged(&mut self, verdict: Verdict, feedback: String, ask: bool) -> Option<Done> {
        if self.phase != Phase::Judging {
            return None;
        }
        self.attempts = self.attempts.saturating_add(1);
        self.verdict = Some(verdict);
        self.feedback = feedback;
        match verdict {
            Verdict::Correct if ask => {
                self.phase = Phase::Ask;
                None
            }
            Verdict::Correct => Some(self.finish(Outcome::Memorized)),
            Verdict::Close | Verdict::Wrong => {
                self.phase = Phase::Result;
                None
            }
        }
    }

    /// The judge could not answer: no try is counted.
    pub fn failed(&mut self) {
        if self.phase == Phase::Judging {
            self.phase = Phase::Failed;
        }
    }

    /// Back to the answer box; false when no try is left.
    pub fn again(&mut self) -> bool {
        let open = match self.phase {
            Phase::Result => self.can_try(),
            Phase::Failed => true,
            _ => false,
        };
        if open {
            self.phase = Phase::Input;
        }
        open
    }

    /// "I don't know" or "Show answer".
    pub fn reveal(&mut self) {
        if matches!(self.phase, Phase::Input | Phase::Result) {
            self.phase = Phase::Revealed;
        }
    }

    /// A hint shows after the first miss only.
    pub fn hint(&self) -> bool {
        self.phase == Phase::Result && self.attempts == 1
    }

    pub fn can_try(&self) -> bool {
        self.attempts < MAX_TRIES
    }

    /// The recall as saved; at least one attempt is counted.
    pub fn finish(&self, outcome: Outcome) -> Done {
        Done {
            outcome,
            attempts: self.attempts.max(1),
            first_try: self.attempts == 1 && outcome == Outcome::Memorized,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn a_first_try_correct_asks_then_counts_as_first_try() {
        let mut r = Recall::default();
        assert!(!r.check("   "));
        assert!(r.check("annoy"));
        assert_eq!(r.judged(Verdict::Correct, "Yes.".into(), true), None);
        assert_eq!(r.phase, Phase::Ask);
        let done = r.finish(Outcome::Memorized);
        assert_eq!((done.attempts, done.first_try), (1, true));
        let mut quick = Recall::default();
        quick.check("annoy");
        let done = quick.judged(Verdict::Correct, String::new(), false);
        assert_eq!(done.map(|d| d.outcome), Some(Outcome::Memorized));
    }

    #[test]
    fn misses_hint_once_and_stop_after_three() {
        let mut r = Recall::default();
        for n in 1..=MAX_TRIES {
            assert!(r.check("wrong guess"));
            r.judged(Verdict::Wrong, String::new(), true);
            assert_eq!(r.hint(), n == 1);
            assert_eq!(r.again(), n < MAX_TRIES);
        }
        assert_eq!(r.phase, Phase::Result);
        assert!(!r.check("one more"));
        r.reveal();
        let done = r.finish(Outcome::Missed);
        assert_eq!((done.attempts, done.first_try), (3, false));
    }

    #[test]
    fn a_failed_judge_costs_no_try() {
        let mut r = Recall::default();
        r.check("annoy");
        r.failed();
        assert_eq!((r.phase, r.attempts), (Phase::Failed, 0));
        assert!(r.again());
        assert_eq!(r.judged(Verdict::Close, String::new(), true), None);
        assert_eq!(r.phase, Phase::Input);
    }

    #[test]
    fn i_do_not_know_saves_a_missed_word_with_one_attempt() {
        let mut r = Recall::default();
        r.reveal();
        assert_eq!(r.phase, Phase::Revealed);
        assert_eq!(r.finish(Outcome::Missed).attempts, 1);
        assert_eq!(Verdict::parse(" Close "), Some(Verdict::Close));
        assert_eq!(Verdict::parse("maybe"), None);
    }
}
