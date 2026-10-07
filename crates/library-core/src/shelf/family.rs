//! Which in-place tree a directory belongs to, and which moves depart.

use super::{Shelf, ShelfKind, ancestors, find};

/// The family a ground belongs to but is not standing in.
pub fn family_for(
    folders: &[crate::folder::WatchedFolder],
    shelves: &[Shelf],
    ground: &str,
) -> Option<(String, String)> {
    crate::governance::Governance::new(folders, shelves).family(ground)
}

/// Whether a shelf move is a departure that owes a copy.
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

/// Split shelf moves into the half that lands and the half that asks.
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
    // Riders collected before the retain.
    let riders: Vec<String> = departing
        .iter()
        .filter(|id| {
            ancestors(shelves, id.as_str()).iter().any(|each| {
                each.id != id.as_str() && departing.iter().any(|outer| outer == &each.id)
            })
        })
        .cloned()
        .collect();
    departing.retain(|id| !riders.contains(id));
    (clean, departing)
}
