//! The vocabulary highlighter's per-pane state: answered words, the asks in
//! flight, and the repaint generation.

use std::collections::HashSet;

use cefr_core::cache::LevelCache;
use leptos::prelude::*;

/// The open document's answered words, plus the bump that makes painted
/// pages re-derive.
#[derive(Clone, Copy)]
pub struct CefrState {
    /// Words answered by the dataset, bands 0..=6; misses included.
    pub levels: StoredValue<LevelCache>,
    /// Keys asked of the backend and not yet answered: one ask each.
    pub pending: StoredValue<HashSet<String>>,
    /// Raised on cache or input changes: pages re-derive per bump.
    pub generation: RwSignal<u64>,
}

impl CefrState {
    /// The misses nobody has asked for yet, claimed for this ask.
    pub fn claim(&self, misses: Vec<String>) -> Vec<String> {
        let mut claimed: Vec<String> = Vec::new();
        let _ = self.pending.try_update_value(|pending| {
            for key in misses {
                if pending.insert(key.clone()) {
                    claimed.push(key);
                }
            }
        });
        claimed
    }

    /// Merge one round's answers and release the keys that were asked.
    pub fn ingest(&self, keys: Vec<String>, levels: Vec<Option<f64>>) {
        let answered = self
            .levels
            .try_update_value(|cache| {
                let mut count = 0usize;
                for (key, level) in keys.iter().zip(levels) {
                    cache.insert(key, cefr_core::band(level));
                    count += 1;
                }
                count
            })
            .unwrap_or(0);
        self.release(&keys);
        // Nothing answered is nothing changed: a dead backend must not loop.
        if answered > 0 {
            let _ = self.generation.try_update(|n| *n = n.wrapping_add(1));
        }
    }

    /// Drop everything a document answered: the next one is its own.
    pub fn invalidate(&self) {
        let _ = self.levels.try_update_value(LevelCache::clear);
        self.release_all();
        let _ = self.generation.try_update(|n| *n = n.wrapping_add(1));
    }

    fn release(&self, keys: &[String]) {
        let _ = self.pending.try_update_value(|pending| {
            for key in keys {
                pending.remove(key);
            }
        });
    }

    fn release_all(&self) {
        let _ = self.pending.try_update_value(HashSet::clear);
    }
}

impl Default for CefrState {
    fn default() -> Self {
        Self {
            levels: StoredValue::new(LevelCache::default()),
            pending: StoredValue::new(HashSet::new()),
            generation: RwSignal::new(0),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ingesting_answers_bumps_the_generation_once() {
        let owner = leptos::prelude::Owner::new();
        owner.with(|| {
            let state = CefrState::default();
            let before = state.generation.get_untracked();
            state.ingest(
                vec!["ephemeral".into(), "zzz".into()],
                vec![Some(5.4), None],
            );
            assert_eq!(state.generation.get_untracked(), before + 1);
            let bands = state.levels.get_value();
            assert_eq!(bands.get("ephemeral"), Some(5));
            assert_eq!(bands.get("zzz"), Some(0));
        });
    }

    #[test]
    fn a_key_asked_twice_before_it_answers_is_asked_once() {
        let owner = leptos::prelude::Owner::new();
        owner.with(|| {
            let state = CefrState::default();
            let asked = vec!["ephemeral".to_string(), "run".to_string()];
            assert_eq!(state.claim(asked.clone()), asked);
            // A second walk over the same row, before the answer landed.
            assert!(state.claim(asked.clone()).is_empty());
            state.ingest(asked.clone(), vec![Some(5.0), None]);
            // Released by the answer, so a later miss can be asked again.
            assert_eq!(state.claim(asked.clone()), asked);
        });
    }

    #[test]
    fn an_empty_answer_releases_without_repainting() {
        let owner = leptos::prelude::Owner::new();
        owner.with(|| {
            let state = CefrState::default();
            let asked = vec!["ephemeral".to_string()];
            state.claim(asked.clone());
            let before = state.generation.get_untracked();
            // A failed lookup answers nothing: no band, and no re-walk storm.
            state.ingest(asked.clone(), Vec::new());
            assert_eq!(state.generation.get_untracked(), before);
            assert!(state.levels.get_value().is_empty());
            assert_eq!(state.claim(asked), vec!["ephemeral".to_string()]);
        });
    }

    #[test]
    fn invalidating_clears_the_words_and_the_asks_in_flight() {
        let owner = leptos::prelude::Owner::new();
        owner.with(|| {
            let state = CefrState::default();
            state.claim(vec!["run".into()]);
            state.ingest(vec!["run".into()], vec![Some(2.0)]);
            state.claim(vec!["ephemeral".into()]);
            state.invalidate();
            assert!(state.levels.get_value().is_empty());
            // An ask in flight when the document changed is nobody's now.
            assert_eq!(
                state.claim(vec!["ephemeral".into()]),
                vec!["ephemeral".to_string()]
            );
        });
    }
}
