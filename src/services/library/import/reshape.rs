//! Re-shaping a tree: the shelf-per-folder answer written onto the row that
//! reads the ground, and the re-filing that answer owes.
//!
//! A pick answered the other way than the ground was imported with moves books
//! between rungs — it never re-reads the disk. The walk in [`super::folder`]
//! brings home what is new; this is what puts what is already here onto the
//! rungs the answer now names.

use std::collections::HashSet;

use leptos::prelude::*;

use library_core::book::index_by_id;
use library_core::folder::{
    self as folder_ops, dir_of_rung, key_in_zone, rel_under, FolderOpts, WatchedFolder,
};
use library_core::scan::subfolder_of;
use library_core::shelf::{self as shelves_ops, Shelf};

use super::folder::{chain_for, page_shelves, write_folder};
use crate::state::AppState;

/// The row and rung the shelf-shape question moved, when the reader answered
/// it the other way than that ground was imported with: the rung a fold plan
/// names, else the rung the pick lit in the row that reads this run's ground.
/// `None` when the answers agree, when the row copies its books, and when no
/// row reads the ground yet.
///
/// A rescan hands a row its own answers back, so only a re-import can move
/// this.
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

/// The shelf a rung of the shape stands on: the one the tree already wears
/// for it, or the mint of one the shape cuts — a hand that took the shelf out
/// leaves a pointer, not a seat.
fn seat_for(
    state: AppState,
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

/// Whether a rung's seat is there to file onto: a shelf the library holds, or
/// one this re-shape has minted. Both count — re-minting over its own mint
/// would grow a twin beside the rung it just cut.
fn seat_stands(shelves: &[Shelf], minted: &[Shelf], id: &str) -> bool {
    minted.iter().any(|shelf| shelf.id == id) || shelves_ops::find(shelves, id).is_some()
}

/// Write the shelf shape one ground answers with onto the row that reads it,
/// before the walk that acts on it. The answer stands for the ground it was
/// given on: the row's root for a pick of the tree's own ground, the rung
/// below it for a pick under that.
pub(super) fn write_shape(state: AppState, row_id: &str, rung: &str, grouped: bool) {
    state.library.folders.update(|folders| {
        if let Some(folder) = folder_ops::find_mut(folders, row_id) {
            folder.set_shape(rung, grouped);
        }
    });
}

/// The whole of one tree's books onto `seat`, and the shelves the one-shelf
/// answer has no place for taken out: the flattening a re-import asks for,
/// and the one an adopting tree takes a member in by. Every rung of `from`
/// goes except the one that is `seat`. Books move, readers do not: a shelf
/// the reader made inside one comes up to `seat` with its books.
pub(super) fn flatten_rungs(state: AppState, from: &str, seat: &str) {
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
        // A set rather than a growing list: the whole tree's books pass
        // through here, and membership was a scan per book.
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

/// Re-files the books of the ground the shape answer was about onto the rungs
/// the shape now names: shelf-per-folder puts each under the rung its own
/// address wears; one-shelf brings them onto the rung the ground answers for
/// and takes out the rungs that answer has no place for. A rung the tree
/// already stands on is reused, and the tree above the answered ground is
/// left where it stands — the reader answered for the ground, not the tree.
///
/// Answers the shelf the ground's books came home to, and `None` when nothing
/// moved.
pub(super) fn reshape_the_tree(
    state: AppState,
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
    // A tree whose root shelf is out of the library has no ground to
    // re-file onto: the answer is the reader's when the shelf comes back with
    // the folder's books.
    if folder
        .shelf_map
        .get("")
        .is_none_or(|id| shelves_ops::find(&shelves, id).is_none())
    {
        return None;
    }
    let root = folder.root.clone();
    let ground = dir_of_rung(&root, rung);
    // The rungs the answer has no place for: the ground's rung and the ones
    // below it that no shape cuts any more. Their books come home with the
    // ground's, and a reader-made shelf inside one comes up to `home`.
    let mut hurt: HashSet<String> = HashSet::new();
    for (key, id) in &own {
        if key_in_zone(key, rung) && !folder.cuts(key) {
            hurt.insert((*id).to_string());
        }
    }
    let mut minted: Vec<Shelf> = Vec::new();
    // Where the ground's books come home: the rung the shape names for it
    // now, or the rung above when no shape cuts it any more.
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
                    // Every address under the ground is under the tree: the
                    // two `rel_under` calls answer for one directory chain.
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
    // The map is the tree's own idea of where its rungs stand: a rung the
    // answer took out cannot be filed onto again.
    folder.shelf_map.retain(|_, id| !hurt.contains(id.as_str()));
    Some(home)
}

/// [`reshape_the_tree`] for a row the run does not hold: the tree a pick was
/// folded into is read fresh, moved, written back and persisted — the run's
/// clone of that ledger is the fold's own write and nothing else may land on
/// top of it.
pub(super) fn reshape_row(
    state: AppState,
    folder_id: &str,
    rung: &str,
    grouped: bool,
    now: u64,
) -> Option<String> {
    let mut folder = state.library.folder(folder_id)?;
    let seat = reshape_the_tree(state, &mut folder, rung, grouped, now);
    write_folder(state, folder);
    crate::storage::persist_library(state.library);
    seat
}
