//! Tracking as a tree, not a bool.
//!
//! A watched folder used to carry one root-level flag — `FolderOpts::watch` —
//! for the WHOLE tree it imported. Every question about "is this bit of the
//! tree tracked" then had exactly one unit to ask it of: the root. There was no
//! data structure smaller than "the whole imported tree" that could hold a
//! tracking decision, so there was no code path that could ever offer
//! subfolder-level control — turning tracking off for one subfolder of a watched
//! import, or on for a new import under an already-tracked ancestor, was not a
//! missing `if` but a missing field.
//!
//! This is the field. A [`TrackingTree`] is a set of per-rung overrides keyed by
//! the same rung paths a folder's `shelf_map` uses (`""` is the root itself),
//! and [`TrackingTree::resolve`] walks a rung's chain up to the root and takes
//! the first explicit decision it finds. The old root-level flag is exactly
//! `resolve("")`, so a tree with one `set("", Track::On)` behaves as the bool
//! did — a one-line migration rather than a rewrite of every call site — while a
//! deeper `set` is the subfolder control the bool could not express.
//!
//! Pure and persisted: no I/O, no signals, just a map and the inheritance rule,
//! so the decisions that are easy to get wrong (which override wins, what an
//! unset rung inherits) are host-testable rather than something you discover by
//! pointing the app at a real folder.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::folder::key_chain;

/// What a rung says about tracking, including saying nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Track {
    /// No opinion of its own: inherit the nearest ancestor that has one, and
    /// track nothing when no ancestor does. The default, and what a rung the
    /// reader never touched carries.
    Inherit,
    /// Track this rung and, absent a deeper override, everything below it.
    On,
    /// Do not track this rung or, absent a deeper override, anything below it —
    /// even under an ancestor that is tracked.
    Off,
}

/// Per-rung tracking decisions for one watched folder's tree.
///
/// Keyed by rung path exactly like [`crate::folder::WatchedFolder::shelf_map`]:
/// `""` is the root, `"Fiction"` a rung below it, `"Fiction/SciFi"` deeper still.
/// Only explicit `On`/`Off` decisions are stored — setting a rung back to
/// [`Track::Inherit`] removes its entry — so the map holds the overrides and
/// nothing else, and an absent rung is an inherited one.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TrackingTree {
    /// Rung path to the decision that rung makes for itself and its subtree.
    /// `Inherit` is never stored: it is the absence of an entry. A blob from
    /// before tracking existed has no key at all, which is an empty tree.
    #[serde(default)]
    overrides: BTreeMap<String, Track>,
}

impl TrackingTree {
    /// An empty tree: no rung tracks, because nothing has been decided.
    pub fn new() -> Self {
        Self::default()
    }

    /// A tree that tracks from its root down — the drop-in for the old
    /// root-level `watch: true`, and what an import that asks to be watched
    /// starts from.
    pub fn tracking_root() -> Self {
        let mut tree = Self::default();
        tree.set("", Track::On);
        tree
    }

    /// Whether `key` is tracked: walk from this rung up to the root and take the
    /// first explicit `On`/`Off`; no override anywhere on the chain is `false`.
    ///
    /// The deepest decision wins because it is the closest one to the rung being
    /// asked about — a subfolder turned off under a tracked root is off, and a
    /// subfolder turned back on under that is on again. [`crate::folder::key_chain`]
    /// owns the walk so this and the shelf chain cannot disagree about a rung's
    /// ancestors.
    pub fn resolve(&self, key: &str) -> bool {
        key_chain(key)
            .into_iter()
            .rev()
            .find_map(|rung| match self.overrides.get(rung) {
                Some(Track::On) => Some(true),
                Some(Track::Off) => Some(false),
                // An `Inherit` entry is never stored, but a hand-edited blob
                // could carry one; read it as the absence it means.
                Some(Track::Inherit) | None => None,
            })
            .unwrap_or(false)
    }

    /// The whole tree's own flag: `resolve("")`, the drop-in replacement for the
    /// old root-level `watch`.
    pub fn tracked(&self) -> bool {
        self.resolve("")
    }

    /// Whether ANY rung carries an explicit [`Track::On`]: the question a focus
    /// rescan asks of a folder — "is there anything here to watch" — which is
    /// not the root's question. A tree whose root the reader turned off while
    /// one subfolder stayed on still owes a walk, and the walk's own per-rung
    /// gate (`crate::ledger::decide`) is what keeps the off rungs quiet.
    pub fn any_on(&self) -> bool {
        self.overrides.values().any(|track| *track == Track::On)
    }

    /// Record a decision for one rung. [`Track::Inherit`] removes the override,
    /// so the rung falls back to whatever its ancestors say — there is no stored
    /// "inherit", only the absence of a decision.
    pub fn set(&mut self, key: &str, track: Track) {
        if track == Track::Inherit {
            self.overrides.remove(key);
        } else {
            self.overrides.insert(key.to_string(), track);
        }
    }

    /// The root's decision made as the WHOLE tree's: every rung's override goes
    /// with it, and the tree answers uniformly from the root again.
    ///
    /// What the shelf menu's root row means by "the whole folder" — and a
    /// sentence, not a default. A reader who turns the tree back on is not
    /// asking which subfolders a previous hand turned off, and a tree that
    /// kept them would be a toggle that visibly did nothing below the rung
    /// they stand on; a reader who turns it off is not leaving one subfolder
    /// secretly watching after a row that said the whole folder stopped.
    pub fn set_root(&mut self, on: bool) {
        self.overrides.clear();
        self.set("", if on { Track::On } else { Track::Off });
    }

    /// The explicit decision a rung carries, or [`Track::Inherit`] when it has
    /// none of its own. What a context-menu toggle reads to show the state it is
    /// about to flip, as distinct from the effective [`resolve`] it inherits.
    pub fn track_at(&self, key: &str) -> Track {
        self.overrides.get(key).copied().unwrap_or(Track::Inherit)
    }

    /// Drop every decision in `zone` — the zone's own rung and the whole subtree
    /// below it. A rung's departure takes the rungs it governs with it, the same
    /// zone arithmetic [`crate::folder::key_in_zone`] gives a shelf map: a
    /// tracking override on a rung that is gone is a decision nothing can reach.
    pub fn prune_zone(&mut self, zone: &str) {
        self.overrides
            .retain(|key, _| !crate::folder::key_in_zone(key, zone));
    }

    /// Whether any rung carries a decision at all. An empty tree is the one a
    /// folder that has never been asked about tracking holds.
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
        // Clearing the override hands the rung back to its ancestor.
        tree.set("Fiction", Track::Inherit);
        assert!(tree.resolve("Fiction"), "inherits the root's On again");
        assert_eq!(tree.track_at("Fiction"), Track::Inherit);
        assert!(!tree.is_empty(), "the root override is still there");
    }

    #[test]
    fn a_lone_off_rung_under_an_untracked_root_tracks_nothing_else() {
        // No root decision at all: an Off override is redundant but harmless,
        // and an On override is the only thing that tracks.
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
        // A departure of the "Fiction" rung: its own decision and everything
        // below it go, the root and a sibling stay.
        tree.prune_zone("Fiction");
        assert_eq!(tree.track_at("Fiction"), Track::Inherit);
        assert_eq!(tree.track_at("Fiction/SciFi"), Track::Inherit);
        assert_eq!(tree.track_at("Poetry"), Track::Off, "a sibling is untouched");
        assert!(tree.tracked(), "and the root still tracks");
        // Pruning the root zone empties the whole tree.
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
        // A tree the reader turned off at the root while one subfolder stayed
        // on still owes a walk, and a tree of nothing but Offs does not.
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
        // "The whole folder" is a sentence about every rung in it: a root
        // turned off leaves no subfolder secretly watching, and a root turned
        // back on activates every rung again — the subfolder a previous hand
        // decided separately is not a decision the whole-tree row keeps.
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
