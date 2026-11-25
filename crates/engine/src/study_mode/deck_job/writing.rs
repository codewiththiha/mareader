//! An AI deck's backs: those cached first, then the writer's answers.
//!
//! The words not cached are cut into requests of the writer's size
//! ([`chunks`]) and all asked at once; the model's queue runs as many as
//! it takes and holds the rest. The deck goes on only once every request
//! has answered. A failed request fails the deck, but the backs the
//! others wrote stay cached, so trying again asks only the words missing.
//! Each request's backs also go to the AI dictionary as they come.

use mareader_dict::Saved;
use mareader_settings::{TaskId, no_model};
use mareader_study::Why;
use mareader_study::mode::{CardJob, Wanted, Written, chunks};
use mareader_study::pending::News;

use super::{DeckJob, Stage};
use crate::ai_dict::of_back;
use crate::study_mode::backs::{Writer, wanted};
use crate::study_mode::run::Setup;
use crate::word_list::Meanings;

/// One request's job, and the cards it asks for, by place.
pub(super) struct Chunk {
    pub(super) job: Box<CardJob>,
    pub(super) asked: Vec<(usize, Wanted)>,
}

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
        let mut rest = asked.into_iter();
        let jobs = chunks(rest.len(), writer.cards.per_request())
            .into_iter()
            .map(|range| {
                let asked: Vec<(usize, Wanted)> = rest.by_ref().take(range.len()).collect();
                let words = asked.iter().map(|(_, w)| w.clone()).collect();
                let job = writer.cards.write(words);
                Chunk { job, asked }
            })
            .collect();
        self.stage = Stage::Writing {
            jobs,
            id,
            failed: None,
        };
        Ok(())
    }

    /// Counts backs as they are written; takes each answer into the deck
    /// and the cache, and goes on once every request has answered.
    pub(super) fn writing(&mut self, meanings: &mut dyn Meanings) -> Result<bool, String> {
        let Stage::Writing { jobs, id, failed } = &mut self.stage else {
            return Ok(false);
        };
        let mut moved = false;
        let mut out = Vec::new();
        for mut chunk in std::mem::take(jobs) {
            let mut answer = None;
            for news in chunk.job.news() {
                match news {
                    News::Partial(backs) => fill(&mut self.written, &chunk.asked, &backs),
                    News::Done(done) => answer = Some(done),
                }
            }
            match answer {
                None => out.push(chunk),
                Some(Ok(backs)) => {
                    let writer = self.writer.as_ref();
                    let filed = keep(&mut self.written, writer, id, chunk.asked, backs);
                    if !filed.is_empty() {
                        meanings.remember(filed);
                    }
                    moved = true;
                }
                Some(Err(failure)) => {
                    failed.get_or_insert(failure);
                    moved = true;
                }
            }
        }
        *jobs = out;
        if !jobs.is_empty() {
            return Ok(moved);
        }
        if let Some(failure) = failed.take() {
            self.setup = setup_for(failure.why);
            return Err(failure.said());
        }
        self.finish_written();
        Ok(true)
    }
}

/// One request's whole answer, into the deck's places and the cache; it
/// stands over what came on the way. Returns the backs the AI dictionary
/// takes.
fn keep(
    written: &mut [Option<Written>],
    writer: Option<&Writer>,
    id: &str,
    asked: Vec<(usize, Wanted)>,
    backs: Vec<Option<Written>>,
) -> Vec<Saved> {
    let who = writer.and_then(|w| Some((w.cards.by(), w.cards.language()?)));
    let mut filed = Vec::new();
    for ((i, wanted), back) in asked.into_iter().zip(backs) {
        if let (Some(writer), Some(back)) = (writer, &back) {
            writer.backs.put(id, &wanted, back.clone());
        }
        if let (Some((by, language)), Some(back)) = (&who, &back) {
            filed.extend(of_back(&wanted, back, (by, language)));
        }
        if let Some(slot) = written.get_mut(i) {
            *slot = back;
        }
    }
    filed
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
