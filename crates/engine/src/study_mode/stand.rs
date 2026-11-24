//! Where a studying pane's pages stand, as the page grid marks them.
//!
//! A page reads freely, or it is locked: shut while nothing is written for
//! it, writing while the AI writes its cards, ready once they wait at the
//! gate. A deck the AI waits in line for, or one that failed, is still
//! shut. The dictionary writes a deck at once, so its locked pages are
//! always ready. The pages before a locked start are shut, as they are
//! never studied.

use std::ops::Range;

use mareader_study::mode::{Batch, Mode, Via};

use super::deck_job::DeckJob;
use super::run::{Ahead, DeckState, Run};

/// Where a page stands in a pane's study.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stand {
    Free,
    /// Locked with nothing written for it yet, or never studied.
    Shut,
    /// Locked while the AI writes its cards.
    Writing,
    /// Locked, its cards ready.
    Ready,
}

/// The locked pages span by span, in order; pages in none read freely.
pub type Stands = Vec<(Range<usize>, Stand)>;

impl Run {
    /// How its pages stand: `lock` shuts those before the start, and `via`
    /// names who can write a batch with no deck yet, if anyone.
    pub fn stands(&self, lock: bool, via: impl Fn(&Batch) -> Option<Via>) -> Stands {
        let mut out = Stands::new();
        if self.mode != Mode::Proactive {
            return out;
        }
        let floor = self.floor(lock);
        if floor > 0 {
            out.push((0..floor, Stand::Shut));
        }
        let Some(gate) = self.gate() else {
            return out;
        };
        if !self.planned() {
            out.push((gate..usize::MAX, Stand::Shut));
            return out;
        }
        for batch in self.batches.iter().filter(|b| b.to >= gate) {
            let pages = batch.from.max(gate)..batch.to.saturating_add(1);
            out.push((pages, self.stand(batch, via(batch))));
        }
        out
    }

    /// How locked `batch` stands: as its deck does, else as `via` writes.
    fn stand(&self, batch: &Batch, via: Option<Via>) -> Stand {
        let ahead = self.ahead.iter().find(|a| a.batch() == batch);
        match (&self.deck, ahead) {
            (DeckState::Open(open), _) if open.batch == *batch => Stand::Ready,
            (DeckState::Making { batch: at, job }, _) if at == batch => of_job(job),
            (DeckState::Failed { batch: at, .. }, _) if at == batch => Stand::Shut,
            (_, Some(Ahead::Ready { .. })) => Stand::Ready,
            (_, Some(Ahead::Making { job, .. })) => of_job(job),
            (_, Some(Ahead::Failed { .. })) => Stand::Shut,
            _ if via == Some(Via::Dictionary) => Stand::Ready,
            _ => Stand::Shut,
        }
    }
}

/// A deck being made: the dictionary's is ready at once, the AI's writing
/// unless it waits in line.
fn of_job(job: &DeckJob) -> Stand {
    match (job.via(), job.queued()) {
        (Via::Dictionary, _) => Stand::Ready,
        (Via::Ai, true) => Stand::Shut,
        (Via::Ai, false) => Stand::Writing,
    }
}
