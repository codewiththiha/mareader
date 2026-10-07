//! Gloss highlights: the document's marks plus the transient selection
//! states.

use std::collections::HashMap;

use ai_core::gloss::{GlossMark, ReflowSpot};
use leptos::prelude::*;

/// The OPEN document's persisted marks, one flat list.
#[derive(Clone, Copy, Default)]
pub struct GlossState {
    pub marks: RwSignal<Vec<GlossMark>>,
    /// Gloss multi-select mode (long-press initiated on a mark).
    pub selection_active: RwSignal<bool>,
    /// Ids of the marks currently selected while in multi-select mode.
    pub selected_marks: RwSignal<std::collections::HashSet<String>>,
    /// The mark whose "processing" animation is live, if any.
    pub processing_id: RwSignal<Option<String>>,
    /// The pane's reflowable spots, parsed once per context.
    pub spots: SpotMemo,
}

impl GlossState {
    /// Clear every field to its resting state.
    pub fn reset(&self) {
        let Self {
            marks,
            selection_active,
            selected_marks,
            processing_id,
            spots,
        } = *self;
        marks.set(Vec::new());
        selection_active.set(false);
        selected_marks.set(std::collections::HashSet::new());
        processing_id.set(None);
        // The memo is keyed by content, so a
        // stale entry is never wrong.
        spots.clear();
    }
}

/// Parsed spots, memoized by the context string.
#[derive(Clone, Copy)]
pub struct SpotMemo(StoredValue<HashMap<String, Option<ReflowSpot>>, LocalStorage>);

impl Default for SpotMemo {
    fn default() -> Self {
        Self(StoredValue::new_local(HashMap::new()))
    }
}

impl SpotMemo {
    /// The memo's bound: past this it is cleared whole.
    const CAP: usize = 512;

    /// The memoized answer for `context`.
    pub fn get(&self, context: &str) -> Option<Option<ReflowSpot>> {
        self.0
            .try_with_value(|memo| memo.get(context).copied())
            .flatten()
    }

    /// Remember `spot` for `context`.
    pub fn insert(&self, context: &str, spot: Option<ReflowSpot>) {
        let _ = self.0.try_update_value(|memo| {
            if memo.len() >= Self::CAP {
                memo.clear();
            }
            memo.insert(context.to_string(), spot);
        });
    }

    /// Drop every entry and its capacity.
    pub fn clear(&self) {
        let _ = self.0.try_update_value(|memo| {
            memo.clear();
            memo.shrink_to_fit();
        });
    }

    /// How many contexts are memoized.
    pub fn len(&self) -> usize {
        self.0.try_with_value(HashMap::len).unwrap_or(0)
    }

    /// Nothing memoized.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}
