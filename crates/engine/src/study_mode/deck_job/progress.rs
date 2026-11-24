//! How far a deck being made has come, as its waiting card and corner
//! toast show it.

use mareader_study::mode::Written;

use super::{DeckJob, Doing, Stage};

impl DeckJob {
    pub fn doing(&self) -> Doing {
        match self.stage {
            Stage::Writing { .. } => Doing::Writing,
            Stage::Meanings { .. } => Doing::LookingUp,
            _ => Doing::Finding,
        }
    }

    /// The cards written out of the deck's, while the writer works.
    pub fn progress(&self) -> Option<(usize, usize)> {
        matches!(self.stage, Stage::Writing { .. }).then(|| written(&self.written))
    }

    /// Whether its writer waits in line for a model.
    pub fn queued(&self) -> bool {
        matches!(&self.stage, Stage::Writing { job, .. } if job.queued())
    }
}

/// The backs found so far, cached ones first, and all there are to find.
fn written(backs: &[Option<Written>]) -> (usize, usize) {
    (backs.iter().flatten().count(), backs.len())
}

#[cfg(test)]
mod tests {
    use mareader_study::mode::{Key, Wanted};

    use super::super::fill;
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
}
