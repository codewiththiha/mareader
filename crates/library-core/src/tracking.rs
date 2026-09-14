//! Tracking as a tree, not a bool.
//!
//! A watched folder used to carry one root-level flag for the whole tree it
//! imported, so no smaller unit could hold a tracking decision. The tree keeps
//! what the flag could not: a folder tracked at the root with one subfolder
//! turned off, or the reverse.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::folder::key_chain;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Track {
    /// No opinion of its own: inherit the nearest ancestor that has one, and track
    /// nothing when no ancestor does.
    Inherit,
    On,
    /// Do not track this rung or, absent a deeper override, anything below it — even under an ancestor that is tracked.
    Off,
}

/// Per-rung tracking decisions for one watched folder's tree, keyed by rung path
/// exactly like [`crate::folder::WatchedFolder::shelf_map`]. Only explicit
/// `On`/`Off` decisions are stored — an absent rung is an [`Track::Inherit`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackingTree {
    /// Rung path to the decision that rung makes for itself and its subtree.
    /// `Inherit` is never stored, and a blob from before tracking existed has no key
    /// at all.
    #[serde(default)]
    overrides: BTreeMap<String, Track>,
}

impl TrackingTree {
    pub fn new() -> Self {
        Self::default()
    }

    /// A tree that tracks from its root down — the drop-in replacement for the old root-level `watch: true`.
    pub fn tracking_root() -> Self {
        let mut tree = Self::default();
        tree.set("", Track::On);
        tree
    }

    /// Whether `key` is tracked: the first explicit `On`/`Off` walking from this rung
    /// up to the root, `false` when the chain carries none. The deepest decision wins
    /// — a subfolder turned off under a tracked root is off, and one turned back on
    /// under that is on again.
    pub fn resolve(&self, key: &str) -> bool {
        key_chain(key)
            .into_iter()
            .rev()
            .find_map(|rung| match self.overrides.get(rung) {
                Some(Track::On) => Some(true),
                Some(Track::Off) => Some(false),
                // An `Inherit` entry is never stored, but a hand-edited blob could carry one.
                Some(Track::Inherit) | None => None,
            })
            .unwrap_or(false)
    }

    /// The whole tree's own answer: `resolve("")`, the drop-in for the old root-level `watch`.
    pub fn tracked(&self) -> bool {
        self.resolve("")
    }

    /// Whether ANY rung carries an explicit [`Track::On`] — the question a focus
    /// rescan asks, which is not the root's question. A tree whose root the reader
    /// turned off while one subfolder stayed on still owes a walk.
    pub fn any_on(&self) -> bool {
        self.overrides.values().any(|track| *track == Track::On)
    }

    /// Record a decision for one rung. [`Track::Inherit`] removes the override, so
    /// the rung falls back to whatever its ancestors say.
    pub fn set(&mut self, key: &str, track: Track) {
        if track == Track::Inherit {
            self.overrides.remove(key);
        } else {
            self.overrides.insert(key.to_string(), track);
        }
    }

    /// The root's decision made as the WHOLE tree's: every rung's override goes with
    /// it, and the tree answers uniformly from the root again. A reader who turns the
    /// tree back on is not asking which subfolders a previous hand turned off.
    pub fn set_root(&mut self, on: bool) {
        self.overrides.clear();
        self.set("", if on { Track::On } else { Track::Off });
    }

    /// The explicit decision a rung carries, or [`Track::Inherit`] when it has none
    /// of its own — the state a context-menu toggle shows, as distinct from the
    /// effective [`resolve`] it inherits.
    pub fn track_at(&self, key: &str) -> Track {
        self.overrides.get(key).copied().unwrap_or(Track::Inherit)
    }

    /// Drop every decision in `zone`: the zone's own rung and the whole subtree below
    /// it, the zone arithmetic [`crate::folder::key_in_zone`] gives a shelf map.
    pub fn prune_zone(&mut self, zone: &str) {
        self.overrides
            .retain(|key, _| !crate::folder::key_in_zone(key, zone));
    }

    pub fn is_empty(&self) -> bool {
        self.overrides.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_tree_tracks_nothing() {
        let tree = TrackingTree::new();
        assert!(tree.is_empty());
        assert!(!tree.tracked());
        assert!(!tree.resolve(""));
        assert!(!tree.resolve("Fiction"));
        assert!(!tree.resolve("Fiction/SciFi"));
        assert_eq!(tree.track_at(""), Track::Inherit);
    }

    #[test]
    fn a_tracked_root_is_the_old_flag_and_covers_the_whole_tree() {
        let tree = TrackingTree::tracking_root();
        assert!(tree.tracked(), "resolve(\"\") is the root-level watch");
        assert!(tree.resolve(""), "and the root itself");
        assert!(tree.resolve("Fiction"), "a rung inherits it");
        assert!(tree.resolve("Fiction/SciFi/deep"), "however deep");
        assert_eq!(tree.track_at(""), Track::On);
        assert_eq!(tree.track_at("Fiction"), Track::Inherit, "inherited, not set");
    }

    #[test]
    fn a_subfolder_turned_off_under_a_tracked_root_is_off() {
        let mut tree = TrackingTree::tracking_root();
        tree.set("Fiction", Track::Off);
        assert!(tree.resolve(""), "the root still tracks");
        assert!(tree.resolve("Poetry"), "a sibling rung is untouched");
        assert!(!tree.resolve("Fiction"), "the rung turned off is off");
        assert!(!tree.resolve("Fiction/SciFi"), "and so is everything below it");
        assert_eq!(tree.track_at("Fiction"), Track::Off);
    }

    #[test]
    fn a_subfolder_turned_back_on_under_an_off_rung_is_on() {
        let mut tree = TrackingTree::tracking_root();
        tree.set("Fiction", Track::Off);
        tree.set("Fiction/SciFi", Track::On);
        assert!(!tree.resolve("Fiction"), "the rung stays off");
        assert!(!tree.resolve("Fiction/Crime"), "an un-set sibling stays off");
        assert!(tree.resolve("Fiction/SciFi"), "the deeper override wins");
        assert!(tree.resolve("Fiction/SciFi/Hard"), "and its subtree follows");
    }

    #[test]
    fn the_deepest_explicit_decision_wins() {
        // Off at the root, On at a rung, Off below it: each rung reads the
        // closest ancestor that has an opinion.
        let mut tree = TrackingTree::new();
        tree.set("", Track::Off);
        tree.set("a", Track::On);
        tree.set("a/b", Track::Off);
        assert!(!tree.resolve(""));
        assert!(!tree.resolve("z"), "a sibling of `a` inherits the root's Off");
        assert!(tree.resolve("a"));
        assert!(tree.resolve("a/x"), "below `a`, above `a/b`");
        assert!(!tree.resolve("a/b"));
        assert!(!tree.resolve("a/b/c"), "below the Off");
    }

    #[test]
    fn setting_inherit_removes_the_override() {
        let mut tree = TrackingTree::tracking_root();
        tree.set("Fiction", Track::Off);
        assert!(!tree.resolve("Fiction"));
        tree.set("Fiction", Track::Inherit);
        assert!(tree.resolve("Fiction"), "inherits the root's On again");
        assert_eq!(tree.track_at("Fiction"), Track::Inherit);
        assert!(!tree.is_empty(), "the root override is still there");
    }

    #[test]
    fn a_lone_off_rung_under_an_untracked_root_tracks_nothing_else() {
        let mut tree = TrackingTree::new();
        tree.set("Fiction", Track::Off);
        assert!(!tree.tracked());
        assert!(!tree.resolve("Fiction"));
        tree.set("Poetry", Track::On);
        assert!(!tree.tracked(), "the root is still undecided");
        assert!(tree.resolve("Poetry"), "but the one rung turned on tracks");
        assert!(tree.resolve("Poetry/Sonnets"));
    }

    #[test]
    fn pruning_a_zone_takes_the_rung_and_its_whole_subtree() {
        let mut tree = TrackingTree::tracking_root();
        tree.set("Fiction", Track::Off);
        tree.set("Fiction/SciFi", Track::On);
        tree.set("Poetry", Track::Off);
        tree.prune_zone("Fiction");
        assert_eq!(tree.track_at("Fiction"), Track::Inherit);
        assert_eq!(tree.track_at("Fiction/SciFi"), Track::Inherit);
        assert_eq!(tree.track_at("Poetry"), Track::Off, "a sibling is untouched");
        assert!(tree.tracked(), "and the root still tracks");
        tree.prune_zone("");
        assert!(tree.is_empty());
    }

    #[test]
    fn the_tree_survives_a_round_trip_through_storage() {
        let mut tree = TrackingTree::tracking_root();
        tree.set("Fiction", Track::Off);
        tree.set("Fiction/SciFi", Track::On);
        let json = serde_json::to_string(&tree).unwrap();
        assert!(json.contains("\"overrides\""), "{json}");
        assert!(json.contains("\"off\""), "{json}");
        let back: TrackingTree = serde_json::from_str(&json).unwrap();
        assert_eq!(back, tree);
        assert!(back.resolve("Fiction/SciFi"));
        assert!(!back.resolve("Fiction"));
    }

    #[test]
    fn a_blob_from_before_tracking_existed_loads_an_empty_tree() {
        // No `overrides` key is an empty tree, which tracks nothing — the
        // migration that seeds it from the old root flag is the loader's job.
        let tree: TrackingTree = serde_json::from_str("{}").unwrap();
        assert!(tree.is_empty());
        assert!(!tree.tracked());
    }

    #[test]
    fn any_on_is_the_walk_question_the_root_alone_cannot_answer() {
        let empty = TrackingTree::new();
        assert!(!empty.any_on());
        let root = TrackingTree::tracking_root();
        assert!(root.any_on(), "the root's own On is an On somewhere");
        let mut off_root = TrackingTree::new();
        off_root.set("", Track::Off);
        assert!(!off_root.any_on(), "an Off nowhere is an Off everywhere");
        off_root.set("Fiction", Track::On);
        assert!(
            off_root.any_on(),
            "one rung kept on is a folder the rescan still owes a walk"
        );
        assert!(!off_root.tracked(), "and the root's own answer is still Off");
    }

    #[test]
    fn the_root_decision_is_the_whole_tree_s() {
        // "The whole folder" is a sentence about every rung in it: a root turned back
        // on activates every rung again, including one a previous hand decided
        // separately.
        let mut tree = TrackingTree::tracking_root();
        tree.set("Fiction", Track::Off);
        tree.set("Poetry", Track::On);
        tree.set_root(false);
        assert!(!tree.resolve(""));
        assert!(!tree.resolve("Fiction"), "the rung's own Off went with it");
        assert!(!tree.resolve("Poetry"), "and so did its On");
        assert!(!tree.any_on(), "no rung is left watching");
        tree.set_root(true);
        assert!(tree.resolve(""), "the root watches");
        assert!(tree.resolve("Fiction"), "every rung inherits it again");
        assert!(tree.resolve("Poetry/deep"), "however deep");
        assert_eq!(
            tree.track_at("Fiction"),
            Track::Inherit,
            "the overrides are gone, not overruled"
        );
    }
}
