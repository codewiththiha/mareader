//! A word asked of an explainer off the frame, and the answer that comes.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

use mareader_library::{Gloss, Mark};
use mareader_study::{Explainer, Failure, WordInfo};

use crate::session::Waker;

/// What a worker sends back: the answer so far, then the end.
pub enum Got {
    Partial(WordInfo),
    Done(Result<Said, Failure>),
}

/// A finished answer and who gave it.
#[derive(Clone, Debug, PartialEq)]
pub struct Said {
    pub info: WordInfo,
    pub by: String,
}

impl Said {
    /// The answer a mark saved, if it has one.
    pub fn of(mark: &Mark) -> Option<Self> {
        let g = mark.gloss.value.as_ref()?;
        let info = WordInfo {
            pos: g.pos.clone(),
            meaning: g.meaning.clone(),
            synonyms: g.synonyms.clone(),
            usages: g.usages.clone(),
        };
        Some(Self {
            info,
            by: g.by.clone(),
        })
    }

    /// The answer as a mark saves it.
    pub fn gloss(&self) -> Gloss {
        Gloss {
            pos: self.info.pos.clone(),
            meaning: self.info.meaning.clone(),
            synonyms: self.info.synonyms.clone(),
            usages: self.info.usages.clone(),
            by: self.by.clone(),
        }
    }
}

/// Asks `explainer` about `word` on a thread; `wake` asks for a frame.
pub fn ask(
    wake: Waker,
    explainer: Arc<dyn Explainer>,
    by: String,
    word: String,
    context: String,
) -> Receiver<Got> {
    let (tx, rx) = channel();
    std::thread::spawn(move || {
        let partial = |info| {
            let _ = tx.send(Got::Partial(info));
            wake();
        };
        let done = explainer.explain(&word, &context, &partial);
        let said = done.map(|i| Said {
            info: i.clamped(),
            by,
        });
        let _ = tx.send(Got::Done(said));
        wake();
    });
    rx
}

#[cfg(test)]
#[path = "asking_tests.rs"]
mod tests;
