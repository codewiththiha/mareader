//! This session's answers, keyed by mark id.

use std::collections::HashMap;
use std::sync::Arc;

use ai_core::gloss::GlossMark;
use ai_core::types::WordInfo;
use leptos::prelude::*;

/// Answers already fetched this session, keyed by mark id. Re-opening a
/// stroke is recall, not a rescan.
#[derive(Clone, Copy)]
pub struct GlossCache {
    /// Answers behind an `Arc`: recalling one is a refcount bump, not a copy
    /// of its prose, and the card, the measure twin and the cache all read the
    /// one allocation.
    ///
    /// Deliberately a `StoredValue`, not a signal: `update_value` notifies
    /// nobody, and nothing should re-render because a session answer was
    /// recorded. The writes that matter are the ones to `content.word_info`.
    answers: StoredValue<HashMap<String, Arc<WordInfo>>, LocalStorage>,
}

impl GlossCache {
    pub(super) fn new() -> Self {
        Self {
            answers: StoredValue::new_local(HashMap::new()),
        }
    }

    /// The cached answer for a mark id, if this session already fetched it.
    pub fn get(&self, id: &str) -> Option<Arc<WordInfo>> {
        self.answers.with_value(|c| c.get(id).cloned())
    }

    /// Record a finished answer.
    pub fn insert(&self, id: String, info: Arc<WordInfo>) {
        self.answers.update_value(|c| {
            c.insert(id, info);
        });
    }

    /// Drop one answer — a failed run must not leave a stale partial
    /// snapshot behind for the mark's next open to recall.
    pub fn remove(&self, id: &str) {
        self.answers.update_value(|c| {
            c.remove(id);
        });
    }

    /// Evict the answers of removed marks, so re-opening them re-requests
    /// instead of recalling an answer for a highlight that no longer exists.
    pub fn evict(&self, marks: &[GlossMark]) {
        self.answers.update_value(|c| {
            for m in marks {
                c.remove(&m.id);
            }
        });
    }
}
