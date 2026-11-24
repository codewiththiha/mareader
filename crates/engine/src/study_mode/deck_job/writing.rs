//! An AI deck's backs: those cached first, then the writer's answer.

use mareader_settings::{TaskId, no_model};
use mareader_study::Why;
use mareader_study::mode::{Wanted, Written};
use mareader_study::pending::News;

use super::{DeckJob, Stage};
use crate::study_mode::backs::wanted;
use crate::study_mode::run::Setup;

impl DeckJob {
    /// Asks the writer for the backs not cached; with all cached, finishes.
    pub(super) fn ask_backs(&mut self) -> Result<(), String> {
        let Some(writer) = self.writer.clone() else {
            self.setup = Some(Setup::Writer);
            return Err(no_model(TaskId::StudyCards));
        };
        let id = writer.cards.id();
        let mut asked = Vec::new();
        self.written = (self.deck.cards.iter().enumerate())
            .map(|(i, card)| {
                let wanted = wanted(card);
                let cached = writer.backs.get(&id, &wanted);
                if cached.is_none() {
                    asked.push((i, wanted));
                }
                cached
            })
            .collect();
        if asked.is_empty() {
            self.finish_written();
            return Ok(());
        }
        let words = asked.iter().map(|(_, w)| w.clone()).collect();
        let job = writer.cards.write(words);
        self.stage = Stage::Writing { job, asked, id };
        Ok(())
    }

    /// Counts backs as they are written; takes the answer into the deck
    /// and the cache.
    pub(super) fn writing(&mut self) -> Result<bool, String> {
        let Stage::Writing { job, asked, id } = &mut self.stage else {
            return Ok(false);
        };
        let mut answer = None;
        for news in job.news() {
            match news {
                News::Partial(backs) => fill(&mut self.written, asked, &backs),
                News::Done(done) => answer = Some(done),
            }
        }
        let Some(answer) = answer else {
            return Ok(false);
        };
        let backs = match answer {
            Ok(backs) => backs,
            Err(failure) => {
                self.setup = setup_for(failure.why);
                return Err(failure.said());
            }
        };
        let (asked, id) = (std::mem::take(asked), std::mem::take(id));
        for ((i, wanted), back) in asked.into_iter().zip(backs) {
            if let (Some(writer), Some(back)) = (&self.writer, &back) {
                writer.backs.put(&id, &wanted, back.clone());
            }
            // The whole answer stands over what came on the way.
            if let Some(slot) = self.written.get_mut(i) {
                *slot = back;
            }
        }
        self.finish_written();
        Ok(true)
    }
}

/// Puts each back written into its card's place, `asked` naming them.
pub(super) fn fill(
    written: &mut [Option<Written>],
    asked: &[(usize, Wanted)],
    backs: &[Option<Written>],
) {
    for ((i, _), back) in asked.iter().zip(backs) {
        if let (Some(slot), Some(back)) = (written.get_mut(*i), back) {
            *slot = Some(back.clone());
        }
    }
}

/// What a model's failure needs set up; none when trying again may do.
fn setup_for(why: Why) -> Option<Setup> {
    match why {
        Why::NotHere => Some(Setup::Writer),
        Why::NoKey | Why::KeysRejected | Why::Setup => Some(Setup::Model),
        _ => None,
    }
}
