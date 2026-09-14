//! One answer to "who owns this path", instead of four.
//!
//! Four places in the library each walked the watched-folder list and the shelf
//! list to answer a variation of the same question — which read-at-place tree's
//! ledger speaks for this ground, and which of its shelves is still standing:
//!
//!   * the import gate's `covered_of` — the standing shelf a tree already holds
//!     for a pick of ground, which turns a re-import into a reconciliation;
//!   * the import gate's `displaced_member` — a member of one tree standing
//!     outside it, which an explicit run folds back;
//!   * [`crate::shelf::family_for`] — the deepest in-place tree a ground belongs
//!     to but is not standing in, which an import folds into;
//!   * the departure's `converts_on_move_to` — whether the folder that placed a
//!     book still names the shelf it is leaving as the rung for its address.
//!
//! Each re-derived "iterate the in-place folders, walk [`crate::folder::rel_under`],
//! read the `shelf_map`, check the shelf stands" for itself: four places to keep
//! in step and four sets of edge cases waiting to drift. This module is the one
//! resolver they read. It is pure and borrowed — built from the two lists a
//! caller already holds, asked, and dropped inside one decision — so the
//! arithmetic lives here once and a host test can name the case it asserts.
//!
//! `displaced_member` stays in the gate: it is asked of a walk's own findings
//! and the live shelf list rather than of the two lists alone, so it is a caller
//! of this resolver's vocabulary rather than a method on it. The other three
//! are here.

use crate::book::Fingerprint;
use crate::folder::{rel_under, WatchedFolder};
use crate::shelf::{ancestors, find as find_shelf, Shelf, ShelfKind};

/// The folder, the rung and the standing shelf that answer for a path.
///
/// A value rather than a tuple at the call site: the three facts a covered
/// answer carries are easy to transpose, and a gate that read `.1` for a shelf
/// id and `.2` for a folder id would be a bug no type catches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Coverage {
    /// The tree whose ledger answers for the path.
    pub folder_id: String,
    /// The rung key the path names in that tree — `""` for the tree's root.
    pub rel: String,
    /// The standing shelf at that rung.
    pub shelf_id: String,
}

/// The seat a SHELF stands on in a read-at-place tree: which folder's ground it
/// wears, and which rung of that folder's tree it is — `""` at the watched root,
/// the rung's rel for a shelf the tree cut, and the closest folder shelf's rung
/// for a shelf the reader made inside the tree.
///
/// The shelf-shaped twin of [`Coverage`], which answers the same question from a
/// path's side: the two facts the watch dot, the menu row and the toggle all
/// read, in one value so they cannot transpose them differently.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seat {
    /// The tree the shelf stands in.
    pub folder_id: String,
    /// The rung of that tree the shelf wears — the rung whose tracking
    /// decision a toggle from this shelf writes.
    pub rung: String,
}

/// The "who owns this path" questions, asked of one immutable snapshot of the
/// two lists every caller already holds.
pub struct Governance<'a> {
    folders: &'a [WatchedFolder],
    shelves: &'a [Shelf],
}

impl<'a> Governance<'a> {
    /// Borrow the two lists a decision is about. Nothing is cloned: a resolver
    /// lives for one question.
    pub fn new(folders: &'a [WatchedFolder], shelves: &'a [Shelf]) -> Self {
        Self { folders, shelves }
    }

    /// The standing shelf an in-place tree already holds for `ground`: `ground`
    /// IS a folder the library reads in place, or a subfolder inside one, and
    /// the rung its directory names in that tree has a shelf still standing.
    ///
    /// A folder's OWN tree answers for it before a tree it merely stands inside
    /// — the empty rung wins outright, because the folder's own root shelf is
    /// the door the reader meant — and otherwise the first tree the list holds
    /// with a standing shelf at the ground's rung answers. Only READ-AT-PLACE
    /// trees are asked: their shelves are the OS folders themselves, so a second
    /// import of the same ground is at best a no-op and at worst a duplicate of
    /// every book on it. A copying tree is never covered — a stored import is
    /// the library's own second instance, and whether to make another is the
    /// reader's call, asked through the ordinary name question.
    ///
    /// This is the import gate's `covered_of`, lifted whole: the same empty-rung
    /// precedence and the same first-match fallback, so a re-pick still turns
    /// into a reconciliation rather than a second tree.
    pub fn covering(&self, ground: &str) -> Option<Coverage> {
        let mut rung: Option<Coverage> = None;
        for folder in self.folders.iter().filter(|f| f.mode().reads_in_place()) {
            let Some(rel) = rel_under(ground, &folder.root) else {
                continue;
            };
            let Some(shelf_id) = folder.shelf_map.get(&rel) else {
                continue;
            };
            if find_shelf(self.shelves, shelf_id).is_none() {
                continue;
            }
            let coverage = Coverage {
                folder_id: folder.id.clone(),
                rel: rel.clone(),
                shelf_id: shelf_id.clone(),
            };
            if rel.is_empty() {
                return Some(coverage);
            }
            if rung.is_none() {
                rung = Some(coverage);
            }
        }
        rung
    }

    /// The family a ground belongs to but is NOT standing in: the deepest
    /// in-place folder whose root covers `ground` at a rung of its own, when the
    /// rung that folder's ledger names for it is vacant — a slot a removal
    /// emptied, or a departure. `None` when no in-place tree covers the ground,
    /// or the covering tree's rung is alive: an alive rung is [`covering`]'s
    /// answer rather than a family to fold back into.
    ///
    /// The answer is the tree's id and the rung key the ground names in it — the
    /// two facts an import's fold and a move's "put it back" both run on. The
    /// deepest tree wins because a nested in-place import is the closer family:
    /// its rung is the seat the ground's own directory names. This is
    /// [`crate::shelf::family_for`], which now reads through here.
    pub fn family(&self, ground: &str) -> Option<(String, String)> {
        self.folders
            .iter()
            .filter(|f| f.mode().reads_in_place())
            .filter_map(|f| {
                rel_under(ground, &f.root)
                    .filter(|rel| !rel.is_empty())
                    .map(|rel| (rel.len(), f, rel))
            })
            .max_by_key(|(len, _, _)| *len)
            .and_then(|(_, folder, rel)| {
                let vacant = match folder.shelf_map.get(&rel) {
                    Some(id) => !self.shelves.iter().any(|s| s.id == *id),
                    None => true,
                };
                vacant.then(|| (folder.id.clone(), rel))
            })
    }

    /// The rung each in-place folder whose ledger answers for `fp` gives `path`,
    /// or `None` for a folder that names none — a rung the reader deleted since
    /// the walk that placed the file, whose book has left the ground all the
    /// same. An EMPTY list is therefore the only thing the length says: no
    /// ledger is waiting on this fingerprint, so no departure is owed.
    ///
    /// This is the departure's `converts_on_move_to` reduced to its core check —
    /// "which folders placed this content, and where does each say it lives now"
    /// — so the rule about a book leaving its ground reads the same resolver the
    /// import gate does rather than re-walking the folder list itself.
    /// The folder and the rung of its tree that answer for a shelf: the shelf's
    /// OWN seat when the folder's tree cut it — its `rel`, the empty string at
    /// the watched root — and the closest folder shelf above it when the reader
    /// made it inside such a tree. A made shelf is not a rung the disk names,
    /// but it is standing inside the tree, and every question about "the watch
    /// this shelf wears" — the dot, the menu row, the toggle — is the same
    /// question about the seat it stands on.
    ///
    /// `None` for a shelf no read-at-place folder answers for: the reader's own
    /// shelf on the reader's own ground, or a shelf of a COPYING folder, whose
    /// import sheet does not offer the watch either. One walk for the three
    /// readers that used to spell it each for themselves, so the dot a card
    /// draws, the state a menu row shows and the rung a toggle writes cannot
    /// disagree about which seat they asked.
    pub fn seat_of(&self, shelf_id: &str) -> Option<Seat> {
        let shelf = find_shelf(self.shelves, shelf_id)?;
        let (folder_id, rung) = match &shelf.kind {
            ShelfKind::Folder { folder_id, rel } => {
                (folder_id.as_str(), rel.as_deref().unwrap_or(""))
            }
            // Not a rung of any tree: the closest folder shelf above it is the
            // tree it stands inside. `ancestors` is root-first, so the LAST
            // folder shelf in it is the nearest one.
            ShelfKind::Virtual => ancestors(self.shelves, shelf_id)
                .iter()
                .rev()
                .find_map(|each| match &each.kind {
                    ShelfKind::Folder { folder_id, rel } => {
                        Some((folder_id.as_str(), rel.as_deref().unwrap_or("")))
                    }
                    ShelfKind::Virtual => None,
                })?,
        };
        let folder = crate::folder::find(self.folders, folder_id)?;
        folder.mode().reads_in_place().then(|| Seat {
            folder_id: folder_id.to_string(),
            rung: rung.to_string(),
        })
    }

    /// Whether the tree a shelf was cut from tracks the rung that shelf stands
    /// on — the question every watch dot asks, and the one a single flag for
    /// the whole import could only answer about the root.
    ///
    /// The seat is [`seat_of`]'s answer, so a subfolder turned off under a
    /// tracked root stops showing a dot while the tree above it keeps watching,
    /// and a shelf the reader made inside such a tree shows the closest folder
    /// shelf's rung — the seat it stands on.
    ///
    /// `None` for a shelf no read-at-place folder answers for — the reader's
    /// own shelf on the reader's own ground, or a shelf of a COPYING folder,
    /// whose import sheet does not offer the watch either, so there is no dot
    /// to draw.
    pub fn shelf_tracked(&self, shelf_id: &str) -> Option<bool> {
        let seat = self.seat_of(shelf_id)?;
        let folder = crate::folder::find(self.folders, &seat.folder_id)?;
        Some(folder.tracks_rung(&seat.rung))
    }

    pub fn placing_rungs(&self, fp: &Fingerprint, path: &str) -> Vec<Option<String>> {
        self.folders
            .iter()
            .filter(|f| f.mode().reads_in_place() && f.placed.contains(fp))
            .map(|f| f.rungs_for(path).0.map(str::to_string))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::folder::FolderOpts;
    use crate::tracking::TrackingTree;
    use crate::testkit::{folder_shelf, fp_n};
    use std::collections::{BTreeMap, HashSet};

    /// An in-place (or copying) folder rooted at `root`, its `shelf_map` given.
    fn folder(id: &str, root: &str, in_place: bool, map: &[(&str, &str)]) -> WatchedFolder {
        WatchedFolder {
            id: id.into(),
            root: root.into(),
            opts: FolderOpts {
                in_place,
                ..FolderOpts::default()
            },
            placed: HashSet::new(),
            ignored: Vec::new(),
            last_seen: Vec::new(),
            shelf_map: map
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect::<BTreeMap<_, _>>(),
            scanned_ms: 0,
            tracking: TrackingTree::default(),
        }
    }

    /// `/books` read in place, cut into the root ("r"), "Fiction" ("fic") and
    /// "Fiction/SciFi" ("sf"), with one virtual shelf beside the tree.
    fn tree() -> (Vec<WatchedFolder>, Vec<Shelf>) {
        let folders = vec![folder(
            "f1",
            "/books",
            true,
            &[("", "r"), ("Fiction", "fic"), ("Fiction/SciFi", "sf")],
        )];
        let shelves = vec![
            folder_shelf("r", "Books", "f1", None, &[], None),
            folder_shelf("fic", "Fiction", "f1", Some("Fiction"), &[], Some("r")),
            folder_shelf("sf", "SciFi", "f1", Some("Fiction/SciFi"), &[], Some("fic")),
            crate::testkit::plain_shelf("mine", &[]),
        ];
        (folders, shelves)
    }

    #[test]
    fn a_tree_covers_its_own_root_and_a_rung_inside_it() {
        let (folders, shelves) = tree();
        let g = Governance::new(&folders, &shelves);
        // The tree's own root, re-picked: the empty rung, answered outright.
        let root = g.covering("/books").expect("the root shelf stands");
        assert_eq!(root.folder_id, "f1");
        assert_eq!(root.rel, "");
        assert_eq!(root.shelf_id, "r");
        // A rung inside it.
        let rung = g.covering("/books/Fiction/SciFi").expect("the rung stands");
        assert_eq!(rung.rel, "Fiction/SciFi");
        assert_eq!(rung.shelf_id, "sf");
        // A directory the map never named has no standing shelf of its own.
        assert_eq!(g.covering("/books/Unmapped"), None);
        // Ground outside every tree is nobody's.
        assert_eq!(g.covering("/other"), None);
        assert_eq!(g.covering("/bookshelf"), None, "a prefix is not a directory");
    }

    #[test]
    fn the_empty_rung_outranks_a_deeper_one() {
        // Two in-place trees, one nested in the other, both standing: a pick of
        // the OUTER root is the outer tree's own door, not the inner tree's.
        let folders = vec![
            folder("outer", "/books", true, &[("", "r")]),
            folder("inner", "/books/Fiction", true, &[("", "fic")]),
        ];
        let shelves = vec![
            folder_shelf("r", "Books", "outer", None, &[], None),
            folder_shelf("fic", "Fiction", "inner", None, &[], None),
        ];
        let g = Governance::new(&folders, &shelves);
        assert_eq!(g.covering("/books").map(|c| c.folder_id).as_deref(), Some("outer"));
        assert_eq!(g.covering("/books/Fiction").map(|c| c.folder_id).as_deref(), Some("inner"));
    }

    #[test]
    fn a_copying_tree_and_a_dead_rung_cover_nothing() {
        // A copying folder's shelf is the library's own, never a covered ground.
        let copying = vec![folder("c1", "/books", false, &[("", "r")])];
        let shelves = vec![folder_shelf("r", "Books", "c1", None, &[], None)];
        assert_eq!(Governance::new(&copying, &shelves).covering("/books"), None);
        // A map pointer at a shelf that is gone is no standing shelf.
        let (folders, mut dead) = tree();
        dead.retain(|s| s.id != "sf");
        assert_eq!(Governance::new(&folders, &dead).covering("/books/Fiction/SciFi"), None);
        // But the root above it still stands.
        assert!(Governance::new(&folders, &dead).covering("/books").is_some());
    }

    #[test]
    fn the_family_is_the_deepest_tree_whose_rung_for_the_ground_is_free() {
        let (folders, shelves) = tree();
        let g = Governance::new(&folders, &shelves);
        // A ground deep in the tree whose rung the map does not name.
        assert_eq!(
            g.family("/books/Fiction/Deleted"),
            Some(("f1".to_string(), "Fiction/Deleted".to_string()))
        );
        // A rung the map names AND whose shelf stands is covered, not family.
        assert_eq!(g.family("/books/Fiction/SciFi"), None);
        // The tree's own root is not inside its family.
        assert_eq!(g.family("/books"), None);
        // A dead map entry — a rung whose shelf is gone — counts as free.
        let mut dead = folders.clone();
        dead[0].shelf_map.insert("Fiction/SciFi".into(), "gone".into());
        assert_eq!(
            Governance::new(&dead, &shelves).family("/books/Fiction/SciFi"),
            Some(("f1".to_string(), "Fiction/SciFi".to_string()))
        );
    }

    #[test]
    fn of_two_nested_trees_the_longest_relative_rung_answers() {
        // The tie-break is the longest `rel`, which is the tree whose root sits
        // HIGHEST — the ground is deepest relative to it. Preserved exactly from
        // the resolver this replaced; the nested-tree case is also asserted in
        // shelf/mod.rs's own family test.
        let outer = folder("f1", "/books", true, &[("Fiction", "fic")]);
        let inner = folder("f2", "/books/Fiction", true, &[]);
        let shelves = vec![crate::testkit::plain_shelf("mine", &[])];
        let both = vec![outer, inner];
        let g = Governance::new(&both, &shelves);
        assert_eq!(
            g.family("/books/Fiction/SciFi"),
            Some(("f1".to_string(), "Fiction/SciFi".to_string())),
            "the outermost tree's rel is the longest, so it answers"
        );
    }

    #[test]
    fn the_placing_rungs_are_the_folders_that_own_the_content() {
        let fp = fp_n(7);
        let mut placed = folder("f1", "/books", true, &[("", "r"), ("Fiction", "fic")]);
        placed.placed.insert(fp);
        let copying = {
            let mut f = folder("c1", "/dvds", false, &[("", "x")]);
            f.placed.insert(fp);
            f
        };
        let unplaced = folder("f2", "/books/Fiction", true, &[("", "y")]);
        let shelves = vec![folder_shelf("r", "Books", "f1", None, &[], None)];
        let folders = vec![placed, copying, unplaced];
        let g = Governance::new(&folders, &shelves);
        // Only the in-place folder that placed the fingerprint answers, with the
        // rung it gives the address.
        assert_eq!(g.placing_rungs(&fp, "/books/Fiction/dune.pdf"), vec![Some("fic".to_string())]);
        // A folder that placed nothing is not in the list, so an empty answer
        // means no ledger is waiting.
        assert!(g.placing_rungs(&fp_n(99), "/books/Fiction/dune.pdf").is_empty());
    }

    #[test]
    fn a_watch_dot_is_the_rung_s_answer_not_the_tree_s() {
        let (folders, shelves) = tree();
        let g = Governance::new(&folders, &shelves);
        // Nothing tracked yet: no rung of the tree answers on.
        assert_eq!(g.shelf_tracked("r"), Some(false));
        assert_eq!(g.shelf_tracked("sf"), Some(false));

        // Tracking the root lights every rung, which is the whole-tree answer the
        // single flag used to be.
        let mut all = folders.clone();
        all[0].set_tracking("", true);
        let g = Governance::new(&all, &shelves);
        assert_eq!(g.shelf_tracked("r"), Some(true));
        assert_eq!(g.shelf_tracked("fic"), Some(true));
        assert_eq!(g.shelf_tracked("sf"), Some(true), "a rung inherits the root");

        // Turning one rung off is the case the flag could not express: that shelf
        // stops answering on and the tree above it keeps watching.
        let mut partly = all.clone();
        partly[0].set_tracking("Fiction", false);
        let g = Governance::new(&partly, &shelves);
        assert_eq!(g.shelf_tracked("r"), Some(true));
        assert_eq!(g.shelf_tracked("fic"), Some(false), "the rung turned off");
        assert_eq!(g.shelf_tracked("sf"), Some(false), "and everything below it");
        // The flag stays the root's answer, so a downgrade reads the same tree.
        assert!(partly[0].tracked());
        assert!(partly[0].opts.watch);
    }

    #[test]
    fn a_shelf_the_reader_made_answers_for_the_tree_it_stands_inside() {
        let (folders, mut shelves) = tree();
        // "mine" hangs at the root; put a reader's shelf inside the Fiction rung
        // and ask about it. It is not a rung the disk names, but it is standing
        // inside the tree, so the tree's answer for that rung is the answer.
        shelves.push(crate::testkit::shelf("mine2", "Mine", &[], Some("fic")));
        let mut tracked = folders.clone();
        tracked[0].set_tracking("", true);
        let g = Governance::new(&tracked, &shelves);
        assert_eq!(g.shelf_tracked("mine2"), Some(true));
        let mut off = tracked.clone();
        off[0].set_tracking("Fiction", false);
        assert_eq!(Governance::new(&off, &shelves).shelf_tracked("mine2"), Some(false));
    }

    #[test]
    fn a_shelf_nothing_reads_in_place_has_no_dot_to_draw() {
        let (folders, shelves) = tree();
        let g = Governance::new(&folders, &shelves);
        // A shelf of the reader's own at the root level: no folder above it.
        assert_eq!(g.shelf_tracked("mine"), None);
        // A shelf the list does not hold, and the pseudo-shelf, answer the same.
        assert_eq!(g.shelf_tracked("gone"), None);
        assert_eq!(g.shelf_tracked(crate::shelf::ALL_SHELF), None);
        // A COPYING folder's shelf has no watch either way: the import sheet does
        // not offer the watch beside a copy, so there is no dot to draw and no
        // surface that could turn one off.
        let copying = vec![folder("c1", "/dvds", false, &[("", "x")])];
        let copy_shelves = vec![folder_shelf("x", "DVDs", "c1", None, &[], None)];
        assert_eq!(Governance::new(&copying, &copy_shelves).shelf_tracked("x"), None);
    }

    #[test]
    fn a_rung_the_reader_deleted_still_counts_as_placed_but_unnamed() {
        let fp = fp_n(3);
        let mut f = folder("f1", "/books", true, &[("", "r")]);
        f.placed.insert(fp);
        let shelves = vec![folder_shelf("r", "Books", "f1", None, &[], None)];
        let folders = vec![f];
        let g = Governance::new(&folders, &shelves);
        // The address sits in a subfolder the map never named: the folder placed
        // the content, so it is in the list, but it gives this path no rung.
        assert_eq!(g.placing_rungs(&fp, "/books/Deep/x.pdf"), vec![None]);
    }

    #[test]
    fn the_seat_a_shelf_stands_on_is_the_rung_a_toggle_writes() {
        let (folders, mut shelves) = tree();
        shelves.push(crate::testkit::shelf("mine2", "Mine", &[], Some("fic")));
        let g = Governance::new(&folders, &shelves);
        // The root shelf sits on the tree's own rung...
        assert_eq!(
            g.seat_of("r"),
            Some(Seat { folder_id: "f1".into(), rung: "".into() })
        );
        // ...a rung shelf on the rung its rel names, however deep...
        assert_eq!(
            g.seat_of("sf"),
            Some(Seat { folder_id: "f1".into(), rung: "Fiction/SciFi".into() })
        );
        // ...and a shelf the reader made inside the tree on the closest folder
        // shelf's rung, which is the seat it stands on.
        assert_eq!(
            g.seat_of("mine2"),
            Some(Seat { folder_id: "f1".into(), rung: "Fiction".into() })
        );
        // A shelf no read-at-place folder answers for has no seat: the
        // reader's own at the root, a shelf that is gone, and a COPYING
        // folder's all answer `None`, exactly as the dot does.
        assert_eq!(g.seat_of("mine"), None);
        assert_eq!(g.seat_of("gone"), None);
        let copying = vec![folder("c1", "/dvds", false, &[("", "x")])];
        let copy_shelves = vec![folder_shelf("x", "DVDs", "c1", None, &[], None)];
        assert_eq!(Governance::new(&copying, &copy_shelves).seat_of("x"), None);
    }
}
