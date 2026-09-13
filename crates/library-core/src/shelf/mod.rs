//! Shelves: an ordered list of book ids, and the two kinds that produce one.
//!
//! A shelf holds membership and nothing else — no copies, no paths, no
//! filesystem intent. That is what makes dragging a book between shelves safe
//! by construction: the only thing a drop can change is an ordered list of
//! ids, so a read-in-place book can be filed anywhere in the app without the
//! file it points at ever being touched.
//!
//! "All" is deliberately not a shelf. Every book is in it by definition, so
//! storing it would be a second copy of the book list that has to be kept in
//! sync forever; [`ALL_SHELF`] is the id the UI uses for that pseudo-shelf and
//! [`find`] answers `None` for it, which is how a caller tells the two
//! apart.
//!
//! ## Nesting
//!
//! [`Shelf::parent`] makes the shelves a forest rather than a list: the root
//! level is the shelves whose parent is `None`, and a level inside a shelf is
//! [`children_of`] on that shelf's id. Nesting is the one relationship in this
//! module that can be wrong in a way no single row shows — a shelf filed inside
//! itself, or inside one of its own children, is a folder that renders nowhere
//! and can never be opened again. So the graph is guarded twice: [`can_nest`]
//! refuses the drop before it is written, and [`sanitize`] cuts any cycle a
//! hand-edited blob carries, because a rule only enforced on the way in is a
//! rule one restored backup can break.
//!
//! Nesting is NOT the same thing as [`ShelfKind::Folder`]'s `rel`. `rel` is a
//! subfolder's address inside a watched directory's tree — a rescan key, owned
//! by the filesystem. For a folder shelf `parent` starts as its projection —
//! the tree on disk is the tree on the shelf, and a scan re-hangs the folder's
//! shelves on the rungs their `rel` names. For a virtual shelf `parent` is the
//! reader's from the start, and no scan ever writes it.
//!
//! ## The hand and the disk
//!
//! What a hand-move of a folder shelf MEANS depends on how its folder is read,
//! and [`departs_on_move`] is the one answer. A shelf of a COPYING folder is
//! the library's own already: the move is the reader's, [`reparent`] takes it
//! and marks the row [`Shelf::manual_parent`], and the next re-hang passes it
//! by. A shelf of a READ-AT-PLACE folder is the OS directory itself — the way
//! a linked book is the OS file itself — so a hand cannot take it off the rung
//! the tree names for it: the move is a DEPARTURE, the shelf's own spelling of
//! what a linked book leaving its rung does, and it leaves as a copy the
//! library owns, asked before the copy is made. The original stays on disk and
//! comes back to the tree on the next import, lit, exactly as a departed
//! book's file does. The departure itself — the ask, the copies, the ledger's
//! zone going free — is the library services' business; what this crate owns
//! is the rule of which moves are departures, the seat a hand-move is measured
//! against, and the subtree that rides with the shelf the hand named.

use serde::{Deserialize, Serialize};

mod family;
mod members;
mod tree;

pub use family::{departing_moves, departs_on_move, family_for};
pub use members::{containing, forget, forget_everywhere, members_of, place, shelf_add};
pub use tree::{
    ancestors, can_nest, children_of, lift_children, reparent, rehang_moves, subtree_ids,
};

/// The pseudo-shelf holding every book: the library's root, and the order the
/// persisted book list is in. Not a [`Shelf`] — see the module docs.
pub const ALL_SHELF: &str = "all";

/// What produced a shelf, which decides whether the UI offers it a folder
/// glyph, a watch dot, or a rename.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ShelfKind {
    /// Created by the reader (or by an import of loose files). Pure
    /// membership: nothing on disk corresponds to it. The default, because a
    /// shelf a blob does not describe is one the reader made.
    #[default]
    Virtual,
    /// Cut from a watched folder's tree. `rel` is the subfolder within
    /// [`crate::folder::WatchedFolder::root`], `None` for the root itself —
    /// which is what a rescan matches a found file's
    /// [`crate::scan::FoundFile::subfolder`] against.
    Folder {
        #[serde(rename = "folderId")]
        folder_id: String,
        #[serde(default)]
        rel: Option<String>,
    },
}

impl ShelfKind {
    /// The watched folder this shelf belongs to, if it belongs to one.
    pub fn folder_id(&self) -> Option<&str> {
        match self {
            ShelfKind::Virtual => None,
            ShelfKind::Folder { folder_id, .. } => Some(folder_id),
        }
    }

    /// True for the shelf a watched folder's root files onto.
    pub fn is_folder_root(&self) -> bool {
        matches!(self, ShelfKind::Folder { rel: None, .. })
    }

    /// The rung this shelf stands on, as the folder's own map keys one: the
    /// empty string for the shelf at a watched root, and the empty string for a
    /// shelf the reader made, which no directory names.
    ///
    /// One spelling of the question "which rung of its tree is this", for the
    /// callers that ask it of a standing shelf rather than of a ledger — the
    /// move's seat check ([`crate::shelf::departs_on_move`]) among them. The
    /// two empty answers are the same answer on purpose: a reader's shelf is a
    /// seat for nothing below it, and a folder's root shelf is the seat every
    /// rung of that folder's tree hangs under.
    pub fn rung(&self) -> &str {
        match self {
            ShelfKind::Virtual => "",
            ShelfKind::Folder { rel, .. } => rel.as_deref().unwrap_or(""),
        }
    }
}

/// One shelf.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Shelf {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub kind: ShelfKind,
    /// Member book ids, in the order the reader arranged them. The order is
    /// the point: a shelf is a list, not a set, and a drop writes an index.
    #[serde(default)]
    pub books: Vec<String>,
    /// The shelf this one is filed inside, or `None` at the library's root.
    ///
    /// `#[serde(default)]` because a blob written before shelves could nest has
    /// no `parent` key at all, and every shelf in it is a root shelf — which is
    /// the right reading of it rather than a migration.
    #[serde(default)]
    pub parent: Option<String>,
    /// The reader moved this shelf by hand off the seat its folder's own
    /// shelves name for it, so its place in the library is the reader's and
    /// not the disk's: a watched folder's rescan re-hangs the shelves it owns
    /// on the rungs their `rel` names, and passes a shelf wearing this mark
    /// by. Written by [`reparent`] — which compares the seat and so CLEARS the
    /// mark when a hand puts the shelf back where the disk names — and born
    /// written on the rungs a merged import mints, whose tree hangs off a
    /// shelf the disk does not own. `#[serde(default)]` because a blob from
    /// before shelves could be hand-moved has no key and every shelf in it is
    /// the scan's.
    ///
    /// Only a shelf a hand may take wears it: a rung of a read-at-place folder
    /// departs as a copy instead of moving (see [`departs_on_move`]), so this
    /// is a copying folder's shelf, a merged tree's rung, or a shelf of an
    /// older blob from before the ask. Such a shelf keeps everything else it
    /// knows: its `rel` is still the rescan key, its folder's `shelf_map`
    /// still routes newly scanned files into it, and its books keep their
    /// read-in-place addresses — a moved shelf moves its whole subtree, the
    /// way a moved directory takes its tree with it.
    #[serde(default)]
    pub manual_parent: bool,
}

impl Shelf {
    /// True when this shelf was cut from a watched folder.
    pub fn is_folder(&self) -> bool {
        self.kind.folder_id().is_some()
    }
}

/// The shelf with this id, if there is one. `None` for [`ALL_SHELF`]: the
/// caller falls back to the whole book list.
pub fn find<'a>(shelves: &'a [Shelf], id: &str) -> Option<&'a Shelf> {
    if id == ALL_SHELF {
        return None;
    }
    shelves.iter().find(|s| s.id == id)
}

/// The same, for a write.
///
/// [`find`]'s answer about [`ALL_SHELF`] is the reason this exists rather than a
/// `iter_mut().find(|s| s.id == id)` at every call site: the pseudo-shelf is the
/// book list and has no member list, so a caller that hands a shelf id straight
/// from the route — which is `"all"` at the root — gets `None` and takes its
/// root-level branch instead of silently matching nothing and looking like a bug.
/// A hand-rolled lookup gets that answer too, but by accident and only while
/// [`sanitize`] keeps dropping a row that wears the id.
pub fn find_mut<'a>(shelves: &'a mut [Shelf], id: &str) -> Option<&'a mut Shelf> {
    if id == ALL_SHELF {
        return None;
    }
    shelves.iter_mut().find(|s| s.id == id)
}

/// Make a persisted shelf list internally valid: drop shelves with no id or no
/// name, drop a row wearing the [`ALL_SHELF`] id (the pseudo-shelf is the book
/// list and no level renders it), dedupe by id (first wins), drop members that
/// are blank, drop a duplicate member keeping its first position, and cut the
/// nesting graph back to a forest. Idempotent. Members that name a book the
/// library no longer has are NOT dropped here — that needs the book list, and
/// [`blob::sanitize`](crate::blob::sanitize) does it with both in hand.
pub fn sanitize(shelves: &mut Vec<Shelf>) {
    let mut seen = std::collections::HashSet::new();
    shelves.retain(|s| {
        !s.id.trim().is_empty() && !s.name.trim().is_empty() && s.id != ALL_SHELF
    });
    shelves.retain(|s| seen.insert(s.id.clone()));
    for s in shelves.iter_mut() {
        let mut members = std::collections::HashSet::new();
        s.books.retain(|m| !m.trim().is_empty() && members.insert(m.clone()));
    }

    // A parent that is the shelf itself, or that names no shelf, is a folder no
    // level renders: the row survives the load and vanishes from the page. Both
    // collapse to the root, which is where the reader can see it again.
    for s in shelves.iter_mut() {
        if s.parent.as_deref() == Some(s.id.as_str()) {
            s.parent = None;
        }
    }
    // Owned ids rather than borrowed ones: the pass below writes to the same list
    // it is reading the names out of, and a set of `&str` into it would hold the
    // borrow open across the write.
    let ids: std::collections::HashSet<String> =
        shelves.iter().map(|s| s.id.clone()).collect();
    for s in shelves.iter_mut() {
        if s.parent.as_deref().is_some_and(|p| !ids.contains(p)) {
            s.parent = None;
        }
    }

    // Then the cycles, which no single row shows. Only the shelves ON a loop are
    // cut: a shelf that merely leads into one keeps its parent, and becomes a
    // root shelf's child once the loop below it is open. Cutting on a walk that
    // fails to come back instead would empty a whole branch for one bad edge,
    // and would not be idempotent — the second pass would find nothing to cut.
    // One edge map rather than a `find` per hop of every walk: the walk per
    // shelf stays — it is the rule — but each hop is a lookup now, and the
    // ids come back owned so the map's borrow ends before the writes begin.
    let on_a_cycle: std::collections::HashSet<String> = {
        let parents: std::collections::HashMap<&str, Option<&str>> = shelves
            .iter()
            .map(|s| (s.id.as_str(), s.parent.as_deref()))
            .collect();
        let mut out = std::collections::HashSet::new();
        for s in shelves.iter() {
            let mut current = parents.get(s.id.as_str()).copied().flatten();
            for _ in 0..=shelves.len() {
                let Some(parent_id) = current else {
                    break;
                };
                if parent_id == s.id {
                    out.insert(s.id.clone());
                    break;
                }
                current = parents.get(parent_id).copied().flatten();
            }
        }
        out
    };
    for s in shelves.iter_mut() {
        if on_a_cycle.contains(&s.id) {
            s.parent = None;
        }
    }
}


#[cfg(test)]
mod tests {
    use crate::book::{Book, Row};

    fn book(id: &str, title: &str) -> Row {
        Row::Book(Book {
            title: Some(title.to_string()),
            added_ms: 1,
            ..crate::testkit::book(id)
        })
    }

    fn plain(id: &str, members: &[&str]) -> Shelf {
        crate::testkit::plain_shelf(id, members)
    }

    #[test]
    fn the_pseudo_shelf_is_not_a_shelf_to_either_lookup() {
        // `sanitize` drops a row wearing the id, so a hand-rolled
        // `iter().find(|s| s.id == id)` answers `None` too — but only while that
        // stays true. The lookups answer it as a rule.
        let mut shelves = vec![plain("s", &["b1"])];
        assert!(find(&shelves, ALL_SHELF).is_none());
        assert!(find_mut(&mut shelves, ALL_SHELF).is_none());
        assert_eq!(find(&shelves, "s").map(|s| s.name.as_str()), Some("s"));
        find_mut(&mut shelves, "s").unwrap().name = "renamed".into();
        assert_eq!(find(&shelves, "s").map(|s| s.name.as_str()), Some("renamed"));
        assert!(find_mut(&mut shelves, "gone").is_none());
    }

    #[test]
    fn a_level_s_members_are_its_shelf_s_or_the_unfiled_rows() {
        let rows = vec![
            book("b1", "Dune"),
            book("b2", "Apple"),
            Row::link("l1".into(), "Dune".into(), "b1".into(), 5),
        ];
        let shelves = vec![plain("s", &["b1", "l1"]), plain("t", &["b2"])];
        assert_eq!(members_of(&rows, &shelves, "s"), vec!["b1", "l1"]);
        assert_eq!(members_of(&rows, &shelves, "gone"), Vec::<&str>::new());
        // The root is a level with no shelf row, and its list is what is left.
        assert_eq!(members_of(&rows, &shelves, ALL_SHELF), Vec::<&str>::new());
        let one_filed = vec![plain("s", &["b1"])];
        assert_eq!(members_of(&rows, &one_filed, ALL_SHELF), vec!["b2", "l1"]);
    }

    use super::*;

    fn shelf(id: &str, name: &str, books: &[&str]) -> Shelf {
        crate::testkit::shelf(id, name, books, None)
    }

    /// One shelf filed inside another: the shape a nest produces.
    fn nested(id: &str, name: &str, parent: &str) -> Shelf {
        Shelf {
            parent: Some(parent.to_string()),
            ..shelf(id, name, &[])
        }
    }

    fn ids_of<'a>(shelves: &[&'a Shelf]) -> Vec<&'a str> {
        shelves.iter().map(|s| s.id.as_str()).collect()
    }

    fn ids(members: &[String]) -> Vec<&str> {
        members.iter().map(String::as_str).collect()
    }

    #[test]
    fn all_is_not_a_shelf_and_never_looks_like_one() {
        let shelves = vec![shelf("s1", "Sci-fi", &["a"])];
        assert!(find(&shelves, ALL_SHELF).is_none());
        assert!(find(&shelves, "s1").is_some());
        assert!(find(&shelves, "nope").is_none());
    }

    #[test]
    fn a_pseudo_all_shelf_never_survives_a_load() {
        // A blob that somehow carries an "all" row must not turn into a
        // bookshelf tile duplicating the whole library: the sanitizer drops
        // it, so no level ever renders it and no view has to remember to.
        let mut shelves = vec![shelf(ALL_SHELF, "All", &["a"]), shelf("s1", "Sci-fi", &["a"])];
        sanitize(&mut shelves);
        let ids: Vec<&str> = shelves.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(ids, vec!["s1"]);
    }

    #[test]
    fn a_drop_appends_by_default() {
        let mut m: Vec<String> = vec!["a".into(), "b".into()];
        place(&mut m, "c", None);
        assert_eq!(ids(&m), vec!["a", "b", "c"]);
    }

    #[test]
    fn a_drop_lands_at_the_index_pointed_at() {
        let mut m: Vec<String> = vec!["a".into(), "b".into(), "c".into()];
        place(&mut m, "d", Some(1));
        assert_eq!(ids(&m), vec!["a", "d", "b", "c"]);
        // Past the end clamps rather than panics: a drop on the gap after the
        // last card is an append.
        place(&mut m, "e", Some(99));
        assert_eq!(ids(&m), vec!["a", "d", "b", "c", "e"]);
    }

    #[test]
    fn moving_a_member_does_not_duplicate_or_shift_the_tail() {
        let mut m: Vec<String> = vec!["a".into(), "b".into(), "c".into()];
        place(&mut m, "c", Some(0));
        assert_eq!(ids(&m), vec!["c", "a", "b"]);
        place(&mut m, "a", Some(1));
        assert_eq!(ids(&m), vec!["c", "a", "b"], "a book stays where it is dropped");
        place(&mut m, "c", Some(3));
        assert_eq!(ids(&m), vec!["a", "b", "c"]);
    }

    #[test]
    fn filing_a_book_that_is_already_filed_moves_nothing() {
        // A restore and an "also show it here" both add a book that may already
        // be a member; appending it again would reshuffle a shelf the reader can
        // see, for an instruction that was not about position.
        let mut s = shelf("s1", "One", &["a", "b"]);
        shelf_add(&mut s, "b");
        assert_eq!(ids(&s.books), vec!["a", "b"]);
        shelf_add(&mut s, "c");
        assert_eq!(ids(&s.books), vec!["a", "b", "c"]);
        shelf_add(&mut s, "a");
        assert_eq!(ids(&s.books), vec!["a", "b", "c"]);
    }

    #[test]
    fn a_book_leaves_one_shelf_or_all_of_them() {
        let mut m: Vec<String> = vec!["a".into(), "b".into()];
        assert!(forget(&mut m, "a"));
        assert!(!forget(&mut m, "a"));
        assert_eq!(ids(&m), vec!["b"]);

        let mut shelves = vec![shelf("s1", "One", &["a", "b"]), shelf("s2", "Two", &["b"])];
        forget_everywhere(&mut shelves, "b");
        assert_eq!(ids(&shelves[0].books), vec!["a"]);
        assert!(shelves[1].books.is_empty());
    }

    #[test]
    fn the_shelves_a_book_is_on_are_found_in_order() {
        let shelves = vec![shelf("s1", "One", &["a", "b"]), shelf("s2", "Two", &["b"])];
        let names: Vec<&str> = containing(&shelves, "b").iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["One", "Two"]);
        assert!(containing(&shelves, "zzz").is_empty());
    }

    #[test]
    fn a_folder_shelf_knows_its_folder_and_its_subfolder() {
        let root = Shelf {
            kind: ShelfKind::Folder {
                folder_id: "f1".into(),
                rel: None,
            },
            ..shelf("s1", "Books", &[])
        };
        let sub = Shelf {
            kind: ShelfKind::Folder {
                folder_id: "f1".into(),
                rel: Some("scifi".into()),
            },
            ..shelf("s2", "scifi", &[])
        };
        assert!(root.is_folder() && sub.is_folder());
        assert!(root.kind.is_folder_root() && !sub.kind.is_folder_root());
        assert_eq!(sub.kind.folder_id(), Some("f1"));
        assert_eq!(shelf("s3", "Mine", &[]).kind.folder_id(), None);
    }

    #[test]
    fn a_folder_kind_persists_with_its_folder_id() {
        let s = Shelf {
            kind: ShelfKind::Folder {
                folder_id: "f1".into(),
                rel: Some("scifi".into()),
            },
            ..shelf("s2", "scifi", &["a"])
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"kind\":\"folder\""), "{json}");
        assert!(json.contains("\"folderId\":\"f1\""), "{json}");
        let back: Shelf = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
        // A blob from before `rel` existed is the folder's root shelf.
        let older: Shelf = serde_json::from_str(
            r#"{"id":"s2","name":"scifi","kind":{"kind":"folder","folderId":"f1"}}"#,
        )
        .unwrap();
        assert!(older.kind.is_folder_root());
    }

    #[test]
    fn sanitize_dedupes_shelves_and_their_members() {
        let mut shelves = vec![
            shelf("s1", "One", &["a", "a", "b", "  "]),
            shelf("s1", "One again", &["c"]),
            shelf("  ", "Nameless id", &[]),
            shelf("s3", "   ", &[]),
        ];
        sanitize(&mut shelves);
        assert_eq!(shelves.len(), 1);
        assert_eq!(ids(&shelves[0].books), vec!["a", "b"]);
    }

    #[test]
    fn a_shelf_without_a_kind_still_loads() {
        let s: Shelf = serde_json::from_str(r#"{"id":"s1","name":"One"}"#).unwrap();
        assert_eq!(s.kind, ShelfKind::Virtual);
        assert!(s.books.is_empty());
        assert_eq!(s.parent, None, "a blob from before nesting has no parent");
    }

    #[test]
    fn a_level_is_the_shelves_filed_directly_inside_it() {
        let shelves = vec![
            shelf("s1", "Fiction", &[]),
            nested("s2", "Sci-fi", "s1"),
            nested("s3", "Crime", "s1"),
            nested("s4", "Space", "s2"),
        ];
        assert_eq!(ids_of(&children_of(&shelves, None)), vec!["s1"]);
        assert_eq!(ids_of(&children_of(&shelves, Some("s1"))), vec!["s2", "s3"]);
        assert_eq!(ids_of(&children_of(&shelves, Some("s2"))), vec!["s4"]);
        assert!(children_of(&shelves, Some("nope")).is_empty());
    }

    #[test]
    fn the_way_out_of_a_shelf_is_the_chain_above_it() {
        let shelves = vec![
            shelf("s1", "Fiction", &[]),
            nested("s2", "Sci-fi", "s1"),
            nested("s3", "Space", "s2"),
        ];
        assert!(ancestors(&shelves, "s1").is_empty());
        assert_eq!(ids_of(&ancestors(&shelves, "s2")), vec!["s1"]);
        assert_eq!(ids_of(&ancestors(&shelves, "s3")), vec!["s1", "s2"]);
        // "All" is not a shelf, so it has no chain either.
        assert!(ancestors(&shelves, ALL_SHELF).is_empty());
        assert!(ancestors(&shelves, "nope").is_empty());
    }

    #[test]
    fn a_shelf_cannot_be_filed_inside_itself_or_its_own_children() {
        // s3 is inside s2 and both sit at the root, so s1 is the one shelf that is
        // nobody's ancestor: it may go anywhere, and the other two may not go down
        // their own branch.
        let shelves = vec![
            shelf("s1", "Fiction", &[]),
            shelf("s2", "Sci-fi", &[]),
            nested("s3", "Space", "s2"),
        ];
        assert!(can_nest(&shelves, "s1", "s3"), "s1 is above nothing, so it can go deepest");
        assert!(can_nest(&shelves, "s2", "s1"));
        assert!(can_nest(&shelves, "s3", "s1"), "and a shelf may be lifted out of its branch");
        assert!(!can_nest(&shelves, "s2", "s2"), "a shelf is not inside itself");
        assert!(!can_nest(&shelves, "s1", "s1"));
        assert!(!can_nest(&shelves, "s2", "s3"), "s3 is already inside s2");
        assert!(!can_nest(&shelves, "s2", "s2"));
        // A shelf that is not in the list is not a cycle, so the graph rule
        // allows it; `reparent` is the half that refuses a shelf it cannot find.
        assert!(can_nest(&shelves, "s9", "s1"));
        let mut none: Vec<Shelf> = Vec::new();
        assert!(!reparent(&mut none, "s9", Some("s1")));
    }

    #[test]
    fn a_nest_writes_one_parent_and_a_refusal_writes_nothing() {
        let mut shelves = vec![
            shelf("s1", "Fiction", &[]),
            shelf("s2", "Sci-fi", &[]),
            nested("s3", "Space", "s2"),
        ];
        assert!(reparent(&mut shelves, "s2", Some("s1")));
        assert_eq!(shelves[1].parent.as_deref(), Some("s1"));
        assert_eq!(ids_of(&children_of(&shelves, Some("s1"))), vec!["s2"]);
        // s2 now holds s3, so filing s1 inside s3 closes the loop and is refused
        // with the list untouched.
        assert!(!can_nest(&shelves, "s1", "s3"));
        assert!(!reparent(&mut shelves, "s1", Some("s3")));
        assert_eq!(shelves[0].parent, None);
        // Back out to the root.
        assert!(reparent(&mut shelves, "s2", None));
        assert_eq!(shelves[1].parent, None);
    }

    #[test]
    fn a_hand_moved_watched_shelf_keeps_its_move_and_says_so() {
        let mut shelves = vec![
            shelf("s1", "Fiction", &[]),
            Shelf {
                kind: ShelfKind::Folder {
                    folder_id: "f1".into(),
                    rel: Some("scifi".into()),
                },
                ..shelf("s2", "Watched", &[])
            },
        ];
        // The reader's hand beats the disk's shape: the move lands, and the row
        // carries the mark the next re-hang reads — which is what makes the
        // move a promise the rescan KEEPS instead of one it breaks.
        assert!(reparent(&mut shelves, "s2", Some("s1")));
        assert_eq!(shelves[1].parent.as_deref(), Some("s1"));
        assert!(shelves[1].manual_parent);
        // Its disk knowledge is the move's survivor: the kind — and with it
        // the `rel` that routes the folder's new files into this very shelf —
        // is untouched by a change of place.
        assert!(matches!(
            shelves[1].kind,
            ShelfKind::Folder { ref rel, .. } if rel.as_deref() == Some("scifi")
        ));
        // A virtual shelf's move is the reader's from the start: no scan ever
        // wrote its parent, so there is no scan to tell to stand aside.
        assert!(reparent(&mut shelves, "s1", None));
        assert!(!shelves[0].manual_parent);
        // And the loop rule holds for a watched shelf exactly as for any
        // other: s2 is inside s1, so s1 inside s2 is refused, mark or no mark.
        assert!(!reparent(&mut shelves, "s1", Some("s2")));
        assert_eq!(shelves[0].parent, None);
    }

    #[test]
    fn a_shelf_from_before_the_mark_existed_is_the_scans() {
        // No `manualParent` key in an older blob is not a hand-move: every
        // shelf in it is where the last scan put it, and the next one may
        // re-hang it.
        let older: Shelf = serde_json::from_str(
            r#"{"id":"s2","name":"scifi","kind":{"kind":"folder","folderId":"f1"}}"#,
        )
        .unwrap();
        assert!(!older.manual_parent);
        // And a shelf the reader moved persists as one.
        let moved = Shelf {
            manual_parent: true,
            parent: Some("s1".into()),
            ..older.clone()
        };
        let json = serde_json::to_string(&moved).unwrap();
        assert!(json.contains("\"manualParent\":true"), "{json}");
        let back: Shelf = serde_json::from_str(&json).unwrap();
        assert_eq!(back, moved);
    }

    #[test]
    fn taking_a_shelf_apart_lifts_the_shelves_inside_it() {
        let mut shelves = vec![
            shelf("s1", "Fiction", &[]),
            nested("s2", "Sci-fi", "s1"),
            nested("s3", "Space", "s2"),
            shelf("s4", "Unrelated", &[]),
        ];
        lift_children(&mut shelves, "s2");
        assert_eq!(
            shelves[2].parent.as_deref(),
            Some("s1"),
            "s3 inherits the level s2 was on, not the top of the library"
        );
        assert_eq!(shelves[3].parent, None, "a shelf elsewhere is not touched");
        lift_children(&mut shelves, "s1");
        assert_eq!(shelves[1].parent, None, "a root shelf's children become roots");
        assert_eq!(
            shelves[2].parent, None,
            "including the one that just moved up into it"
        );
    }

    #[test]
    fn sanitize_collapses_a_parent_that_names_no_shelf() {
        let mut shelves = vec![nested("s1", "Orphan", "gone"), nested("s2", "Filed", "s1")];
        sanitize(&mut shelves);
        assert_eq!(shelves[0].parent, None, "an orphan is a root, not a hole");
        assert_eq!(shelves[1].parent.as_deref(), Some("s1"), "its child is untouched");
        assert_eq!(ids_of(&children_of(&shelves, None)), vec!["s1"]);
    }

    #[test]
    fn sanitize_opens_a_cycle_and_leaves_the_branch_above_it_alone() {
        // s1 is filed in s2, and s2 and s3 are filed in each other: the loop is
        // the pair, and s1 only leads into it.
        let mut shelves = vec![
            nested("s1", "Leads in", "s2"),
            nested("s2", "Loop a", "s3"),
            nested("s3", "Loop b", "s2"),
            shelf("s4", "Unrelated", &[]),
        ];
        sanitize(&mut shelves);
        assert_eq!(shelves[1].parent, None, "both edges of the loop are cut");
        assert_eq!(shelves[2].parent, None);
        assert_eq!(
            shelves[0].parent.as_deref(),
            Some("s2"),
            "a shelf that leads into the loop keeps the parent it had"
        );
        assert_eq!(shelves[3].parent, None);
        // Nothing is unreachable: every shelf is on some level again.
        let mut on_a_level: Vec<&str> = [None, Some("s1"), Some("s2"), Some("s3")]
            .iter()
            .flat_map(|at| children_of(&shelves, *at))
            .map(|s| s.id.as_str())
            .collect();
        on_a_level.sort_unstable();
        assert_eq!(on_a_level, vec!["s1", "s2", "s3", "s4"]);
        // Idempotent: a second pass finds a forest and changes nothing.
        let before = shelves.clone();
        sanitize(&mut shelves);
        assert_eq!(shelves, before);
    }

    #[test]
    fn a_shelf_filed_inside_itself_is_put_back_at_the_root() {
        let mut shelves = vec![Shelf {
            parent: Some("s1".into()),
            ..shelf("s1", "Self", &[])
        }];
        sanitize(&mut shelves);
        assert_eq!(shelves[0].parent, None);
        assert_eq!(ids_of(&children_of(&shelves, None)), vec!["s1"]);
    }

    #[test]
    fn a_parent_persists_with_the_shelf() {
        let s = nested("s2", "Sci-fi", "s1");
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"parent\":\"s1\""), "{json}");
        let back: Shelf = serde_json::from_str(&json).unwrap();
        assert_eq!(back, s);
    }

    /// A shelf cut from a watched folder's tree: `rel` is the rung it serves,
    /// `None` for the folder's root shelf.
    fn cut(id: &str, folder_id: &str, rel: Option<&str>, parent: Option<&str>) -> Shelf {
        Shelf {
            id: id.to_string(),
            name: id.to_string(),
            kind: ShelfKind::Folder {
                folder_id: folder_id.to_string(),
                rel: rel.map(str::to_string),
            },
            books: Vec::new(),
            parent: parent.map(str::to_string),
            manual_parent: false,
        }
    }

    #[test]
    fn a_flat_subfolder_shelf_rehangs_under_the_rung_it_was_cut_from() {
        // An older build minted "2/deep" as a sibling of the root; the disk's
        // tree says it hangs on "2", and "2" on the folder's own shelf.
        let shelves = vec![
            cut("r", "f1", None, None),
            cut("two", "f1", Some("2"), None),
            cut("deep", "f1", Some("2/deep"), None),
        ];
        let moves = rehang_moves(&shelves, "f1");
        assert_eq!(
            moves,
            vec![
                ("two".to_string(), Some("r".to_string())),
                ("deep".to_string(), Some("two".to_string())),
            ]
        );
    }

    #[test]
    fn a_hand_moved_shelf_keeps_its_place_and_still_routes_its_subtree() {
        let mut two = cut("two", "f1", Some("2"), Some("r"));
        two.manual_parent = true;
        let shelves = vec![
            cut("r", "f1", None, None),
            two,
            // The child still resolves its rung through the moved shelf: the
            // subtree the reader carried off re-hangs together.
            cut("deep", "f1", Some("2/deep"), None),
        ];
        let moves = rehang_moves(&shelves, "f1");
        assert_eq!(moves, vec![("deep".to_string(), Some("two".to_string()))]);
    }

    #[test]
    fn a_rehang_never_touches_a_virtual_shelf_or_another_folders() {
        let shelves = vec![
            cut("r", "f1", None, None),
            plain("mine", &[]),
            cut("other", "f2", None, None),
        ];
        assert!(rehang_moves(&shelves, "f1").is_empty());
    }

    #[test]
    fn a_stale_rung_that_names_the_shelf_itself_is_not_a_move_onto_itself() {
        // "2/deep" whose rung resolves through a map that names ITSELF: the
        // self-edge is filtered, and what is left is the honest answer — the
        // shelf belongs on the rung above, and with no "2" in the list that is
        // a re-hang to the root.
        let shelves = vec![cut("loop", "f1", Some("loop"), Some("r"))];
        let moves = rehang_moves(&shelves, "f1");
        assert_eq!(moves, vec![("loop".to_string(), None)]);
    }

    use crate::folder::{FolderOpts, WatchedFolder};
    use crate::tracking::TrackingTree;
    use std::collections::{BTreeMap, HashSet};

    /// `/books` read in place, cut into three rungs — the root ("r"),
    /// "Fiction" ("fic") and "Fiction/SciFi" ("sf") — with the map that names
    /// each, and one virtual shelf of the reader's own beside the tree.
    fn in_place_tree() -> (Vec<Shelf>, Vec<WatchedFolder>) {
        let shelves = vec![
            cut("r", "f1", None, None),
            cut("fic", "f1", Some("Fiction"), Some("r")),
            cut("sf", "f1", Some("Fiction/SciFi"), Some("fic")),
            plain("mine", &[]),
        ];
        let folders = vec![WatchedFolder {
            id: "f1".into(),
            root: "/books".into(),
            opts: FolderOpts::default(),
            placed: HashSet::new(),
            ignored: Vec::new(),
            last_seen: Vec::new(),
            shelf_map: BTreeMap::from([
                (String::new(), "r".to_string()),
                ("Fiction".to_string(), "fic".to_string()),
                ("Fiction/SciFi".to_string(), "sf".to_string()),
            ]),
            scanned_ms: 0,
            tracking: TrackingTree::default(),
        }];
        (shelves, folders)
    }

    fn marked(shelves: &[Shelf], id: &str) -> bool {
        shelves
            .iter()
            .find(|s| s.id == id)
            .is_some_and(|s| s.manual_parent)
    }

    #[test]
    fn a_rung_of_a_reading_folder_departs_whatever_it_leaves_for() {
        let (shelves, folders) = in_place_tree();
        // Another rung of the very tree that named the shelf is still a
        // departure: what ties a rung to its folder is the seat its directory
        // stands on, not membership of the folder's shelf tree — the shelf's
        // own spelling of the book rule one level down.
        assert!(departs_on_move(&shelves, &folders, "sf", Some("r")));
        // The reader's own shelf and the library's root are nobody's rung.
        assert!(departs_on_move(&shelves, &folders, "sf", Some("mine")));
        assert!(departs_on_move(&shelves, &folders, "sf", None));
        // And the tree's root rung departs into any shelf: the seat it stands
        // on is the root, and a move off the root is a move off the seat.
        assert!(departs_on_move(&shelves, &folders, "r", Some("mine")));
    }

    #[test]
    fn a_reorder_on_the_seat_and_a_return_to_it_copy_nothing() {
        let (shelves, folders) = in_place_tree();
        // A re-order among siblings is the same parent, which is the same
        // ground: the cheapest drag in the library must stay the cheapest.
        assert!(!departs_on_move(&shelves, &folders, "sf", Some("fic")));
        assert!(!departs_on_move(&shelves, &folders, "r", None));
        // A rung an older blob carries off its seat comes BACK to the seat as
        // a return: the hand put it where the disk names, so no copy is owed.
        let mut off_seat = shelves.clone();
        off_seat
            .iter_mut()
            .find(|s| s.id == "sf")
            .unwrap()
            .parent = Some("mine".to_string());
        assert!(!departs_on_move(&off_seat, &folders, "sf", Some("fic")));
    }

    #[test]
    fn a_virtual_shelf_and_a_copying_folder_s_move_freely() {
        let (mut shelves, mut folders) = in_place_tree();
        shelves.push(cut("stored", "f2", None, None));
        folders.push(WatchedFolder {
            id: "f2".into(),
            root: "/dvds".into(),
            opts: FolderOpts {
                in_place: false,
                ..FolderOpts::default()
            },
            placed: HashSet::new(),
            ignored: Vec::new(),
            last_seen: Vec::new(),
            shelf_map: BTreeMap::from([(String::new(), "stored".to_string())]),
            scanned_ms: 0,
            tracking: TrackingTree::default(),
        });
        // The reader's own shelf is the reader's wherever it hangs.
        assert!(!departs_on_move(&shelves, &folders, "mine", Some("r")));
        // A copying folder's shelf is the library's own already: no ledger
        // waits on its rung, so the move is the membership edit it always was.
        assert!(!departs_on_move(&shelves, &folders, "stored", Some("mine")));
        // A shelf the list does not hold, and a folder shelf no folder
        // answers for, owe nothing at all.
        assert!(!departs_on_move(&shelves, &folders, "gone", Some("r")));
        let orphan = vec![cut("orphan", "f9", None, None)];
        assert!(!departs_on_move(&orphan, &folders, "orphan", Some("mine")));
    }

    #[test]
    fn a_departing_shelf_inside_another_departing_one_rides_with_it() {
        let (shelves, folders) = in_place_tree();
        let ids: Vec<String> = ["fic", "sf", "mine"]
            .iter()
            .map(|id| id.to_string())
            .collect();
        let (clean, departing) = departing_moves(&shelves, &folders, &ids, None);
        assert_eq!(
            departing,
            vec!["fic".to_string()],
            "sf rides inside fic's copy and asks nothing of its own"
        );
        assert_eq!(clean, vec!["mine".to_string()]);
    }

    #[test]
    fn a_hand_that_puts_a_shelf_back_on_its_seat_gives_the_scan_its_place_back() {
        let (mut shelves, _) = in_place_tree();
        // Off the seat, the mark goes on: the next re-hang passes the shelf by.
        assert!(reparent(&mut shelves, "sf", Some("mine")));
        assert!(marked(&shelves, "sf"));
        // Back on the seat the disk names, the mark comes off: the place is
        // the scan's again, and the re-hang owns the shelf like any other.
        assert!(reparent(&mut shelves, "sf", Some("fic")));
        assert!(!marked(&shelves, "sf"));
        // A re-order on the seat the shelf already hangs on writes the same
        // answer it had: the mark is a fact about the place, not the gesture.
        assert!(reparent(&mut shelves, "sf", Some("fic")));
        assert!(!marked(&shelves, "sf"));
    }

    #[test]
    fn the_family_is_the_deepest_tree_whose_rung_for_the_ground_is_free() {
        let (shelves, folders) = in_place_tree();
        // A ground deep in f1's tree whose rung the map does not name: the
        // family is f1, and the key is the rung the ground names in it.
        assert_eq!(
            family_for(&folders, &shelves, "/books/Fiction/Deleted"),
            Some(("f1".to_string(), "Fiction/Deleted".to_string()))
        );
        // A rung the map names AND whose shelf stands is no family to fold
        // into — it is a covered shelf, the import gate's own answer.
        assert_eq!(
            family_for(&folders, &shelves, "/books/Fiction/SciFi"),
            None
        );
        // The tree's own root is not inside its family.
        assert_eq!(family_for(&folders, &shelves, "/books"), None);
        // A dead map entry — a rung whose shelf is gone — counts as free.
        let mut dead_slot = folders.clone();
        dead_slot[0]
            .shelf_map
            .insert("Fiction/SciFi".to_string(), "gone".to_string());
        assert_eq!(
            family_for(&dead_slot, &shelves, "/books/Fiction/SciFi"),
            Some(("f1".to_string(), "Fiction/SciFi".to_string()))
        );
        // Of two nested trees, the DEEPEST wins: its rung is the seat the
        // ground's own directory names.
        let mut deeper = folders[0].clone();
        deeper.id = "f2".into();
        deeper.root = "/books/Fiction".into();
        deeper.shelf_map.clear();
        let mut outer = folders[0].clone();
        outer.shelf_map.remove("Fiction/SciFi");
        let both = vec![outer, deeper];
        assert_eq!(
            family_for(&both, &shelves, "/books/Fiction/SciFi"),
            Some(("f1".to_string(), "Fiction/SciFi".to_string()))
        );
    }

    #[test]
    fn the_subtree_is_everything_below_and_never_the_root_itself() {
        let tree = vec![
            shelf("a", "A", &[]),
            nested("b", "B", "a"),
            nested("c", "C", "b"),
            nested("d", "D", "a"),
        ];
        let mut under_a = subtree_ids(&tree, &["a".to_string()]);
        under_a.sort();
        assert_eq!(
            under_a,
            vec!["b".to_string(), "c".to_string(), "d".to_string()]
        );
        assert!(
            subtree_ids(&tree, &["d".to_string()]).is_empty(),
            "an empty leaf has no subtree"
        );
        // Two roots that share a descendant count it once — and a root is
        // never reported as its own descendant.
        let mut under_both = subtree_ids(&tree, &["a".to_string(), "b".to_string()]);
        under_both.sort();
        assert_eq!(under_both, vec!["c".to_string(), "d".to_string()]);
        // And a loop a hand-edited blob can still carry terminates: x is the
        // root and is never counted, y is below it, and x-inside-y is the root
        // again, which the seen-set refuses.
        let looped = vec![nested("x", "X", "y"), nested("y", "Y", "x")];
        assert_eq!(
            subtree_ids(&looped, &["x".to_string()]),
            vec!["y".to_string()]
        );
    }
}
