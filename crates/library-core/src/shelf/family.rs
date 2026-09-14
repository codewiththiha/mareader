//! The shelf-and-folder family rules: which in-place tree a directory
//! belongs to, and which shelf moves are departures from their folder's
//! ground rather than plain re-hangs.

use super::{ancestors, find, Shelf, ShelfKind};

/// The family a ground directory belongs to but is not standing in: the
/// DEEPEST in-place folder whose root covers `ground` at a rung of its own,
/// when the rung that folder's ledger names for it is not standing — a slot a
/// removal emptied, or a departure. `None` when no in-place tree covers the
/// ground, or the covering tree's rung is alive: an alive rung is the import
/// gate's "already in the library" answer rather than a family to fold back
/// into.
///
/// The answer is the tree's id and the rung key the ground names in it — the
/// two facts an import's fold and a move's "put it back" both run on
/// (`crate::ledger` folds through the import's own `reclaim_rung`), from the
/// one lookup every family question reads. The deepest tree wins because a
/// nested in-place import is the closer family: its rung is the seat the
/// ground's own directory names.
pub fn family_for(
    folders: &[crate::folder::WatchedFolder],
    shelves: &[Shelf],
    ground: &str,
) -> Option<(String, String)> {
    crate::governance::Governance::new(folders, shelves).family(ground)
}

/// Whether moving this shelf under `parent` is a departure that owes a copy:
/// a shelf cut from a READ-AT-PLACE folder, leaving the seat the folder's own
/// ledger names for its rung.
///
/// The negatives are as load-bearing as the positive, and each is the book
/// departure's rule read one level up. A VIRTUAL shelf is the reader's own
/// and simply moves. A shelf of a COPYING folder is the library's own already
/// — no ledger waits on its rung, and the move is the membership edit it has
/// always been. A RE-ORDER on the seat a shelf already hangs on copies
/// nothing: the same parent is the same ground. And a move BACK onto the seat
/// is a return rather than a departure: [`reparent`] takes the hand's shelf
/// off the mark and gives the scan its place back.
///
/// Everywhere else is a departure, **including another rung of the very tree
/// that named the shelf.** What ties a read-at-place shelf to its folder is
/// the seat its directory stands on, not membership of the folder's shelf
/// tree: a rung dragged from `Fiction/` to `Sci-Fi/` is no longer where the
/// ledger says it is, and a linked shelf wearing a place it was dragged off
/// is a shelf the next import of that folder collides with instead of coming
/// home to. The seat is the LEDGER's answer — the folder's `shelf_map` at the
/// rung above — which is the same map [`crate::folder::WatchedFolder::rungs_for`]
/// names a file's rung from, so a shelf and the books standing on it cannot
/// disagree about the ground they left.
pub fn departs_on_move(
    shelves: &[Shelf],
    folders: &[crate::folder::WatchedFolder],
    shelf_id: &str,
    parent: Option<&str>,
) -> bool {
    let Some(shelf) = find(shelves, shelf_id) else {
        return false;
    };
    let ShelfKind::Folder { folder_id, .. } = &shelf.kind else {
        return false;
    };
    let Some(folder) = folders
        .iter()
        .find(|f| &f.id == folder_id && f.mode().reads_in_place())
    else {
        return false;
    };
    // A re-order on the seat the shelf already hangs on is the folder's own
    // business, whatever the seat is.
    if shelf.parent.as_deref() == parent {
        return false;
    }
    let key = shelf.kind.rung();
    let seat = crate::folder::parent_key(key).and_then(|rung| folder.shelf_map.get(rung));
    seat.map(String::as_str) != parent
}

/// A batch of requested shelf moves, split into the half that lands as it is
/// and the half that owes the departure's ask — the shelf's own screen for
/// what a drag, a bulk filing or a sibling reorder is about to do.
///
/// One rule per shelf, [`departs_on_move`] against the requested parent, and
/// the departures named in the order the gesture gave. The batch shape hides
/// one exclusion: a departing shelf filed INSIDE another departing one rides
/// with it and asks nothing of its own — the subtree goes with the copy, the
/// way a directory's tree goes with the directory. Such a shelf is in neither
/// half: the outer departure carries it, and landing it separately would pull
/// it out of the very copy it rode.
pub fn departing_moves(
    shelves: &[Shelf],
    folders: &[crate::folder::WatchedFolder],
    ids: &[String],
    parent: Option<&str>,
) -> (Vec<String>, Vec<String>) {
    let mut clean: Vec<String> = Vec::new();
    let mut departing: Vec<String> = Vec::new();
    for id in ids {
        if departs_on_move(shelves, folders, id, parent) {
            departing.push(id.clone());
        } else {
            clean.push(id.clone());
        }
    }
    // The riders, collected before the retain: a filter that read `departing`
    // while the retain held it would be two borrows of one list.
    let riders: Vec<String> = departing
        .iter()
        .filter(|id| {
            ancestors(shelves, id.as_str())
                .iter()
                .any(|each| {
                    each.id != id.as_str() && departing.iter().any(|outer| outer == &each.id)
                })
        })
        .cloned()
        .collect();
    departing.retain(|id| !riders.contains(id));
    (clean, departing)
}
