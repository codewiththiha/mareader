//! The only place a drop touches library state.
//!
//! Every move here rides a service the shelf's menus already ride — `crate::services::library`
//! for the moves and the shelf a fold makes — so a dragged book persists, keeps its cover and is
//! revealed exactly as a filed one is.

use leptos::prelude::*;

use library_core::shelf::{ALL_SHELF, find};

use super::controller::DragPayload;
use super::effect::DropEffect;
use crate::services::library::{
    create_shelf_here, move_many_to_shelf, nest_many, nest_shelf, reorder_shelves_to_anchor,
    unfile_books, SeamSide,
};
use crate::state::AppState;

pub fn apply(state: AppState, effect: DropEffect, payload: DragPayload) {
    if payload.is_empty() {
        return;
    }
    // A drag inside an expanded branch is THAT branch's: reading the page's level here instead would unfile a book that sits on both from the open shelf for a reorder that never left the branch.
    let from = match payload.source.clone() {
        Some(named) => (named != ALL_SHELF).then_some(named),
        None => {
            let open = state.library.shelf.get_untracked();
            (open != ALL_SHELF).then_some(open)
        }
    };

    match effect {
        DropEffect::Refused => {}
        DropEffect::InsertBefore {
            book_id,
            shelf,
            after,
        } => {
            // HERE is two facts the effect carries rather than this step re-deriving: the container that renders the row, and which side of the anchor the seam was. The held folders get no position: a level renders its folders before its books.
            let (to, index) = insert_anchor(state, &book_id, shelf.as_deref(), after);
            move_many_to_shelf(state, &payload.books, from, to.clone(), index);
            land_folders(state, &payload.folders, &to);
        }
        DropEffect::ShelfSibling { anchor_id, after } => {
            let side = if after {
                SeamSide::After
            } else {
                SeamSide::Before
            };
            reorder_shelves_to_anchor(state, &payload.folders, &anchor_id, side);
        }
        DropEffect::FileToShelf { shelf_id } if shelf_id.is_empty() => {
            match from.as_deref() {
                Some(shelf) => unfile_books(state, &payload.books, shelf),
                None => {
                    move_many_to_shelf(state, &payload.books, None, ALL_SHELF.to_string(), None)
                }
            }
            land_folders(state, &payload.folders, ALL_SHELF);
        }
        DropEffect::FileToShelf { shelf_id } => {
            move_many_to_shelf(state, &payload.books, from, shelf_id.clone(), None);
            land_folders(state, &payload.folders, &shelf_id);
        }
        DropEffect::NestInto { folder_id } => {
            move_many_to_shelf(state, &payload.books, from, folder_id.clone(), None);
            land_folders(state, &payload.folders, &folder_id);
        }
        DropEffect::CreateFolder { with_book_id } => {
            let shelf_id = create_shelf_here(state);
            let mut books = payload.books;
            if !books.contains(&with_book_id) {
                books.push(with_book_id);
            }
            move_many_to_shelf(state, &books, from, shelf_id.clone(), None);
            land_folders(state, &payload.folders, &shelf_id);
        }
    }
}

fn land_folders(state: AppState, folders: &[String], to: &str) {
    if to == ALL_SHELF {
        for folder in folders {
            nest_shelf(state, folder, None);
        }
    } else {
        nest_many(state, folders, to);
    }
}

/// The index is the anchor's position in its CONTAINER rather than a count of what is on screen, which is the fix for a drop between nested rows — an expanded tree renders rows the level's own order does not hold.
fn insert_anchor(
    state: AppState,
    book_id: &str,
    shelf: Option<&str>,
    after: bool,
) -> (String, Option<usize>) {
    let reorder = state.library.view.with_untracked(|view| view.drag_reorders());
    let open = state.library.shelf.get_untracked();
    let container: Option<String> = match shelf {
        Some(named) => (named != ALL_SHELF).then(|| named.to_string()),
        None => (open != ALL_SHELF).then_some(open),
    };
    let step = usize::from(after && reorder);
    match container {
        Some(id) => {
            let index = reorder.then(|| {
                state.library.shelves.with_untracked(|shelves| {
                    find(shelves, &id)
                        .and_then(|each| each.books.iter().position(|member| member == book_id))
                        .map_or(0, |at| at + step)
                })
            });
            (id, index)
        }
        None => {
            let index = reorder.then(|| {
                state.library.books.with_untracked(|rows| {
                    rows.iter()
                        .position(|row| row.id() == book_id)
                        .map_or(0, |at| at + step)
                })
            });
            (ALL_SHELF.to_string(), index)
        }
    }
}
