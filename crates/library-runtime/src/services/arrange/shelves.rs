//! The reader's own shelves: made, named, nested, reordered, taken apart.

use leptos::prelude::*;

use library_core::book::Row;
use library_core::folder as folder_ops;
use library_core::id;
use library_core::shelf::{self as shelf, ALL_SHELF, Shelf};

use runtime_contract::time::now_ms;

use super::asking::ask_move_shelf;
use super::shelf_departure::{SeamSide, ShelfSeam, screen_shelf_moves};

/// `parent`: `None` is the level the page is on, `Some` a named shelf.
pub fn create_shelf_and_enter(
    state: crate::context::LibraryContext,
    parent: Option<&str>,
) -> String {
    let id = match parent {
        Some(parent) => create_shelf_at(state, Some(parent.to_string())),
        None => create_shelf_here(state),
    };
    state.library.shelf.set(id.clone());
    crate::services::persist_library(state.library);
    id
}

/// A new shelf under the open level, without drilling into it.
pub fn create_shelf_here(state: crate::context::LibraryContext) -> String {
    let at = state.library.shelf.get_untracked();
    let parent = (at != ALL_SHELF).then_some(at);
    create_shelf_at(state, parent)
}

fn create_shelf_at(state: crate::context::LibraryContext, parent: Option<String>) -> String {
    let id = library_core::id::next_shelf_id(now_ms());
    let made = id.clone();
    state.library.shelves.update(|shelves| {
        shelves.push(Shelf::virtual_shelf(made, "New shelf", parent));
    });
    // A new shelf re-runs the page's own derives; no query self-set needed.
    id
}

/// Screen the move, ask about departing shelfs that owe a copy.
fn screened(
    state: crate::context::LibraryContext,
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

/// The cycle check is `shelf::reparent`'s, so every caller gets it.
pub fn nest_shelf(
    state: crate::context::LibraryContext,
    folder_id: &str,
    parent: Option<&str>,
) -> bool {
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
        crate::services::persist_library(state.library);
    }
    moved
}

/// Nestings for the batch, one persist, and no name question asked.
pub fn nest_many(state: crate::context::LibraryContext, folder_ids: &[String], parent: &str) {
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
        crate::services::persist_library(state.library);
    }
}

/// A drag onto a shelf row's edge: a reorder on the shared level.
pub fn reorder_shelves_to_anchor(
    state: crate::context::LibraryContext,
    ids: &[String],
    anchor: &str,
    side: SeamSide,
) {
    if ids.is_empty() {
        return;
    }
    let parent = state
        .library
        .shelves
        .with_untracked(|shelves| shelf::find(shelves, anchor).and_then(|s| s.parent.clone()));
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
        crate::services::persist_library(state.library);
    }
}

pub fn rename_shelf(state: crate::context::LibraryContext, shelf_id: &str, name: &str) {
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
    crate::services::persist_library(state.library);
}

/// A book's new title is LOCKED: a chosen name, not a supplied one.
pub fn rename_row(state: crate::context::LibraryContext, row_id: &str, name: &str) {
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
    crate::services::persist_library(state.library);
}

/// The link wears the target's name so the row reads beside its book.
pub fn add_link(
    state: crate::context::LibraryContext,
    name: &str,
    target: &str,
    shelf_id: &str,
) -> String {
    let now = now_ms();
    let link_id = id::next_id(now);
    let made = link_id.clone();
    state.library.books.update(|rows| {
        rows.push(Row::link(
            link_id,
            name.to_string(),
            target.to_string(),
            now,
        ));
    });
    if shelf_id != ALL_SHELF {
        state.library.shelves.update(|shelves| {
            if let Some(shelf) = shelf::find_mut(shelves, shelf_id) {
                shelf::shelf_add(shelf, &made);
            }
        });
    }
    crate::services::persist_library(state.library);
    made
}

/// The page steps out a level; the books come up to the nearest rung.
pub fn delete_shelf(state: crate::context::LibraryContext, shelf_id: &str) {
    let was_inside = state.library.shelf.get_untracked() == shelf_id;
    // One read answers every fact about the shelf that is going.
    let (stepped_out, detached, rung, stood_on) = state.library.shelves.with_untracked(|shelves| {
        shelf::find(shelves, shelf_id).map_or(
            (ALL_SHELF.to_string(), None, None, Vec::new()),
            |gone| {
                (
                    gone.parent.clone().unwrap_or_else(|| ALL_SHELF.to_string()),
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
        // The folder's rungs re-hang the way its next scan would.
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
        (Some(folder_id), Some(rung)) => state
            .library
            .shelves
            .with_untracked(|shelves| shelf::rung_above(shelves, folder_id, rung)),
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
    crate::services::persist_library(state.library);
}

pub fn memberships(state: crate::context::LibraryContext, book_id: &str) -> Vec<(String, String)> {
    state.library.shelves.with_untracked(|shelves| {
        shelf::containing(shelves, book_id)
            .into_iter()
            .map(|s| (s.id.clone(), s.name.clone()))
            .collect()
    })
}
