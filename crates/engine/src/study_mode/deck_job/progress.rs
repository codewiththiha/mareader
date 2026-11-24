//! How far a deck being made has come, as its waiting card and corner
//! toast show it.
//!
//! [`DeckJob::made`] counts real steps only: the words' levels looked up,
//! their classes tagged, then each card the AI has written, cached ones
//! first. Cards come one by one as the model streams its answer; before
//! the first, nothing moves, as nothing is known.

use mareader_study::mode::Written;

use super::{DeckJob, Doing, Stage};

/// Of the whole deck, the share finding its words takes; writing the rest.
const FINDING: f32 = 0.1;

impl DeckJob {
    pub fn doing(&self) -> Doing {
        match self.stage {
            Stage::Writing { .. } => Doing::Writing,
            Stage::Meanings { .. } | Stage::Classes { .. } => Doing::LookingUp,
            _ => Doing::Finding,
        }
    }

    /// The cards written out of the deck's, while the writer works.
    pub fn progress(&self) -> Option<(usize, usize)> {
        matches!(self.stage, Stage::Writing { .. }).then(|| written(&self.written))
    }

    /// How much of the deck is made, 0 to 1.
    pub fn made(&self) -> f32 {
        let found = match &self.stage {
            Stage::Reading(_) => 0.0,
            Stage::Levels { next } => part(*next, self.keys.len()) / 2.0,
            Stage::Tags { asked, of, .. } => {
                let tagged = of.saturating_sub(asked.len());
                0.5 + part(tagged, *of) / 2.0
            }
            _ => 1.0,
        };
        let cards = match &self.stage {
            Stage::Writing { .. } => {
                let (done, of) = written(&self.written);
                part(done, of)
            }
            Stage::Settle | Stage::Classes { .. } | Stage::Done => 1.0,
            _ => 0.0,
        };
        whole(found, cards)
    }

    /// Whether its writer waits in line for a model: every request still
    /// out is queued, none running.
    pub fn queued(&self) -> bool {
        let Stage::Writing { jobs, .. } = &self.stage else {
            return false;
        };
        !jobs.is_empty() && jobs.iter().all(|chunk| chunk.job.queued())
    }
}

/// `n` of `of`, as a share; none of nothing.
fn part(n: usize, of: usize) -> f32 {
    n as f32 / of.max(1) as f32
}

/// The deck's share made, with `found` of its words and `cards` written.
fn whole(found: f32, cards: f32) -> f32 {
    (FINDING * found + (1.0 - FINDING) * cards).clamp(0.0, 1.0)
}

/// The backs found so far, cached ones first, and all there are to find.
fn written(backs: &[Option<Written>]) -> (usize, usize) {
    (backs.iter().flatten().count(), backs.len())
}

#[cfg(test)]
mod tests {
    use mareader_study::mode::{Key, Wanted};

    use super::super::writing::fill;
    use super::*;

    fn back(meaning: &str) -> Option<Written> {
        Some(Written {
            meaning: meaning.to_owned(),
            ..Written::default()
        })
    }

    #[test]
    fn progress_counts_backs_as_they_come() {
        assert_eq!(written(&[]), (0, 0));
        let wanted = |lemma: &str| Wanted {
            key: Key::new(lemma, None),
            form: lemma.to_owned(),
            sentence: String::new(),
        };
        let mut backs = vec![back("cached"), None, None];
        let asked = [(1, wanted("brisk")), (2, wanted("arcane"))];
        fill(&mut backs, &asked, &[back("lively"), None]);
        assert_eq!(written(&backs), (2, 3), "one more card as it comes");
        fill(&mut backs, &asked, &[None, back("hidden")]);
        assert_eq!(backs, [back("cached"), back("lively"), back("hidden")]);
    }

    #[test]
    fn made_counts_the_words_found_then_each_card() {
        assert_eq!(whole(0.0, 0.0), 0.0);
        assert_eq!(whole(1.0, 0.0), FINDING, "found, no card yet");
        let half = whole(1.0, part(4, 8));
        assert!((half - (FINDING + (1.0 - FINDING) / 2.0)).abs() < 1e-6);
        assert_eq!(whole(1.0, part(8, 8)), 1.0);
        assert_eq!(part(0, 0), 0.0);
    }
}
