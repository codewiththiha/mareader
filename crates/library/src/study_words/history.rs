//! Recall sessions, joined by their unique logical stamps.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::clock::Stamp;

/// An answer the judge accepted for grading.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RecallAnswer {
    pub text: String,
    pub verdict: String,
    pub feedback: String,
}

/// A completed recall; the key in the history identifies it across devices.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RecallSession {
    pub when: u64,
    pub answers: Vec<RecallAnswer>,
}

/// Append-only sessions and the latest book-erasure tombstone.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecallHistory {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cleared: Option<Stamp>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub sessions: BTreeMap<Stamp, RecallSession>,
}

impl RecallHistory {
    pub fn is_empty(&self) -> bool {
        self.cleared.is_none() && self.sessions.is_empty()
    }

    pub(super) fn add(&mut self, at: Stamp, when: u64, answers: Vec<RecallAnswer>) {
        if !answers.is_empty() && self.cleared.is_none_or(|clear| at > clear) {
            self.sessions.insert(at, RecallSession { when, answers });
        }
    }

    pub(super) fn clear(&mut self, at: Stamp) {
        self.cleared = Some(self.cleared.map_or(at, |old| old.max(at)));
        self.prune();
    }

    pub(super) fn merge(&mut self, other: Self) {
        self.cleared = self.cleared.max(other.cleared);
        for (at, run) in other.sessions {
            self.sessions
                .entry(at)
                .and_modify(|mine| *mine = mine.clone().max(run.clone()))
                .or_insert(run);
        }
        self.prune();
    }

    fn prune(&mut self) {
        if let Some(clear) = self.cleared {
            self.sessions.retain(|at, _| *at > clear);
        }
    }

    pub(super) fn latest(&self) -> Option<Stamp> {
        self.cleared.max(self.sessions.keys().next_back().copied())
    }
}
