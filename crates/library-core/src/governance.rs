//! One answer to "who owns this path", instead of four.
//!
//! Four places each walked the watched-folder list and the shelf list to answer
//! a variation of one question: which read-at-place tree's ledger speaks for
//! this ground, and which of its shelves is still standing. They are resolved
//! once here.

use crate::book::Fingerprint;
use crate::folder::{rel_under, WatchedFolder};
use crate::shelf::{ancestors, find as find_shelf, Shelf, ShelfKind};

/// The folder, the rung and the standing shelf that answer for a path. A value
/// rather than a tuple: the three facts are easy to transpose, and a gate that
/// read `.1` for a shelf id would be a bug no type catches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Coverage {
    pub folder_id: String,
    pub rel: String,
    pub shelf_id: String,
}

/// The seat a SHELF stands on in a read-at-place tree: which folder's ground it
/// wears, and which rung of that folder's tree it is. The shelf-shaped twin of
/// [`Coverage`], which answers the same question from a path's side.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seat {
    pub folder_id: String,
    /// The rung of that tree the shelf wears — the rung whose tracking decision a toggle from this shelf writes.
    pub rung: String,
}

/// The "who owns this path" questions, asked of one snapshot of the two lists every caller already holds.
pub struct Governance<'a> {
    folders: &'a [WatchedFolder],
    shelves: &'a [Shelf],
}

impl<'a> Governance<'a> {
    /// Borrow the two lists a decision is about; nothing is cloned.
    pub fn new(folders: &'a [WatchedFolder], shelves: &'a [Shelf]) -> Self {
        Self { folders, shelves }
    }

    /// The standing shelf an in-place tree already holds for `ground`: `ground` IS a
    /// folder the library reads in place, or a subfolder inside one, and the rung its
    /// directory names in that tree has a shelf still standing.
    ///
    /// A folder's OWN tree answers for it before a tree it merely stands inside — the
    /// empty rung wins outright — and otherwise the tree whose root sits highest
    /// does.
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

    /// The family a ground belongs to but is NOT standing in: the deepest in-place
    /// folder whose root covers `ground` at a rung of its own, when the rung that
    /// folder's ledger names for it is vacant — a slot a removal emptied, or a
    /// departure. `None` when the covering tree's rung is alive: that is
    /// [`covering`]'s answer rather than a family to fold back into.
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

    /// The rung each in-place folder whose ledger answers for `fp` gives `path`, or
    /// `None` for a folder that names none — a rung the reader deleted since the walk
    /// that placed the file. An EMPTY list is the only thing the length says: no
    /// ledger is waiting on this fingerprint, so no departure is owed.
    pub fn seat_of(&self, shelf_id: &str) -> Option<Seat> {
        let shelf = find_shelf(self.shelves, shelf_id)?;
        let (folder_id, rung) = match &shelf.kind {
            ShelfKind::Folder { folder_id, rel } => {
                (folder_id.as_str(), rel.as_deref().unwrap_or(""))
            }
            // Not a rung of any tree: the closest folder shelf above it is the tree it stands inside.
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

    /// Whether the tree a shelf was cut from tracks the rung that shelf stands on —
    /// the question every watch dot asks, which a single flag for the whole import
    /// could only answer about the root.
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
        let root = g.covering("/books").expect("the root shelf stands");
        assert_eq!(root.folder_id, "f1");
        assert_eq!(root.rel, "");
        assert_eq!(root.shelf_id, "r");
        let rung = g.covering("/books/Fiction/SciFi").expect("the rung stands");
        assert_eq!(rung.rel, "Fiction/SciFi");
        assert_eq!(rung.shelf_id, "sf");
        assert_eq!(g.covering("/books/Unmapped"), None);
        assert_eq!(g.covering("/other"), None);
        assert_eq!(g.covering("/bookshelf"), None, "a prefix is not a directory");
    }

    #[test]
    fn the_empty_rung_outranks_a_deeper_one() {
        // Two in-place trees, one nested in the other: a pick of the OUTER root is the outer tree's own door.
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
        let copying = vec![folder("c1", "/books", false, &[("", "r")])];
        let shelves = vec![folder_shelf("r", "Books", "c1", None, &[], None)];
        assert_eq!(Governance::new(&copying, &shelves).covering("/books"), None);
        let (folders, mut dead) = tree();
        dead.retain(|s| s.id != "sf");
        assert_eq!(Governance::new(&folders, &dead).covering("/books/Fiction/SciFi"), None);
        assert!(Governance::new(&folders, &dead).covering("/books").is_some());
    }

    #[test]
    fn the_family_is_the_deepest_tree_whose_rung_for_the_ground_is_free() {
        let (folders, shelves) = tree();
        let g = Governance::new(&folders, &shelves);
        assert_eq!(
            g.family("/books/Fiction/Deleted"),
            Some(("f1".to_string(), "Fiction/Deleted".to_string()))
        );
        assert_eq!(g.family("/books/Fiction/SciFi"), None);
        assert_eq!(g.family("/books"), None);
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
        // HIGHEST. The nested-tree case is also asserted in shelf/mod.rs's family test.
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
        assert_eq!(g.placing_rungs(&fp, "/books/Fiction/dune.pdf"), vec![Some("fic".to_string())]);
        assert!(g.placing_rungs(&fp_n(99), "/books/Fiction/dune.pdf").is_empty());
    }

    #[test]
    fn a_watch_dot_is_the_rung_s_answer_not_the_tree_s() {
        let (folders, shelves) = tree();
        let g = Governance::new(&folders, &shelves);
        assert_eq!(g.shelf_tracked("r"), Some(false));
        assert_eq!(g.shelf_tracked("sf"), Some(false));

        let mut all = folders.clone();
        all[0].set_tracking("", true);
        let g = Governance::new(&all, &shelves);
        assert_eq!(g.shelf_tracked("r"), Some(true));
        assert_eq!(g.shelf_tracked("fic"), Some(true));
        assert_eq!(g.shelf_tracked("sf"), Some(true), "a rung inherits the root");

        // Turning one rung off is the case the flag could not express: that shelf stops answering on.
        let mut partly = all.clone();
        partly[0].set_tracking("Fiction", false);
        let g = Governance::new(&partly, &shelves);
        assert_eq!(g.shelf_tracked("r"), Some(true));
        assert_eq!(g.shelf_tracked("fic"), Some(false), "the rung turned off");
        assert_eq!(g.shelf_tracked("sf"), Some(false), "and everything below it");
        assert!(partly[0].tracked());
        assert!(partly[0].opts.watch);
    }

    #[test]
    fn a_shelf_the_reader_made_answers_for_the_tree_it_stands_inside() {
        let (folders, mut shelves) = tree();
        // A shelf inside the Fiction rung is not a rung the disk names, but the tree's answer for that rung is its answer.
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
        assert_eq!(g.shelf_tracked("mine"), None);
        assert_eq!(g.shelf_tracked("gone"), None);
        assert_eq!(g.shelf_tracked(crate::shelf::ALL_SHELF), None);
        // A COPYING folder's shelf has no watch either way: the import sheet does not offer one beside a copy.
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
        assert_eq!(g.placing_rungs(&fp, "/books/Deep/x.pdf"), vec![None]);
    }

    #[test]
    fn the_seat_a_shelf_stands_on_is_the_rung_a_toggle_writes() {
        let (folders, mut shelves) = tree();
        shelves.push(crate::testkit::shelf("mine2", "Mine", &[], Some("fic")));
        let g = Governance::new(&folders, &shelves);
        assert_eq!(
            g.seat_of("r"),
            Some(Seat { folder_id: "f1".into(), rung: "".into() })
        );
        assert_eq!(
            g.seat_of("sf"),
            Some(Seat { folder_id: "f1".into(), rung: "Fiction/SciFi".into() })
        );
        assert_eq!(
            g.seat_of("mine2"),
            Some(Seat { folder_id: "f1".into(), rung: "Fiction".into() })
        );
        assert_eq!(g.seat_of("mine"), None);
        assert_eq!(g.seat_of("gone"), None);
        let copying = vec![folder("c1", "/dvds", false, &[("", "x")])];
        let copy_shelves = vec![folder_shelf("x", "DVDs", "c1", None, &[], None)];
        assert_eq!(Governance::new(&copying, &copy_shelves).seat_of("x"), None);
    }
}
