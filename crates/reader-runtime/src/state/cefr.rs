//! The vocabulary highlighter's per-pane state: answered words and the
//! repaint generation.

use cefr_core::{LevelCache, band_of};
use leptos::prelude::*;

/// The open document's answered words, plus the bump that makes painted
/// pages re-derive.
#[derive(Clone, Copy)]
pub struct CefrState {
    /// Words answered by the dataset, bands 0..=6; misses included.
    pub levels: StoredValue<LevelCache>,
    /// Raised on cache or input changes: pages re-derive per bump.
    pub generation: RwSignal<u64>,
}

impl CefrState {
    /// Merge one round's answers, raise the generation; all `try_` —
    /// answers may land after disposal.
    pub fn ingest(&self, words: Vec<String>, levels: Vec<Option<f64>>) {
        let _ = self.levels.try_update_value(|cache| {
            for (word, level) in words.into_iter().zip(levels) {
                cache.insert(&word, band_of(level));
            }
        });
        let _ = self.generation.try_update(|n| *n = n.wrapping_add(1));
    }

    /// Invalidate everything painted: the dataset arrived or the rule
    /// changed wholesale.
    pub fn invalidate(&self) {
        let _ = self.levels.try_update_value(LevelCache::clear);
        let _ = self.generation.try_update(|n| *n = n.wrapping_add(1));
    }

    /// Clear on a document change: the next document's words are its own.
    pub fn reset(&self) {
        self.invalidate();
    }
}

impl Default for CefrState {
    fn default() -> Self {
        Self {
            levels: StoredValue::new(LevelCache::default()),
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
    fn invalidating_clears_the_words_but_not_the_generation_watchers() {
        let owner = leptos::prelude::Owner::new();
        owner.with(|| {
            let state = CefrState::default();
            state.ingest(vec!["run".into()], vec![Some(2.0)]);
            state.invalidate();
            assert!(state.levels.get_value().is_empty());
        });
    }
}
