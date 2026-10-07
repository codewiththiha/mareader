//! This session's answers, keyed by mark id.

use std::collections::HashMap;
use std::sync::Arc;

use ai_core::gloss::GlossMark;
use ai_core::types::WordInfo;
use leptos::prelude::*;

/// Answers fetched this session, keyed by mark id.
#[derive(Clone, Copy)]
pub struct GlossCache {
    /// Behind an `Arc`, so recall is a refcount bump.
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

    /// Drop one answer; a failure must not be recalled.
    pub fn remove(&self, id: &str) {
        self.answers.update_value(|c| {
            c.remove(id);
        });
    }

    /// Evict removed marks' answers so they re-request.
    pub fn evict(&self, marks: &[GlossMark]) {
        self.answers.update_value(|c| {
            for m in marks {
                c.remove(&m.id);
            }
        });
    }
}
