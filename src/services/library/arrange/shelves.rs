//! The reader's own shelves: made, named, nested, reordered and taken apart. A shelf holds
//! ids and never held a byte, so every operation here is a membership edit — with the
//! departure's question asked of the moves that carry a read-at-place shelf off its folder's
//! seat.

use leptos::prelude::*;

use library_core::folder as folder_ops;
use library_core::shelf::{self as shelf, Shelf, ALL_SHELF};

use crate::state::AppState;
use crate::time::now_ms;

use super::shelf_departure::{raise_departure, screen_shelf_moves, SeamSide, ShelfSeam};

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

/// What a bulk "file onto a new shelf" wants: the reader picked books on one shelf and asked for them to be on another, and navigating them away is an answer to a question they did not ask.
pub fn create_shelf_here(state: AppState) -> String {
    let at = state.library.shelf.get_untracked();
    let parent = (at != ALL_SHELF).then_some(at);
    create_shelf_at(state, parent)
}

fn create_shelf_at(state: AppState, parent: Option<String>) -> String {
    let id = library_core::id::next_shelf_id(now_ms());
    let made = id.clone();
    state.library.shelves.update(|shelves| {
        shelves.push(Shelf {
            id: made,
            name: "New shelf".to_string(),
            kind: library_core::shelf::ShelfKind::Virtual,
            books: Vec::new(),
            parent,
            manual_parent: false,
        });
    });
    // A belt-and-braces tick for an open search: re-setting the query guarantees the folder filter and the shelves-inside derive are seen together on the frame the shelf lands.
    state.library.query.set(state.library.query.get_untracked());
    id
}


/// The cycle check is `library_core::shelf::reparent`'s and not the caller's: a folder filed inside itself renders on no level at all and can never be opened again, so the rule has to hold for every caller.
pub fn nest_shelf(state: AppState, folder_id: &str, parent: Option<&str>) -> bool {
    let one = [folder_id.to_string()];
    let (clean, departing) = screen_shelf_moves(state, &one, parent);
    if !departing.is_empty() {
        raise_departure(state, departing, parent.map(str::to_string), None);
        return false;
    }
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

/// The books are memberships and the folders are nestings, and one persist covers the batch. No NAME question is asked: a nesting writes no membership, so nothing arrives on the parent's level.
pub fn nest_many(state: AppState, folder_ids: &[String], parent: &str) {
    if folder_ids.is_empty() {
        return;
    }
    let (clean, departing) = screen_shelf_moves(state, folder_ids, Some(parent));
    if !departing.is_empty() {
        raise_departure(state, departing, Some(parent.to_string()), None);
    }
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

/// What a drag onto a shelf ROW's edge commits — the sibling seam the list layout draws — and a reorder rather than a filing wherever the two shelves already share a level, which is the common case.
pub fn reorder_shelves_to_anchor(state: AppState, ids: &[String], anchor: &str, side: SeamSide) {
    if ids.is_empty() {
        return;
    }
    let parent = state.library.shelves.with_untracked(|shelves| {
        shelf::find(shelves, anchor).and_then(|s| s.parent.clone())
    });
    let (clean, departing) = screen_shelf_moves(state, ids, parent.as_deref());
    if !departing.is_empty() {
        raise_departure(
            state,
            departing,
            parent,
            Some(ShelfSeam {
                anchor_id: anchor.to_string(),
                side,
            }),
        );
    }
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

/// The books stay in the library — a shelf is a list of ids and never held a byte — and the page steps back out a level. The shelves inside it move up, for the same reason: a child left pointing at a parent that is gone renders on no level at all.
pub fn delete_shelf(state: AppState, shelf_id: &str) {
    let was_inside = state.library.shelf.get_untracked() == shelf_id;
    // One read of the shelf list answers both facts about the shelf that is going: the level to step out to, and — cut as well — which watched folder filed onto it.
    let (stepped_out, detached) =
        state
            .library
            .shelves
            .with_untracked(|shelves| {
                shelf::find(shelves, shelf_id)
                    .map_or((ALL_SHELF.to_string(), None), |gone| {
                        (
                            gone.parent
                                .clone()
                                .unwrap_or_else(|| ALL_SHELF.to_string()),
                            gone.kind.folder_id().map(str::to_string),
                        )
                    })
            });
    state.library.shelves.update(|shelves| {
        shelf::lift_children(shelves, shelf_id);
        shelves.retain(|s| s.id != shelf_id);
    });
    if let Some(folder_id) = detached {
        state.library.folders.update(|folders| {
            if let Some(folder) = folder_ops::find_mut(folders, &folder_id) {
                folder.shelf_map.retain(|_, sid| sid != shelf_id);
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
