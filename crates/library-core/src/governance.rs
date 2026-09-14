//! One answer to "who owns this path", instead of four.
//!
//! Four places each walked the watched-folder list and the shelf list to answer
//! a variation of one question: which read-at-place tree's ledger speaks for
//! this ground, and which of its shelves is still standing. They are resolved
//! once here.

use crate::book::Fingerprint;
use crate::folder::{rel_under, FolderMode, WatchedFolder};
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

/// The seat a SHELF stands on in a tree: which folder's ground it wears, and which
/// rung of that folder's tree it is. The shelf-shaped twin of [`Coverage`], which
/// answers the same question from a path's side.
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

    /// The shelf a folder's ledger names for one of its rungs, when that shelf is still
    /// standing: the one read behind both [`Self::covering`] and [`Self::family`].
    fn standing_rung<'f>(&self, folder: &'f WatchedFolder, rung: &str) -> Option<&'f String> {
        folder
            .shelf_map
            .get(rung)
            .filter(|id| self.shelves.iter().any(|s| s.id == **id))
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
            let Some(shelf_id) = self.standing_rung(folder, &rel) else {
                continue;
            };
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
    /// folder whose root covers `ground` at a rung of its own, when that folder's
    /// ledger names no standing shelf for the rung — a slot a removal emptied, or a
    /// departure — while the shelf its own root is kept on still stands. `None` when
    /// the covering tree's rung is alive: that is [`covering`]'s answer rather than a
    /// family to fold back into.
    ///
    /// A tree whose root shelf the reader has taken out is no family: ground under it
    /// is a start of its own, not a rung of a tree nothing stands on.
    pub fn family(&self, ground: &str) -> Option<(String, String)> {
        self.folders
            .iter()
            .filter(|f| f.mode().reads_in_place())
            .filter(|f| self.standing_rung(f, "").is_some())
            .filter_map(|f| {
                rel_under(ground, &f.root)
                    .filter(|rel| !rel.is_empty())
                    .map(|rel| (rel.len(), f, rel))
            })
            .max_by_key(|(len, _, _)| *len)
            .and_then(|(_, folder, rel)| {
                self.standing_rung(folder, &rel)
                    .is_none()
                    .then(|| (folder.id.clone(), rel))
            })
    }

    /// The tree a shelf stands in: which folder's ground it wears, and which rung of
    /// that folder's tree it is — a rung's own `rel`, or the rung of the closest folder
    /// shelf above a shelf the reader made. `None` for a shelf no tree answers for: one
    /// the reader made at the root, and one that LEFT its tree
    /// ([`ShelfKind::Departed`]), whose books the library keeps copies of.
    pub fn tree_of(&self, shelf_id: &str) -> Option<Seat> {
        let shelf = find_shelf(self.shelves, shelf_id)?;
        let (folder_id, rung) = match &shelf.kind {
            ShelfKind::Folder { folder_id, rel } => {
                (folder_id.as_str(), rel.as_deref().unwrap_or(""))
            }
            // A shelf a move took off its tree stands where the reader put it and not on ground
            // the disk names, so no tree answers for it.
            ShelfKind::Departed => return None,
            // Not a rung of any tree: the closest folder shelf above it is the tree it stands
            // inside — and a shelf that left its tree ends that ground, so what stands inside
            // one stands on the reader's own.
            ShelfKind::Virtual => ancestors(self.shelves, shelf_id)
                .iter()
                .rev()
                .take_while(|each| each.kind != ShelfKind::Departed)
                .find_map(|each| match &each.kind {
                    ShelfKind::Folder { folder_id, rel } => {
                        Some((folder_id.as_str(), rel.as_deref().unwrap_or("")))
                    }
                    _ => None,
                })?,
        };
        crate::folder::find(self.folders, folder_id)?;
        Some(Seat {
            folder_id: folder_id.to_string(),
            rung: rung.to_string(),
        })
    }

    /// [`Self::tree_of`] for the trees that READ IN PLACE: the seat a watch toggle writes to and
    /// a dot reads. A copying tree answers no seat, because tracking is a promise about the tree
    /// the books are read from.
    pub fn seat_of(&self, shelf_id: &str) -> Option<Seat> {
        let seat = self.tree_of(shelf_id)?;
        let folder = crate::folder::find(self.folders, &seat.folder_id)?;
        folder.mode().reads_in_place().then_some(seat)
    }

    /// Where a shelf's books live, when the library can answer: [`FolderMode::Copy`] for a shelf
    /// whose books the library keeps copies of — cut from a folder imported that way, or taken off
    /// a read-at-place tree by a move that paid the copy — the tree's own mode for a shelf that is
    /// a door onto a directory on disk, and `None` for a shelf no folder answers for.
    pub fn mode_of(&self, shelf_id: &str) -> Option<FolderMode> {
        if find_shelf(self.shelves, shelf_id)?.kind == ShelfKind::Departed {
            return Some(FolderMode::Copy);
        }
        let seat = self.tree_of(shelf_id)?;
        crate::folder::find(self.folders, &seat.folder_id).map(|f| f.mode())
    }

    /// Whether the tree a shelf was cut from tracks the rung that shelf stands on —
    /// the question every watch dot asks, which a single flag for the whole import
    /// could only answer about the root.
    pub fn shelf_tracked(&self, shelf_id: &str) -> Option<bool> {
        let seat = self.seat_of(shelf_id)?;
        let folder = crate::folder::find(self.folders, &seat.folder_id)?;
        Some(folder.tracks_rung(&seat.rung))
    }

    /// The rung each in-place folder whose ledger answers for `fp` gives `path`, or
    /// `None` for a folder that names none — a rung the reader deleted since the walk
    /// that placed the file. An EMPTY list is the only thing the length says: no
    /// ledger is waiting on this fingerprint, so no departure is owed.
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
    use crate::folder::{FolderMode, FolderOpts};
    use crate::tracking::TrackingTree;
    use crate::testkit::{departed_shelf, folder_shelf, fp_n};
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
        let outer = folder("f1", "/books", true, &[("", "or"), ("Fiction", "fic")]);
        let inner = folder("f2", "/books/Fiction", true, &[("", "ir")]);
        let shelves = vec![
            folder_shelf("or", "Books", "f1", None, &[], None),
            folder_shelf("ir", "Fiction", "f2", None, &[], None),
        ];
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
    fn a_shelf_a_move_took_off_its_tree_answers_no_tree() {
        let (folders, mut shelves) = tree();
        // The reader dragged SciFi out of Fiction and paid the copy, and filed it inside the rung
        // it left: nothing answers for it any more, while the rung it sits in answers for itself.
        shelves.push(departed_shelf("moved", &[], Some("fic")));
        let mut tracked = folders.clone();
        tracked[0].set_tracking("", true);
        let g = Governance::new(&tracked, &shelves);
        assert_eq!(g.tree_of("moved"), None);
        assert_eq!(g.seat_of("moved"), None);
        assert_eq!(g.shelf_tracked("moved"), None, "no ground, no dot");
        assert_eq!(g.mode_of("moved"), Some(FolderMode::Copy));
        assert_eq!(g.tree_of("fic").map(|seat| seat.rung), Some("Fiction".to_string()));
    }

    #[test]
    fn a_shelf_inside_a_departed_one_stands_on_the_reader_s_own() {
        let (folders, mut shelves) = tree();
        shelves.push(departed_shelf("moved", &[], Some("fic")));
        shelves.push(crate::testkit::shelf("mine2", "Mine", &[], Some("moved")));
        let g = Governance::new(&folders, &shelves);
        assert_eq!(g.tree_of("mine2"), None, "the ground ends with the shelf that left it");
    }

    #[test]
    fn a_copying_tree_gives_the_badge_but_no_seat() {
        let copying = vec![folder("c1", "/books", false, &[("", "r")])];
        let shelves = vec![folder_shelf("r", "Books", "c1", None, &[], None)];
        let g = Governance::new(&copying, &shelves);
        assert_eq!(g.mode_of("r"), Some(FolderMode::Copy), "the library keeps its own");
        assert_eq!(g.seat_of("r"), None, "and there is no tracking to promise");
        assert_eq!(g.tree_of("r").map(|seat| seat.rung), Some(String::new()));
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
