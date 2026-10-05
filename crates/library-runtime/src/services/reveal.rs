//! Taking the reader to a book — in the library, and on the disk.
//!
//! The order is the whole of it: the breadcrumb moves to the shelf the book is on,
//! and only then is the card scrolled to and lit up.

use std::sync::atomic::{AtomicU64, Ordering};

use leptos::prelude::*;

use library_core::book::Row;
use library_core::folder::dir_of_rung;
use library_core::shelf::{ALL_SHELF, ShelfKind, containing, find};

use super::toast;
use crate::state::library::Reveal;

static NONCE: AtomicU64 = AtomicU64::new(1);

pub fn reveal_book(state: crate::context::LibraryContext, book_id: &str) {
    navigate_to_shelf_of(state, book_id);
    light(state, book_id);
}

/// The one write a reveal is: what to light, and the nonce that makes a second reveal
/// of the SAME thing a second reveal.
fn light(state: crate::context::LibraryContext, id: &str) {
    state.library.reveal.set(Some(Reveal {
        id: id.to_string(),
        nonce: NONCE.fetch_add(1, Ordering::Relaxed),
    }));
}

/// The shelf half of [`reveal_book`], and where a folder link's tap goes: the pointer
/// promises "opens the folder where it is", and where it is may be a level the reader is
/// not on.
pub fn reveal_shelf(state: crate::context::LibraryContext, shelf_id: &str) {
    let level = state
        .library
        .shelves
        .with_untracked(|shelves| find(shelves, shelf_id).and_then(|s| s.parent.clone()))
        .unwrap_or_else(|| ALL_SHELF.to_string());
    goto_level(state, level);
    light(state, shelf_id);
}

/// The first shelf in shelf order, so the answer is the same every time; the
/// root when the book is on no shelf.
fn navigate_to_shelf_of(state: crate::context::LibraryContext, book_id: &str) {
    let target = state
        .library
        .shelves
        .with_untracked(|shelves| containing(shelves, book_id).first().map(|s| s.id.clone()))
        .unwrap_or_else(|| ALL_SHELF.to_string());
    goto_level(state, target);
}

/// Move the breadcrumb, and only when it has to move: every effect on the
/// level re-runs on a write, so re-setting the level the reader is already on
/// would re-walk the shelf for nothing.
fn goto_level(state: crate::context::LibraryContext, level: String) {
    if state.library.shelf.get_untracked() != level {
        state.library.shelf.set(level);
    }
}

/// The address a ROW reveals in the OS file manager: the store's own file for a copied
/// book — the copy IS the file this row reads — and the file where it stands otherwise.
/// A link reveals what it points AT.
pub fn path_of_row(state: crate::context::LibraryContext, row_id: &str) -> Option<String> {
    match state.library.row(row_id)? {
        Row::Book(book) => Some(book.path().to_string()),
        Row::Link { target, .. } => {
            if library_core::id::is_shelf(&target) {
                path_of_shelf(state, &target)
            } else {
                match state.library.row(&target)? {
                    Row::Book(book) => Some(book.path().to_string()),
                    Row::Link { .. } => None,
                }
            }
        }
    }
}

/// The ground its watched folder's tree cut the shelf from: the watched root with the
/// rung's own key joined on. A shelf the reader owns has no ground and answers none.
pub fn path_of_shelf(state: crate::context::LibraryContext, shelf_id: &str) -> Option<String> {
    let shelves = state.library.shelves.get_untracked();
    let shelf = find(&shelves, shelf_id)?;
    let ShelfKind::Folder { folder_id, rel } = &shelf.kind else {
        return None;
    };
    let folder = state.library.folder(folder_id)?;
    Some(dir_of_rung(&folder.root, rel.as_deref().unwrap_or("")))
}

/// Take the reader to the file itself: the file manager opens on the item, selected
/// inside its folder. Which path a row reveals is [`path_of_row`]'s answer, and the
/// caller holds it before the ask.
pub fn reveal_in_folder(state: crate::context::LibraryContext, path: String) {
    if !tauri_bridge::has_tauri() {
        toast(
            state,
            "Revealing a file is only available in the desktop app.".to_string(),
        );
        return;
    }
    wasm_bindgen_futures::spawn_local(async move {
        if let Err(message) = super::reveal_path(path).await {
            toast(state, message);
        }
    });
}
