//! The reader's own shelves: made, named, nested, reordered and taken apart. A shelf holds
//! ids and never held a byte, so every operation here is a membership edit — with the
//! departure's question asked of the moves that carry a read-at-place shelf off its folder's
//! seat.

use leptos::prelude::*;

use library_core::book::Row;
use library_core::folder as folder_ops;
use library_core::id;
use library_core::shelf::{self as shelf, Shelf, ALL_SHELF};

use crate::state::AppState;
use crate::time::now_ms;

use super::asking::ask_move_shelf;
use super::shelf_departure::{screen_shelf_moves, SeamSide, ShelfSeam};

/// `parent` is where the shelf hangs: `None` is the level the page is on, and `Some` is a
/// shelf the reader named — a shelf made from inside a folder is that folder being subdivided,
/// so the parent is the folder that was asked rather than the level the page happens to be
/// on.
pub fn create_shelf_and_enter(state: AppState, parent: Option<&str>) -> String {
    let id = match parent {
        Some(parent) => create_shelf_at(state, Some(parent.to_string())),
        None => create_shelf_here(state),
    };
    state.library.shelf.set(id.clone());
    crate::storage::persist_library(state.library);
    id
}

/// A new shelf under the open level, without drilling into it: the reader
/// asked for the books to be on another shelf, not to navigate there.
pub fn create_shelf_here(state: AppState) -> String {
    let at = state.library.shelf.get_untracked();
    let parent = (at != ALL_SHELF).then_some(at);
    create_shelf_at(state, parent)
}

fn create_shelf_at(state: AppState, parent: Option<String>) -> String {
    let id = library_core::id::next_shelf_id(now_ms());
    let made = id.clone();
    state.library.shelves.update(|shelves| {
        shelves.push(Shelf::virtual_shelf(made, "New shelf", parent));
    });
    // The folder list derives off `shelves` and the search filter off `query`, and both are
    // read tracked inside the page's own derives — a new shelf re-runs them on its own; the
    // old self-set of the query (a notification forced through a write of the same value)
    // was belt-and-braces the derives did not need.
    id
}

/// The cycle check is `library_core::shelf::reparent`'s, not the caller's: a
/// folder filed inside itself renders on no level and can never be opened
/// again, so the rule has to hold for every caller.
/// Screen a shelf move against the departure rule and ask about the ones that
/// owe a copy. Answers the shelves that can move now; the departing ones are on
/// the sheet and come back through [`super::asking`]'s own answer.
fn screened(
    state: AppState,
    ids: &[String],
    parent: Option<&str>,
    seam: Option<ShelfSeam>,
) -> Vec<String> {
    let (clean, departing) = screen_shelf_moves(state, ids, parent);
    if !departing.is_empty() {
        ask_move_shelf(state, departing, parent.map(str::to_string), seam);
    }
    clean
}

pub fn nest_shelf(state: AppState, folder_id: &str, parent: Option<&str>) -> bool {
    let one = [folder_id.to_string()];
    let clean = screened(state, &one, parent, None);
    if clean.is_empty() {
        return false;
    }
    let mut moved = false;
    state.library.shelves.update(|shelves| {
        moved = shelf::reparent(shelves, folder_id, parent);
    });
    if moved {
        crate::storage::persist_library(state.library);
    }
    moved
}

/// Books as memberships, folders as nestings, one persist for the batch. No
/// name question is asked: a nesting writes no membership, so nothing arrives
/// on the parent's level.
pub fn nest_many(state: AppState, folder_ids: &[String], parent: &str) {
    if folder_ids.is_empty() {
        return;
    }
    let clean = screened(state, folder_ids, Some(parent), None);
    if clean.is_empty() {
        return;
    }
    let mut moved_ids: Vec<String> = Vec::new();
    state.library.shelves.update(|shelves| {
        for folder_id in &clean {
            if shelf::reparent(shelves, folder_id, Some(parent)) {
                moved_ids.push(folder_id.clone());
            }
        }
    });
    if !moved_ids.is_empty() {
        crate::storage::persist_library(state.library);
    }
}

/// What a drag onto a shelf row's edge commits — the sibling seam the list
/// layout draws — a reorder rather than a filing wherever the two shelves
/// already share a level, which is the common case.
pub fn reorder_shelves_to_anchor(state: AppState, ids: &[String], anchor: &str, side: SeamSide) {
    if ids.is_empty() {
        return;
    }
    let parent = state.library.shelves.with_untracked(|shelves| {
        shelf::find(shelves, anchor).and_then(|s| s.parent.clone())
    });
    let clean = screened(
        state,
        ids,
        parent.as_deref(),
        Some(ShelfSeam {
            anchor_id: anchor.to_string(),
            side,
        }),
    );
    if clean.is_empty() {
        return;
    }
    let mut moved = false;
    state.library.shelves.update(|shelves| {
        let parent = shelf::find(shelves, anchor).and_then(|s| s.parent.clone());
        for id in &clean {
            if id == anchor || !shelf::reparent(shelves, id, parent.as_deref()) {
                continue;
            }
            let (Some(at), Some(mut ai)) = (
                shelves.iter().position(|s| s.id == *id),
                shelves.iter().position(|s| s.id == anchor),
            ) else {
                continue;
            };
            let item = shelves.remove(at);
            if at < ai {
                ai -= 1;
            }
            let at = match side {
                SeamSide::After => ai + 1,
                SeamSide::Before => ai,
            };
            shelves.insert(at, item);
            moved = true;
        }
    });
    if moved {
        crate::storage::persist_library(state.library);
    }
}

pub fn rename_shelf(state: AppState, shelf_id: &str, name: &str) {
    let name = name.trim();
    if name.is_empty() {
        return;
    }
    let name = name.to_string();
    state.library.shelves.update(|shelves| {
        if let Some(shelf) = shelf::find_mut(shelves, shelf_id) {
            shelf.name = name;
        }
    });
    crate::storage::persist_library(state.library);
}

/// A book's new title is LOCKED, and the lock is the difference between a name the reader
/// chose and a name a document supplied. A service rather than a method on the state: the
/// write is one half of the rename and the persist is the other, and a modal that touched
/// storage directly was a second caller of the pair that could drift from the first.
pub fn rename_row(state: AppState, row_id: &str, name: &str) {
    state.library.books.update(|rows| {
        let Some(row) = library_core::book::find_row_mut(rows, row_id) else {
            return;
        };
        match row {
            Row::Book(b) => {
                b.title = Some(name.to_string());
                b.title_locked = true;
            }
            Row::Link { name: own, .. } => *own = name.to_string(),
        }
    });
    crate::storage::persist_library(state.library);
}

/// The name is the target's own at this moment, which is what makes the row recognisable on
/// the shelf beside the book it points at. A service for the same reason [`rename_row`] is:
/// state holds signals, the rules (id, order, seat, persist) are a transaction.
pub fn add_link(state: AppState, name: &str, target: &str, shelf_id: &str) -> String {
    let now = now_ms();
    let link_id = id::next_id(now);
    let made = link_id.clone();
    state.library.books.update(|rows| {
        rows.push(Row::link(link_id, name.to_string(), target.to_string(), now));
    });
    if shelf_id != ALL_SHELF {
        state.library.shelves.update(|shelves| {
            if let Some(shelf) = shelf::find_mut(shelves, shelf_id) {
                shelf::shelf_add(shelf, &made);
            }
        });
    }
    crate::storage::persist_library(state.library);
    made
}

/// The books stay in the library — a shelf is a list of ids and never held a byte — and the page
/// steps back out a level. The shelves inside it, and the books standing on it, come up exactly one
/// level too, onto the nearest rung the folder's tree still stands on: a shelf a level was taken out
/// from under is a shelf the folder's next scan cannot see, and the reader never put its books on
/// the library's own top level.
pub fn delete_shelf(state: AppState, shelf_id: &str) {
    let was_inside = state.library.shelf.get_untracked() == shelf_id;
    // One read of the shelf list answers every fact about the shelf that is going: the level to step out to, which watched folder filed onto it, the rung it stood on, and the books that come up with it.
    let (stepped_out, detached, rung, stood_on) = state.library.shelves.with_untracked(|shelves| {
        shelf::find(shelves, shelf_id).map_or(
            (ALL_SHELF.to_string(), None, None, Vec::new()),
            |gone| {
                (
                    gone.parent
                        .clone()
                        .unwrap_or_else(|| ALL_SHELF.to_string()),
                    gone.kind.folder_id().map(str::to_string),
                    gone.is_folder().then(|| gone.kind.rung().to_string()),
                    gone.books.clone(),
                )
            },
        )
    });
    state.library.shelves.update(|shelves| {
        shelf::lift_children(shelves, shelf_id);
        shelves.retain(|s| s.id != shelf_id);
        // Then the folder's own rungs re-hang the way its next scan would hang them: a rung whose
        // level is gone takes the nearest one still standing, so nothing is left pointing at a
        // shelf that is not there.
        if let Some(folder_id) = &detached {
            for (id, want) in shelf::rehang_moves(shelves, folder_id) {
                if let Some(moved) = shelf::find_mut(shelves, &id) {
                    moved.parent = want;
                }
            }
        }
    });
    if let Some(folder_id) = &detached {
        state.library.folders.update(|folders| {
            if let Some(folder) = folder_ops::find_mut(folders, folder_id) {
                folder.shelf_map.retain(|_, sid| sid != shelf_id);
            }
        });
    }
    // Read after the removal, so the rung that went cannot answer for itself.
    let home = match (&detached, &rung) {
        (Some(folder_id), Some(rung)) => state.library.shelves.with_untracked(|shelves| {
            shelf::rung_above(shelves, folder_id, rung)
        }),
        _ => None,
    };
    if let Some(home) = home {
        state.library.shelves.update(|shelves| {
            for id in &stood_on {
                if let Some(seat) = shelf::find_mut(shelves, &home) {
                    shelf::shelf_add(seat, id);
                }
            }
        });
    }
    let shelves_now = state.library.shelves.get_untracked();
    state.library.books.update(|rows| {
        library_core::book::drop_dead_shelf_links(rows, &shelves_now);
    });
    if was_inside {
        state.library.shelf.set(stepped_out);
    }
    crate::storage::persist_library(state.library);
}

pub fn memberships(state: AppState, book_id: &str) -> Vec<(String, String)> {
    state.library.shelves.with_untracked(|shelves| {
        shelf::containing(shelves, book_id)
            .into_iter()
            .map(|s| (s.id.clone(), s.name.clone()))
            .collect()
    })
}
