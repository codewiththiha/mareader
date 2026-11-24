//! An AI deck shown once half its backs are in ([`DeckJob::early`]).
//!
//! The cards written so far go to the reader at once, their classes
//! settled from the tagger and the AI only; the job writes on. Its whole
//! deck comes later, and the open deck takes the cards it lacks
//! (`Open::land`).

use mareader_study::mode::Classes;

use super::classes::settled;
use super::{Built, DeckJob, Stage};
use crate::study_mode::backs::Shown;

impl DeckJob {
    /// The cards written so far, once at least half of them are and some
    /// requests are still out; only ever once.
    pub fn early(&mut self) -> Option<Built> {
        let Stage::Writing { jobs, .. } = &self.stage else {
            return None;
        };
        let done = self.written.iter().flatten().count();
        let half = self.written.len().div_ceil(2);
        if self.handed || jobs.is_empty() || done < half.max(1) {
            return None;
        }
        self.handed = true;
        let pairs = self.deck.cards.iter().zip(&self.written);
        let heard = pairs.filter_map(|(card, back)| {
            let back = back.clone()?;
            let classes = Classes {
                tagger: card.key.pos,
                ai: back.pos,
                dictionary: None,
            };
            Some((Shown::written(card.clone(), Some(back)), classes))
        });
        let (cards, known) = settled(heard.collect(), &self.known);
        Some(Built {
            cards,
            found: self.deck.found,
            skipped_known: self.deck.skipped_known.saturating_add(known),
            via: self.via,
        })
    }
}
