//! Who judges a recall: the reader's answer for a word in its sentence.
//!
//! The engine hands an [`Attempt`] to a [`Judge`] and polls the job it gets
//! back; dropping the job cancels it. [`Practice`] judges from a script, for
//! tests and shots: see [`practice_verdict`].

use super::cards::Once;
use super::{Practice, Verdict};
use crate::{Failure, Why};

/// One answer to judge: the word as printed, its lemma and its sentence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attempt {
    pub form: String,
    pub lemma: String,
    pub sentence: String,
    pub answer: String,
}

/// What the judge made of an answer, and why, in a sentence or two.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Judged {
    pub verdict: Verdict,
    pub feedback: String,
}

pub type Judgement = Result<Judged, Failure>;

/// Judges recalls off the UI thread.
pub trait Judge: Send + Sync {
    /// Who judges, as the popover names them while it waits.
    fn by(&self) -> String;

    /// Starts on `attempt`; dropping the job cancels it.
    fn judge(&self, attempt: Attempt) -> Box<dyn JudgeJob>;
}

/// An answer being judged.
pub trait JudgeJob: Send {
    /// The judgement once it came; after that, none.
    fn take(&mut self) -> Option<Judgement>;
}

/// The scripted verdict: "right" in the answer is correct, "close" is close.
pub fn practice_verdict(answer: &str) -> Verdict {
    let answer = answer.to_lowercase();
    if answer.contains("right") {
        Verdict::Correct
    } else if answer.contains("close") {
        Verdict::Close
    } else {
        Verdict::Wrong
    }
}

impl Judge for Practice {
    fn by(&self) -> String {
        "Sample answers".to_owned()
    }

    fn judge(&self, attempt: Attempt) -> Box<dyn JudgeJob> {
        let verdict = practice_verdict(&attempt.answer);
        let word = &attempt.lemma;
        let feedback = match verdict {
            Verdict::Correct => format!("Yes, that is what “{word}” means here."),
            Verdict::Close => format!("Close, but say what “{word}” means more exactly."),
            Verdict::Wrong => format!("That doesn’t fit “{word}” in this sentence."),
        };
        let judged = match self {
            Self::Ready => Some(Ok(Judged { verdict, feedback })),
            Self::Silent => None,
            Self::Failing => Some(Err(Failure::detailed(Why::Offline, "scripted failure"))),
        };
        Box::new(Once(judged))
    }
}

impl JudgeJob for Once<Judgement> {
    fn take(&mut self) -> Option<Judgement> {
        self.0.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attempt(answer: &str) -> Attempt {
        Attempt {
            form: "brisk".to_owned(),
            lemma: "brisk".to_owned(),
            sentence: "A brisk walk.".to_owned(),
            answer: answer.to_owned(),
        }
    }

    #[test]
    fn practice_judges_by_its_script_once() {
        assert_eq!(practice_verdict("Right: quick"), Verdict::Correct);
        assert_eq!(practice_verdict("close to fast"), Verdict::Close);
        assert_eq!(practice_verdict("slow"), Verdict::Wrong);
        let mut job = Practice::Ready.judge(attempt("right"));
        let judged = job.take().and_then(Result::ok).map(|j| j.verdict);
        assert_eq!(judged, Some(Verdict::Correct));
        assert!(job.take().is_none(), "a judgement comes once");
        assert!(Practice::Silent.judge(attempt("right")).take().is_none());
        let failed = Practice::Failing.judge(attempt("right")).take();
        assert!(matches!(failed, Some(Err(f)) if f.why == Why::Offline));
    }
}
