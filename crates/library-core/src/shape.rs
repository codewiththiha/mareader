//! The shelf shape as a tree: a per-rung answer, inherited downward.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::folder::key_chain;

/// Per-rung shelf-shape answers for one folder's tree.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShapeTree {
    /// Rung path to its answer; inheriting rungs store nothing.
    #[serde(default)]
    overrides: BTreeMap<String, bool>,
}

impl ShapeTree {
    pub fn new() -> Self {
        Self::default()
    }

    /// The deepest answer on `key`'s chain, `None` when the chain inherits.
    pub fn at(&self, key: &str) -> Option<bool> {
        key_chain(key)
            .into_iter()
            .rev()
            .find_map(|rung| self.overrides.get(rung).copied())
    }

    /// Record one rung's answer, until a deeper rung answers.
    pub fn set(&mut self, key: &str, grouped: bool) {
        self.overrides.insert(key.to_string(), grouped);
    }

    /// Drop every answer inside `zone`.
    pub fn prune_zone(&mut self, zone: &str) {
        self.overrides
            .retain(|key, _| !crate::folder::key_in_zone(key, zone));
    }

    /// Whether the tree stores no answer of its own.
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
