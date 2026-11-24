//! Cards an AI writes: what it is asked, what it answers, and who answers.
//!
//! The engine hands a deck's words to a [`CardWriter`] and polls the job it
//! gets back; dropping the job cancels it. [`Practice`] writers answer from
//! a script, for tests and shots.

use super::Key;
use crate::pending::{News, Pending};
use crate::{Failure, Why};

/// Seconds a deck waits on its writer before the dictionary is offered.
pub const SLOW_SECS: f64 = 7.0;

/// One word a card is wanted for, as the book uses it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wanted {
    pub key: Key,
    pub form: String,
    pub sentence: String,
}

/// One card's back as the AI wrote it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Written {
    pub meaning: String,
    pub synonyms: Vec<String>,
    pub usage: String,
}

/// A writer's answer: one back per word asked, in order, or none.
pub type Answer = Result<Vec<Option<Written>>, Failure>;

/// Writes a deck's backs off the UI thread.
pub trait CardWriter: Send + Sync {
    /// Who writes, as the waiting card names them.
    fn by(&self) -> String;

    /// Who writes and in which language; cached backs are kept apart by it.
    fn id(&self) -> String;

    /// Starts on `words`; dropping the job cancels it.
    fn write(&self, words: Vec<Wanted>) -> Box<CardJob>;
}

/// A deck's backs being written.
pub type CardJob = dyn Pending<Vec<Option<Written>>>;

/// Scripted writers, for tests and shots.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Practice {
    /// Writes a plain back for every word at once.
    Ready,
    /// Never answers, so the deck keeps waiting.
    Silent,
    /// Fails as an unreachable server does.
    Failing,
    /// Fails as a provider with no API key does.
    Keyless,
}

impl Practice {
    pub fn parse(name: &str) -> Option<Self> {
        match name.trim() {
            "ready" => Some(Self::Ready),
            "silent" => Some(Self::Silent),
            "failing" => Some(Self::Failing),
            "keyless" => Some(Self::Keyless),
            _ => None,
        }
    }
}

/// The back [`Practice::Ready`] writes for `wanted`.
pub fn practice_back(wanted: &Wanted) -> Written {
    Written {
        meaning: format!("What “{}” means in this sentence.", wanted.form),
        synonyms: vec!["sample".to_owned()],
        usage: format!("A sample sentence with “{}”.", wanted.key.lemma),
    }
}

impl CardWriter for Practice {
    fn by(&self) -> String {
        "Sample answers".to_owned()
    }

    fn id(&self) -> String {
        format!("practice-{self:?}")
    }

    fn write(&self, words: Vec<Wanted>) -> Box<CardJob> {
        let backs: Vec<_> = words.iter().map(|w| Some(practice_back(w))).collect();
        let answer = match self {
            Self::Ready => Some(Ok(backs)),
            Self::Silent => None,
            Self::Failing => Some(Err(Failure::detailed(Why::Offline, "scripted failure"))),
            Self::Keyless => Some(Err(Failure::detailed(Why::NoKey, "scripted failure"))),
        };
        Box::new(Once(answer))
    }
}

/// An answer known from the start, or none ever.
pub(super) struct Once<T>(pub(super) Option<T>);

impl<O: Send> Pending<O> for Once<Result<O, Failure>> {
    fn news(&mut self) -> Vec<News<O>> {
        self.0.take().map(News::Done).into_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wanted(lemma: &str) -> Wanted {
        Wanted {
            key: Key::new(lemma, None),
            form: lemma.to_owned(),
            sentence: format!("A {lemma} day."),
        }
    }

    #[test]
    fn practice_writers_answer_once_wait_or_fail() {
        let mut ready = Practice::Ready.write(vec![wanted("brisk"), wanted("arcane")]);
        let backs = ready.take().and_then(Result::ok).unwrap_or_default();
        assert_eq!(backs.len(), 2);
        assert!(backs.iter().all(Option::is_some));
        assert!(ready.take().is_none(), "an answer comes once");
        assert!(
            Practice::Silent
                .write(vec![wanted("brisk")])
                .take()
                .is_none()
        );
        let failed = Practice::Failing.write(Vec::new()).take();
        assert!(matches!(failed, Some(Err(f)) if f.why == Why::Offline));
        assert_eq!(Practice::parse("silent"), Some(Practice::Silent));
        assert_eq!(Practice::parse("slow"), None);
        assert_ne!(Practice::Ready.id(), Practice::Failing.id());
    }
}
