//! The shelf-and-folder family rules: which in-place tree a directory belongs
//! to, and which shelf moves are departures from their folder's ground rather
//! than plain re-hangs.

use super::{ancestors, find, Shelf, ShelfKind};

/// The family a ground directory belongs to but is not standing in: the DEEPEST
/// in-place folder whose root covers `ground` at a rung of its own, when the
/// rung that folder's ledger names for it is not standing — a slot a removal
/// emptied, or a departure. `None` when no in-place tree covers the ground, or
/// the covering tree's rung is alive.
pub fn family_for(
    folders: &[crate::folder::WatchedFolder],
    shelves: &[Shelf],
    ground: &str,
) -> Option<(String, String)> {
    crate::governance::Governance::new(folders, shelves).family(ground)
}

/// Whether moving this shelf under `parent` is a departure that owes a copy: a
/// shelf cut from a READ-AT-PLACE folder, leaving the seat the folder's own
/// ledger names for its rung.
///
/// Each negative is the book departure's rule read one level up: a shelf that is
/// nobody's rung — one the reader made, and one a move already took off its tree —
/// is the reader's own, a shelf of a COPYING folder is the library's own once more,
/// and a shelf with no folder above it has no ground to leave.
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
    // A re-order on the seat the shelf already hangs on is the folder's own business.
    if shelf.parent.as_deref() == parent {
        return false;
    }
    let key = shelf.kind.rung();
    let seat = crate::folder::parent_key(key).and_then(|rung| folder.shelf_map.get(rung));
    seat.map(String::as_str) != parent
}

/// A batch of requested shelf moves, split into the half that lands as it is and
/// the half that owes the departure's ask. One rule per shelf,
/// [`departs_on_move`] against the requested parent, and the departures named in
/// the order the gesture gave.
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
