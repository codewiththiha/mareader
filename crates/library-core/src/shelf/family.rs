//! The shelf-and-folder family rules: which in-place tree a directory belongs
//! to, and which shelf moves are departures from their folder's ground rather
//! than plain re-hangs.

use super::{ancestors, find, Shelf, ShelfKind};

/// The family a ground directory belongs to but is not standing in —
/// [`crate::governance::Governance::family`] with the folder and shelf lists
/// passed in.
pub fn family_for(
    folders: &[crate::folder::WatchedFolder],
    shelves: &[Shelf],
    ground: &str,
) -> Option<(String, String)> {
    crate::governance::Governance::new(folders, shelves).family(ground)
}

/// Whether moving this shelf under `parent` is a departure that owes a copy:
/// a shelf cut from a read-at-place folder, leaving the seat the folder's
/// ledger names for its rung.
///
/// Each negative is the book departure's rule one level up: a shelf that is
/// nobody's rung is the reader's own, a copying folder's shelf is the
/// library's own already, and a re-order on the current seat moves nothing.
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
    // A re-order on the current seat is the folder's own business.
    if shelf.parent.as_deref() == parent {
        return false;
    }
    let key = shelf.kind.rung();
    let seat = crate::folder::parent_key(key).and_then(|rung| folder.shelf_map.get(rung));
    seat.map(String::as_str) != parent
}

/// Split a batch of requested shelf moves into the half that lands as it is
/// and the half that owes the departure's ask, in the order the gesture gave.
/// A departing shelf nested inside another departing shelf rides along rather
/// than asking twice.
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
    // Riders collected before the retain: filtering against `departing`
    // while the retain holds it would be two borrows of one list.
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
