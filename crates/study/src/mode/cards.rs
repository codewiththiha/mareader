//! Cards an AI writes: what it is asked, what it answers, and who answers.
//!
//! The engine cuts a deck's words into requests ([`chunks`]) of the size
//! the writer's model takes, hands each to a [`CardWriter`] and polls the
//! jobs it gets back; dropping a job cancels it. [`Practice`] writers
//! answer from a script, for tests and shots.

use std::ops::Range;

use super::Key;
use crate::class::Pos;
use crate::pending::{News, Pending};
use crate::{Failure, Why};

/// Seconds a deck waits on its writer before the dictionary is offered.
pub const SLOW_SECS: f64 = 7.0;
/// Words one request carries unless the model's setting says otherwise.
pub const PER_REQUEST: u8 = 10;
/// Words one request carries at most, whatever the setting says.
pub const PER_REQUEST_MAX: u8 = 20;

/// `n` words cut into requests of about `limit` each, sizes within one.
///
/// There are `n / limit` requests, rounded half up, and at least one, so
/// a small remainder joins the others: with 10, 13 words go in one
/// request, 15 in 8 and 7, and 32 in 11, 11 and 10. No request carries
/// more than 1.5 times `limit`. A `limit` of 0 counts as 1; no words, no
/// requests.
pub fn chunks(n: usize, limit: usize) -> Vec<Range<usize>> {
    let limit = limit.max(1);
    let rounded = n.saturating_mul(2).saturating_add(limit) / limit.saturating_mul(2);
    let k = rounded.clamp(1, n.max(1));
    let (base, extra) = (n / k, n % k);
    let mut start = 0;
    let mut out = Vec::new();
    for i in 0..k {
        let end = start + base + usize::from(i < extra);
        if end > start {
            out.push(start..end);
        }
        start = end;
    }
    out
}

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
    /// The class the AI read the word as, when it named one.
    pub pos: Option<Pos>,
    pub meaning: String,
    pub synonyms: Vec<String>,
    pub usage: String,
    /// The word only matters in its book, as a name does.
    pub book_only: bool,
}

/// A writer's answer: one back per word asked, in order, or none.
pub type Answer = Result<Vec<Option<Written>>, Failure>;

/// Writes a deck's backs off the UI thread.
pub trait CardWriter: Send + Sync {
    /// Who writes, as the waiting card names them.
    fn by(&self) -> String;

    /// Who writes and in which language; cached backs are kept apart by it.
    fn id(&self) -> String;

    /// Words one request carries, as the writer's model is set.
    fn per_request(&self) -> usize;

    /// Starts on `words`; dropping the job cancels it.
    fn write(&self, words: Vec<Wanted>) -> Box<CardJob>;

    /// The language an AI writes the backs in, for the AI dictionary;
    /// none keeps them out, as practice backs are.
    fn language(&self) -> Option<&'static str> {
        None
    }
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
        pos: wanted.key.pos,
        meaning: format!("What “{}” means in this sentence.", wanted.form),
        synonyms: vec!["sample".to_owned()],
        usage: format!("A sample sentence with “{}”.", wanted.key.lemma),
        book_only: false,
    }
}

impl CardWriter for Practice {
    fn by(&self) -> String {
        "Sample answers".to_owned()
    }

    fn id(&self) -> String {
        format!("practice-{self:?}")
    }

    fn per_request(&self) -> usize {
        usize::from(PER_REQUEST)
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

    fn sizes(n: usize, limit: usize) -> Vec<usize> {
        chunks(n, limit).iter().map(Range::len).collect()
    }

    #[test]
    fn words_split_evenly_and_a_small_remainder_joins_in() {
        assert_eq!(sizes(10, 10), [10]);
        for n in [11, 12, 13, 14] {
            assert_eq!(sizes(n, 10), [n], "{n} words fit one request");
        }
        assert_eq!(sizes(15, 10), [8, 7]);
        assert_eq!(sizes(20, 10), [10, 10]);
        assert_eq!(sizes(32, 10), [11, 11, 10]);
        assert_eq!(sizes(5, 10), [5]);
        assert_eq!(sizes(3, 1), [1, 1, 1]);
        assert_eq!(sizes(4, 0), [1, 1, 1, 1], "a limit of 0 counts as 1");
        assert!(chunks(0, 10).is_empty(), "no words, no requests");
        let cut = chunks(32, 10);
        let joined: Vec<usize> = cut.iter().flat_map(Range::clone).collect();
        assert_eq!(joined, (0..32).collect::<Vec<_>>(), "in order, none lost");
        for (n, limit) in [(30, 7), (29, 4), (30, 30), (17, 3), (30, 2)] {
            let most = sizes(n, limit).into_iter().max().unwrap_or(0);
            assert!(most * 2 <= limit * 3, "{n} by {limit}: {most}");
        }
    }
}
