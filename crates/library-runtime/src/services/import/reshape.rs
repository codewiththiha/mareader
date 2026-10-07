//! Re-shaping a tree: the pick written onto the row that reads it.

use std::collections::HashSet;

use leptos::prelude::*;

use library_core::book::index_by_id;
use library_core::folder::{
    self as folder_ops, FolderOpts, WatchedFolder, dir_of_rung, key_in_zone, rel_under,
};
use library_core::scan::subfolder_of;
use library_core::shelf::{self as shelves_ops, Shelf};

use super::folder::{chain_for, page_shelves, write_folder};

/// The row and rung the pick moved, or `None` when they agree.
pub(super) fn shape_moved(
    folders: &[WatchedFolder],
    root: &str,
    folded: Option<&str>,
    rung: &str,
    opts: &FolderOpts,
) -> Option<(String, String)> {
    let row = match folded {
        Some(tree_id) => folder_ops::find(folders, tree_id)?,
        None => folders.iter().find(|row| row.root == root)?,
    };
    (row.mode().reads_in_place() && row.shape_at(rung) != opts.groups)
        .then(|| (row.id.clone(), rung.to_string()))
}

/// The shelf a rung stands on: the tree's own, or a fresh mint.
fn seat_for(
    state: crate::context::LibraryContext,
    folder: &mut WatchedFolder,
    key: &str,
    now: u64,
    minted: &mut Vec<Shelf>,
) -> String {
    let shelves = state.library.shelves.get_untracked();
    if let Some(id) = folder.shelf_map.get(key).cloned()
        && seat_stands(&shelves, minted, &id)
    {
        return id;
    }
    folder.shelf_map.remove(key);
    let root = folder.root.clone();
    chain_for(folder, key, now, &root, &None, false, minted)
}

/// Whether a rung's seat is there: the library's, or this run's mint.
fn seat_stands(shelves: &[Shelf], minted: &[Shelf], id: &str) -> bool {
    minted.iter().any(|shelf| shelf.id == id) || shelves_ops::find(shelves, id).is_some()
}

/// Write the shape onto the row that reads the ground, before the walk.
pub(super) fn write_shape(
    state: crate::context::LibraryContext,
    row_id: &str,
    rung: &str,
    grouped: bool,
) {
    state.library.folders.update(|folders| {
        if let Some(folder) = folder_ops::find_mut(folders, row_id) {
            folder.set_shape(rung, grouped);
        }
    });
}

/// All of one tree's books onto `seat`; every other rung goes.
pub(super) fn flatten_rungs(state: crate::context::LibraryContext, from: &str, seat: &str) {
    state.library.shelves.update(|shelves| {
        let own: Vec<String> = shelves_ops::rungs_of(shelves, from)
            .into_values()
            .map(String::from)
            .collect();
        let going: HashSet<String> = own
            .iter()
            .filter(|id| id.as_str() != seat)
            .cloned()
            .collect();
        // A set: membership was a scan per book.
        let mut held: HashSet<String> = HashSet::new();
        for rung in &own {
            let Some(shelf) = shelves_ops::find(shelves, rung) else {
                continue;
            };
            held.extend(shelf.books.iter().cloned());
        }
        if let Some(shelf) = shelves_ops::find_mut(shelves, seat) {
            for book_id in held {
                shelves_ops::shelf_add(shelf, &book_id);
            }
        }
        for shelf in shelves.iter_mut() {
            if shelf
                .parent
                .as_deref()
                .is_some_and(|parent| going.contains(parent))
            {
                shelf.parent = Some(seat.to_string());
            }
        }
        shelves.retain(|shelf| !going.contains(&shelf.id));
    });
}

/// Re-file the answered ground's books onto the rungs the shape names.
pub(super) fn reshape_the_tree(
    state: crate::context::LibraryContext,
    folder: &mut WatchedFolder,
    rung: &str,
    grouped: bool,
    now: u64,
) -> Option<String> {
    folder.set_shape(rung, grouped);
    let shelves = state.library.shelves.get_untracked();
    let own = shelves_ops::rungs_of(&shelves, &folder.id);
    if own.is_empty() {
        return None;
    }
    // No root shelf, no ground to re-file onto.
    if folder
        .shelf_map
        .get("")
        .is_none_or(|id| shelves_ops::find(&shelves, id).is_none())
    {
        return None;
    }
    let root = folder.root.clone();
    let ground = dir_of_rung(&root, rung);
    // Rungs no shape cuts any more; their books come home.
    let mut hurt: HashSet<String> = HashSet::new();
    for (key, id) in &own {
        if key_in_zone(key, rung) && !folder.cuts(key) {
            hurt.insert((*id).to_string());
        }
    }
    let mut minted: Vec<Shelf> = Vec::new();
    // Where the ground's books come home.
    let home_key = folder.rung_for(rung);
    let home = seat_for(state, folder, &home_key, now, &mut minted);
    let books = state.library.books.get_untracked();
    let by_id = index_by_id(&books);
    let mut moving: Vec<(String, String)> = Vec::new();
    for id in own.values() {
        let Some(held) = shelves_ops::find(&shelves, id) else {
            continue;
        };
        let doomed = hurt.contains(*id);
        for book_id in &held.books {
            let book = by_id.get(book_id.as_str()).and_then(|row| row.book());
            let in_ground = book.is_some_and(|book| rel_under(book.path(), &ground).is_some());
            let seat = if in_ground {
                match book.and_then(|book| rel_under(book.path(), &root)) {
                    Some(rel) => {
                        let key = folder.rung_for(subfolder_of(&rel));
                        seat_for(state, folder, &key, now, &mut minted)
                    }
                    // Under the ground is under the tree.
                    None => home.clone(),
                }
            } else if doomed {
                home.clone()
            } else {
                continue;
            };
            if seat.as_str() != *id {
                moving.push((book_id.clone(), seat));
            }
        }
    }
    if moving.is_empty() && hurt.is_empty() {
        return None;
    }
    page_shelves(state, minted);
    state.library.shelves.update(|shelves| {
        let moved: HashSet<&str> = moving.iter().map(|(id, _)| id.as_str()).collect();
        for id in own.values() {
            let Some(shelf) = shelves_ops::find_mut(shelves, id) else {
                continue;
            };
            shelf.books.retain(|book| !moved.contains(book.as_str()));
        }
        for (book_id, seat) in &moving {
            let Some(shelf) = shelves_ops::find_mut(shelves, seat) else {
                continue;
            };
            shelves_ops::shelf_add(shelf, book_id);
        }
        for shelf in shelves.iter_mut() {
            if shelf
                .parent
                .as_deref()
                .is_some_and(|parent| hurt.contains(parent))
            {
                shelf.parent = Some(home.clone());
            }
        }
        shelves.retain(|shelf| !hurt.contains(shelf.id.as_str()));
    });
    // A rung the answer took out cannot be filed onto again.
    folder.shelf_map.retain(|_, id| !hurt.contains(id.as_str()));
    Some(home)
}

/// [`reshape_the_tree`] for a row the run does not hold.
pub(super) fn reshape_row(
    state: crate::context::LibraryContext,
    folder_id: &str,
    rung: &str,
    grouped: bool,
    now: u64,
) -> Option<String> {
    let mut folder = state.library.folder(folder_id)?;
    let seat = reshape_the_tree(state, &mut folder, rung, grouped, now);
    write_folder(state, folder);
    crate::services::persist_library(state.library);
    seat
}
