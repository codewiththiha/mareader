//! The shelf shape as a tree, not a bool.
//!
//! A single root-level flag could not answer for a subtree: re-importing one
//! nested folder asks for a shelf per folder from that folder down while the
//! rest of the tree stays on its root rung. The tree stores a per-rung answer
//! and lets every other rung inherit the nearest one above it.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::folder::key_chain;

/// Per-rung shelf-shape answers for one watched folder's tree, keyed by rung
/// path like [`crate::folder::WatchedFolder::shelf_map`]. Only explicit answers
/// are stored; an absent rung inherits the nearest ancestor that answered, and
/// [`crate::folder::FolderOpts::groups`] when none did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShapeTree {
    /// Rung path to the answer for that rung and its subtree. Inheriting rungs
    /// store nothing, and blobs from before the shape was a tree have no key.
    #[serde(default)]
    overrides: BTreeMap<String, bool>,
}

impl ShapeTree {
    pub fn new() -> Self {
        Self::default()
    }

    /// The deepest answer on `key`'s chain: `Some(true)` cuts a shelf per
    /// folder from this rung down, `Some(false)` keeps the subtree on one
    /// shelf, `None` when the whole chain inherits. A subfolder answered
    /// against a one-shelf root wins for its own subtree.
    pub fn at(&self, key: &str) -> Option<bool> {
        key_chain(key)
            .into_iter()
            .rev()
            .find_map(|rung| self.overrides.get(rung).copied())
    }

    /// Record the answer one rung gives for itself and its subtree, until a
    /// deeper rung answers for itself.
    pub fn set(&mut self, key: &str, grouped: bool) {
        self.overrides.insert(key.to_string(), grouped);
    }

    /// Drop every answer inside `zone` (same zone arithmetic as
    /// [`crate::folder::key_in_zone`]). Answers above the zone stand, so the
    /// subtree falls back to inheriting them.
    pub fn prune_zone(&mut self, zone: &str) {
        self.overrides
            .retain(|key, _| !crate::folder::key_in_zone(key, zone));
    }

    /// Whether the tree stores no answer of its own, so every rung takes the
    /// answer its folder was imported with.
    pub fn is_empty(&self) -> bool {
        self.overrides.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tree_no_one_answered_for_hands_every_rung_to_the_folder() {
        let tree = ShapeTree::new();
        assert!(tree.is_empty());
        assert_eq!(tree.at(""), None);
        assert_eq!(tree.at("Fiction"), None);
        assert_eq!(tree.at("Fiction/SciFi"), None);
    }

    #[test]
    fn a_rung_answers_for_itself_and_the_subtree_below_it() {
        let mut tree = ShapeTree::new();
        tree.set("Fiction", true);
        assert_eq!(tree.at("Fiction"), Some(true));
        assert_eq!(tree.at("Fiction/SciFi"), Some(true), "a rung inherits it");
        assert_eq!(tree.at("Fiction/SciFi/deep"), Some(true), "however deep");
        assert_eq!(
            tree.at("Reference"),
            None,
            "and a sibling keeps its own answer"
        );
    }

    #[test]
    fn the_deepest_answer_wins() {
        let mut tree = ShapeTree::new();
        tree.set("Fiction", true);
        tree.set("Fiction/SciFi", false);
        assert_eq!(tree.at("Fiction"), Some(true));
        assert_eq!(tree.at("Fiction/SciFi"), Some(false));
        assert_eq!(tree.at("Fiction/SciFi/deep"), Some(false));
        assert_eq!(tree.at("Fiction/History"), Some(true));
    }

    #[test]
    fn pruning_a_zone_keeps_the_answers_above_it() {
        let mut tree = ShapeTree::new();
        tree.set("Fiction", true);
        tree.set("Fiction/SciFi", false);
        tree.set("Reference", true);
        tree.prune_zone("Fiction");
        assert_eq!(tree.at("Fiction"), None);
        assert_eq!(tree.at("Fiction/SciFi"), None);
        assert_eq!(tree.at("Reference"), Some(true));
    }
}
