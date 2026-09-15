//! Shelves: an ordered list of book ids, and the kinds that produce one.
//!
//! A shelf holds membership and nothing else — no copies, no paths, no
//! filesystem intent — which makes dragging a book between shelves safe by
//! construction: a drop can only change an ordered list of ids.

use serde::{Deserialize, Serialize};

mod family;
mod members;
mod tree;

pub use family::{departing_moves, departs_on_move, family_for};
pub use members::{containing, forget, forget_everywhere, members_of, place, shelf_add};
pub use tree::{
    ancestors, can_nest, children_of, lift_children, rehang_moves, reparent, rung_above, rungs_of,
    subtree_ids,
};

/// The pseudo-shelf holding every book: the library's root level, ordered by
/// the persisted book list. Not a [`Shelf`].
pub const ALL_SHELF: &str = "all";

/// What produced a shelf, which decides whether the UI offers it a folder glyph, a watch dot, or a rename.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ShelfKind {
    /// The default, because a shelf a blob does not describe is one the reader made.
    #[default]
    Virtual,
    /// Cut from a watched folder's tree. `rel` is the subfolder within
    /// [`crate::folder::WatchedFolder::root`], `None` for the root itself.
    Folder {
        #[serde(rename = "folderId")]
        folder_id: String,
        #[serde(default)]
        rel: Option<String>,
    },
    /// A rung that left its tree: the reader moved it off the seat the
    /// folder's shelves name for it and the move paid the copy, so its books
    /// are the library's own and no tree answers for this shelf any more.
    Departed,
}

impl ShelfKind {
    /// The folder a shelf is a rung of; a virtual or departed shelf is
    /// nobody's rung.
    pub fn folder_id(&self) -> Option<&str> {
        match self {
            ShelfKind::Virtual | ShelfKind::Departed => None,
            ShelfKind::Folder { folder_id, .. } => Some(folder_id),
        }
    }

    pub fn is_folder_root(&self) -> bool {
        matches!(self, ShelfKind::Folder { rel: None, .. })
    }

    /// The rung this shelf stands on, keyed like the folder's own map: the
    /// empty string for a watched root and for a shelf no directory names
    /// (virtual or departed). One spelling of "which rung is this" for
    /// callers asking a standing shelf rather than a ledger.
    pub fn rung(&self) -> &str {
        match self {
            ShelfKind::Virtual | ShelfKind::Departed => "",
            ShelfKind::Folder { rel, .. } => rel.as_deref().unwrap_or(""),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Shelf {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub kind: ShelfKind,
    /// Member row ids in the reader's order: a shelf is a list, not a set.
    #[serde(default)]
    pub books: Vec<String>,
    /// Defaulted: a blob written before shelves could nest has no `parent`
    /// key, and every shelf in it is a root shelf.
    #[serde(default)]
    pub parent: Option<String>,
    /// The reader moved this shelf by hand off the seat its folder's shelves
    /// name for it, so its place is the reader's, not the disk's: a rescan
    /// re-hangs the shelves it owns and skips one wearing this mark. Written
    /// by [`reparent`], which clears the mark when a hand puts the shelf back.
    #[serde(default)]
    pub manual_parent: bool,
}

impl Shelf {
    pub fn is_folder(&self) -> bool {
        self.kind.folder_id().is_some()
    }

    /// A shelf the reader made (or a copy's landing level): no folder answers
    /// for it. One constructor, so the field list is answered here rather than
    /// spelled out at every mint.
    pub fn virtual_shelf(
        id: impl Into<String>,
        name: impl Into<String>,
        parent: Option<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            kind: ShelfKind::Virtual,
            books: Vec::new(),
            parent,
            manual_parent: false,
        }
    }

    /// A rung a folder's tree cut: `rel` is the subfolder it stands for,
    /// `None` for the root. A minted rung starts on the tree's own ground
    /// (`manual_parent` false); a hand-move sets the mark later.
    pub fn folder_shelf(
        id: impl Into<String>,
        name: impl Into<String>,
        folder_id: impl Into<String>,
        rel: Option<String>,
        parent: Option<String>,
    ) -> Self {
        Self {
            id: id.into(),
            name: name.into(),
            kind: ShelfKind::Folder {
                folder_id: folder_id.into(),
                rel,
            },
            books: Vec::new(),
            parent,
            manual_parent: false,
        }
    }
}

/// `None` for [`ALL_SHELF`]: the caller falls back to the whole book list.
pub fn find<'a>(shelves: &'a [Shelf], id: &str) -> Option<&'a Shelf> {
    if id == ALL_SHELF {
        return None;
    }
    shelves.iter().find(|s| s.id == id)
}

/// Like [`find`], and the reason both special-case [`ALL_SHELF`]: the
/// pseudo-shelf is the book list and has no member list, so a caller handing
/// a route's shelf id straight in gets `None` and takes its root-level branch.
pub fn find_mut<'a>(shelves: &'a mut [Shelf], id: &str) -> Option<&'a mut Shelf> {
    if id == ALL_SHELF {
        return None;
    }
    shelves.iter_mut().find(|s| s.id == id)
}

/// Drop shelves with no id or name and any row wearing the [`ALL_SHELF`] id,
/// dedupe by id (first wins), drop blank and duplicate members, and cut the
/// nesting graph back to a forest. Idempotent.
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

    // A parent that is the shelf itself, or that names no shelf, renders on
    // no level: both collapse to the root.
    for s in shelves.iter_mut() {
        if s.parent.as_deref() == Some(s.id.as_str()) {
            s.parent = None;
        }
    }
    // Owned ids: the pass below writes to the list it reads names from.
    let ids: std::collections::HashSet<String> =
        shelves.iter().map(|s| s.id.clone()).collect();
    for s in shelves.iter_mut() {
        if s.parent.as_deref().is_some_and(|p| !ids.contains(p)) {
            s.parent = None;
        }
    }

    // Then the cycles, which no single row shows. Only shelves ON a loop are
    // cut: one that merely leads into a loop keeps its parent and becomes a
    // root shelf's child once the loop below it is open.
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
        // `sanitize` drops a row wearing the id; the lookups answer it as a rule.
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
        assert_eq!(members_of(&rows, &shelves, ALL_SHELF), Vec::<&str>::new());
        let one_filed = vec![plain("s", &["b1"])];
        assert_eq!(members_of(&rows, &one_filed, ALL_SHELF), vec!["b2", "l1"]);
    }

    use super::*;

    fn shelf(id: &str, name: &str, books: &[&str]) -> Shelf {
        crate::testkit::shelf(id, name, books, None)
    }

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
        // A blob carrying an "all" row must not become a tile duplicating the
        // whole library.
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
        // Appending an existing member would reshuffle a shelf for an
        // instruction that was not about position.
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
        assert!(ancestors(&shelves, ALL_SHELF).is_empty());
        assert!(ancestors(&shelves, "nope").is_empty());
    }

    #[test]
    fn a_shelf_cannot_be_filed_inside_itself_or_its_own_children() {
        // s3 is inside s2 and both sit at the root, so s1 is the one shelf
        // that is nobody's ancestor.
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
        assert!(!can_nest(&shelves, "s1", "s3"));
        assert!(!reparent(&mut shelves, "s1", Some("s3")));
        assert_eq!(shelves[0].parent, None);
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
        // The reader's hand beats the disk's shape: the move lands, and the
        // mark makes it a promise the rescan keeps.
        assert!(reparent(&mut shelves, "s2", Some("s1")));
        assert_eq!(shelves[1].parent.as_deref(), Some("s1"));
        assert!(shelves[1].manual_parent);
        assert!(matches!(
            shelves[1].kind,
            ShelfKind::Folder { ref rel, .. } if rel.as_deref() == Some("scifi")
        ));
        assert!(reparent(&mut shelves, "s1", None));
        assert!(!shelves[0].manual_parent);
        assert!(!reparent(&mut shelves, "s1", Some("s2")));
        assert_eq!(shelves[0].parent, None);
    }

    #[test]
    fn a_shelf_from_before_the_mark_existed_is_the_scans() {
        // No `manualParent` key in an older blob is not a hand-move.
        let older: Shelf = serde_json::from_str(
            r#"{"id":"s2","name":"scifi","kind":{"kind":"folder","folderId":"f1"}}"#,
        )
        .unwrap();
        assert!(!older.manual_parent);
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
        let mut on_a_level: Vec<&str> = [None, Some("s1"), Some("s2"), Some("s3")]
            .iter()
            .flat_map(|at| children_of(&shelves, *at))
            .map(|s| s.id.as_str())
            .collect();
        on_a_level.sort_unstable();
        assert_eq!(on_a_level, vec!["s1", "s2", "s3", "s4"]);
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

    /// `rel` is the rung it serves, `None` for the folder's root shelf.
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
        // tree says it hangs on "2".
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
            // The subtree the reader carried off re-hangs together.
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
    fn a_stale_rung_that_names_the_shelf_itself_rehangs_on_the_rung_above_it() {
        // The shelf's own key is never its seat: the rung above it answers,
        // and with no root rung in the list that is the library's top level.
        let shelves = vec![cut("loop", "f1", Some("loop"), Some("r"))];
        let moves = rehang_moves(&shelves, "f1");
        assert_eq!(moves, vec![("loop".to_string(), None)]);
    }

    #[test]
    fn a_rung_under_a_level_that_was_taken_apart_hangs_inside_the_tree() {
        // `2nd` stood inside `1st` and `1st` is gone: its books come up to
        // the rung the tree still stands on, never out of the tree.
        let shelves = vec![
            cut("r", "f1", None, None),
            cut("second", "f1", Some("1st/2nd"), None),
        ];
        assert_eq!(
            rehang_moves(&shelves, "f1"),
            vec![("second".to_string(), Some("r".to_string()))]
        );
    }

    #[test]
    fn the_rung_a_key_hangs_on_is_the_nearest_one_still_standing() {
        let shelves = vec![
            cut("r", "f1", None, None),
            cut("first", "f1", Some("1st"), Some("r")),
            cut("deep", "f1", Some("1st/2nd/books"), Some("first")),
        ];
        assert_eq!(
            rung_above(&shelves, "f1", "1st/2nd"),
            Some("first".to_string()),
            "the level between them is gone, so one rung up is the answer"
        );
        assert_eq!(
            rung_above(&shelves, "f1", "1st/2nd/books"),
            Some("first".to_string()),
            "and a shelf two levels under a hole comes up to the same rung"
        );
        assert_eq!(rung_above(&shelves, "f1", "1st"), Some("r".to_string()));
        assert_eq!(
            rung_above(&shelves, "f1", ""),
            None,
            "the root rung is the one key with nothing above it"
        );
        let bare: Vec<Shelf> = Vec::new();
        assert_eq!(rung_above(&bare, "f1", "1st"), None);
    }

    use crate::folder::{FolderOpts, WatchedFolder};
    use crate::tracking::TrackingTree;
    use std::collections::{BTreeMap, HashSet};

    /// `/books` read in place, cut into three rungs — root ("r"), "Fiction"
    /// ("fic"), "Fiction/SciFi" ("sf") — plus one virtual shelf.
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
            shapes: crate::shape::ShapeTree::default(),
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
        // Another rung of the same tree is still a departure: what ties a
        // rung to its folder is the seat its directory stands on, not
        // membership of the folder's shelf tree.
        assert!(departs_on_move(&shelves, &folders, "sf", Some("r")));
        assert!(departs_on_move(&shelves, &folders, "sf", Some("mine")));
        assert!(departs_on_move(&shelves, &folders, "sf", None));
        assert!(departs_on_move(&shelves, &folders, "r", Some("mine")));
    }

    #[test]
    fn a_reorder_on_the_seat_and_a_return_to_it_copy_nothing() {
        let (shelves, folders) = in_place_tree();
        // A re-order among siblings is the same ground: the cheapest drag in
        // the library must stay the cheapest.
        assert!(!departs_on_move(&shelves, &folders, "sf", Some("fic")));
        assert!(!departs_on_move(&shelves, &folders, "r", None));
        // A rung an older blob carries off its seat comes back as a return:
        // no copy is owed.
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
            shapes: crate::shape::ShapeTree::default(),
        });
        assert!(!departs_on_move(&shelves, &folders, "mine", Some("r")));
        // A copying folder's shelf is the library's own: no ledger waits on
        // its rung.
        assert!(!departs_on_move(&shelves, &folders, "stored", Some("mine")));
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
        assert!(reparent(&mut shelves, "sf", Some("mine")));
        assert!(marked(&shelves, "sf"));
        // Back on the seat the disk names, the mark comes off.
        assert!(reparent(&mut shelves, "sf", Some("fic")));
        assert!(!marked(&shelves, "sf"));
        // A re-order on the shelf's current seat writes the same answer it had.
        assert!(reparent(&mut shelves, "sf", Some("fic")));
        assert!(!marked(&shelves, "sf"));
    }

    #[test]
    fn the_family_is_the_deepest_tree_whose_rung_for_the_ground_is_free() {
        let (shelves, folders) = in_place_tree();
        // Ground deep in f1's tree whose rung the map does not name: the
        // family is f1.
        assert_eq!(
            family_for(&folders, &shelves, "/books/Fiction/Deleted"),
            Some(("f1".to_string(), "Fiction/Deleted".to_string()))
        );
        assert_eq!(
            family_for(&folders, &shelves, "/books/Fiction/SciFi"),
            None
        );
        assert_eq!(family_for(&folders, &shelves, "/books"), None);
        let mut dead_slot = folders.clone();
        dead_slot[0]
            .shelf_map
            .insert("Fiction/SciFi".to_string(), "gone".to_string());
        assert_eq!(
            family_for(&dead_slot, &shelves, "/books/Fiction/SciFi"),
            Some(("f1".to_string(), "Fiction/SciFi".to_string()))
        );
        let mut deeper = folders[0].clone();
        deeper.id = "f2".into();
        deeper.root = "/books/Fiction".into();
        deeper.shelf_map.clear();
        deeper
            .shelf_map
            .insert(String::new(), "deeper_root".to_string());
        let mut both_shelves = shelves.clone();
        both_shelves.push(cut("deeper_root", "f2", None, None));
        let mut outer = folders[0].clone();
        outer.shelf_map.remove("Fiction/SciFi");
        let both = vec![outer, deeper];
        assert_eq!(
            family_for(&both, &both_shelves, "/books/Fiction/SciFi"),
            Some(("f1".to_string(), "Fiction/SciFi".to_string()))
        );
    }

    #[test]
    fn a_taken_out_root_leaves_no_family_behind_it() {
        let (shelves, folders) = in_place_tree();
        // A rung a removal emptied is a home to come back to while the tree's
        // root stands.
        assert_eq!(
            family_for(&folders, &shelves, "/books/Fiction/Deleted"),
            Some(("f1".to_string(), "Fiction/Deleted".to_string()))
        );
        // With the root shelf gone the tree is out of the library and the
        // ground under it is a start of its own.
        let taken_out: Vec<Shelf> = shelves.iter().filter(|s| s.id != "r").cloned().collect();
        assert_eq!(family_for(&folders, &taken_out, "/books/Fiction/Deleted"), None);
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
        let mut under_both = subtree_ids(&tree, &["a".to_string(), "b".to_string()]);
        under_both.sort();
        assert_eq!(under_both, vec!["c".to_string(), "d".to_string()]);
        // A loop a hand-edited blob can carry still terminates: the seen-set
        // refuses the second visit of a root.
        let looped = vec![nested("x", "X", "y"), nested("y", "Y", "x")];
        assert_eq!(
            subtree_ids(&looped, &["x".to_string()]),
            vec!["y".to_string()]
        );
    }
}
