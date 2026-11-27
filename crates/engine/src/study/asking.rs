//! A word asked of an explainer off the frame, and the answer that comes.
//! Dropping its [`Ask`] stops it: the explainer gives up and frees its model.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};

use mareader_library::{Gloss, Mark};
use mareader_study::{Explainer, Failure, Pos, WordInfo};

use crate::session::Waker;

/// What a worker sends back: the answer so far, then the end.
pub enum Got {
    Partial(WordInfo),
    Reset,
    Done(Result<Said, Failure>, Option<&'static str>),
}

/// A finished answer, who gave it, and the word asked.
#[derive(Clone, Debug, PartialEq)]
pub struct Said {
    pub info: WordInfo,
    pub by: String,
    pub word: String,
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
            book_only: g.book_only,
        };
        Some(Self {
            info,
            by: g.by.clone(),
            word: mark.word.clone(),
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
            book_only: self.info.book_only,
        }
    }
}

/// A word being asked: what comes back, and the flag that stops it.
pub struct Ask {
    pub rx: Receiver<Got>,
    stop: Arc<AtomicBool>,
}

impl Drop for Ask {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// Asks `explainer` about a word, its sentence and the tagger's class
/// on a thread; `wake` asks for a frame.
pub fn ask(
    wake: Waker,
    explainer: Arc<dyn Explainer>,
    (word, context, hint): (String, String, Option<Pos>),
) -> Ask {
    let (tx, rx) = channel();
    let stop = Arc::new(AtomicBool::new(false));
    let told = Arc::clone(&stop);
    std::thread::spawn(move || {
        let partial = |info| {
            let _ = tx.send(Got::Partial(info));
            wake();
        };
        let stopped = || told.load(Ordering::Relaxed);
        let words = (word.as_str(), context.as_str(), hint);
        let mut current = explainer;
        let mut done = current.explain_until(words, &partial, &stopped);
        if !stopped()
            && done
                .as_ref()
                .is_err_and(|f| f.why == mareader_study::Why::Language)
            && let Some(next) = current.compatible_snapshot()
        {
            let _ = tx.send(Got::Reset);
            wake();
            current = next;
            done = current.explain_until(words, &partial, &stopped);
        }
        let language = current.language();
        let said = done.map(|(info, by)| Said {
            info: info.clamped(),
            by,
            word: word.clone(),
        });
        let _ = tx.send(Got::Done(said, language));
        wake();
    });
    Ask { rx, stop }
}

#[cfg(test)]
#[path = "asking_tests.rs"]
mod tests;
