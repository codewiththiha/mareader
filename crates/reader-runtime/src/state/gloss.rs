//! Gloss highlights: the persisted marks of the open document, plus the
//! transient multi-select and "thinking" states the mark layer paints.

use std::collections::HashMap;

use ai_core::gloss::{GlossMark, ReflowSpot};
use leptos::prelude::*;

/// The persisted gloss highlights of the OPEN document. One flat list rather
/// than a per-page map: a document has a handful of marks, every page host
/// filters the list itself, and a `Vec` is what both localStorage and the
/// `<For>` in the mark layer want.
#[derive(Clone, Copy, Default)]
pub struct GlossState {
    pub marks: RwSignal<Vec<GlossMark>>,
    /// Gloss multi-select mode (long-press initiated on a mark).
    pub selection_active: RwSignal<bool>,
    /// Ids of the marks currently selected while in multi-select mode.
    pub selected_marks: RwSignal<std::collections::HashSet<String>>,
    /// Id of the mark whose "processing" highlighter animation is live, if
    /// any. Lives here, not in the popover, because the animation is painted
    /// by the in-page mark layer: while the model works there is NO surface at
    /// all, so the stroke itself carries the thinking state.
    pub processing_id: RwSignal<Option<String>>,
    /// The reflowable spots this pane's marks carry, parsed once per context
    /// string (see `crate::components::ai::reflow_anchor::parse_spot`).
    /// PER PANE, in the pane's arena: it dies with the pane's owner, so one
    /// pane's dispose can never clear another pane's memo, and nothing has
    /// to remember to forget it.
    pub spots: SpotMemo,
}

impl GlossState {
    /// Clear every field to its resting state. Runs on document close and as
    /// the first step of an open. Destructured with no rest, so a field added
    /// to the struct cannot be silently forgotten by either path.
    ///
    /// The handles are `Copy`, so this binds the signals the struct already
    /// holds; `Self::default()` would allocate a fresh arena node per field on
    /// every close and leak them.
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
        // The memo is keyed by content, so a stale entry is never WRONG —
        // but the closed document's contexts are memory the next one would
        // carry without reading.
        spots.clear();
    }
}

/// Parsed spots, memoized by the exact context string they came from.
///
/// Keying on content is what makes a memo safe here: a `context` is
/// write-once — the envelope is produced at capture and nothing edits it in
/// place — so the same string always parses to the same spot, and a
/// replaced context simply arrives under a different key. `None` is cached
/// too: a legacy or malformed context is exactly as stable as a good one.
#[derive(Clone, Copy)]
pub struct SpotMemo(StoredValue<HashMap<String, Option<ReflowSpot>>, LocalStorage>);

impl Default for SpotMemo {
    fn default() -> Self {
        Self(StoredValue::new_local(HashMap::new()))
    }
}

impl SpotMemo {
    /// The memo only bounds a long session across many documents: past this
    /// it is cleared whole, which keeps the hot path free of bookkeeping.
    const CAP: usize = 512;

    /// The memoized answer for `context`: `Some(spot_or_none)` on a hit,
    /// `None` on a miss (or once the pane's arena is gone).
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
