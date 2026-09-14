//! A level going away: the rung of a read-at-place tree the hand names holds books the folder
//! placed there, and taking it apart takes them off the ground that made them. The copies are a
//! cost, and a cost is a question — the ask, its answers, and the act itself.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use library_core::book::{Origin, Row, book_rows};
use library_core::folder::WatchedFolder;
use library_core::shelf::{self as shelf, Shelf};

use crate::services::library::folder_label;
use crate::state::AppState;

use super::departure::depart;
use super::shelves::delete_shelf;

/// The rung the hand is taking apart, and the books that leave the ground with it.
#[derive(Clone, PartialEq)]
pub struct ShelfApartAsk {
    pub shelf_id: String,
    pub name: String,
    pub folder_name: String,
    /// The books read in place that stand on the rung.
    pub books: usize,
    /// The shelf they come up to: the nearest rung the folder's tree still stands on. `None` for
    /// the tree's own root rung, whose books come up to the library itself.
    pub home: Option<String>,
}

impl ShelfApartAsk {
    /// `None` unless the shelf is a rung of a reading folder holding at least one book the folder
    /// placed there: a shelf the reader made, and one whose books are the library's own already,
    /// come apart with nothing to ask.
    pub(super) fn of(state: AppState, shelf_id: &str) -> Option<Self> {
        let shelves = state.library.shelves.get_untracked();
        let rung = shelf::find(&shelves, shelf_id)?;
        let folder_id = rung.kind.folder_id()?;
        let folder = state
            .library
            .folders
            .get_untracked()
            .into_iter()
            .find(|folder| folder.id == folder_id)?;
        if !folder.mode().reads_in_place() {
            return None;
        }
        let rows = state.library.books.get_untracked();
        let books = rung_books(&rows, rung, &folder);
        (!books.is_empty()).then_some(Self {
            shelf_id: rung.id.clone(),
            name: rung.name.clone(),
            folder_name: folder_label(&folder.root),
            books: books.len(),
            home: shelf::rung_above(&shelves, folder_id, rung.kind.rung())
                .and_then(|id| shelf::find(&shelves, &id))
                .map(|seat| seat.name.clone()),
        })
    }
}

/// The books a rung holds in place, asked of the live library: a sheet is up while the library goes
/// on living, so the count the sheet showed and the list the answer acts on are one question.
fn in_place_books(state: AppState, shelf_id: &str) -> Vec<String> {
    let shelves = state.library.shelves.get_untracked();
    let Some(rung) = shelf::find(&shelves, shelf_id) else {
        return Vec::new();
    };
    let Some(folder) = rung.kind.folder_id().and_then(|folder_id| {
        state
            .library
            .folders
            .get_untracked()
            .into_iter()
            .find(|folder| folder.id == folder_id)
    }) else {
        return Vec::new();
    };
    let rows = state.library.books.get_untracked();
    rung_books(&rows, rung, &folder)
}

/// The membership and the ledger between them answer it: a book the reader stores on a rung is the
/// library's own and simply moves, and a book no reading folder placed is nobody's departure.
fn rung_books(books: &[Row], rung: &Shelf, folder: &WatchedFolder) -> Vec<String> {
    rung.books
        .iter()
        .filter(|id| {
            book_rows(books).any(|book| {
                &book.id == *id
                    && matches!(book.origin, Origin::Linked { .. })
                    && folder.placed.contains(&book.fp)
            })
        })
        .cloned()
        .collect()
}

/// What the menus call: a rung holding read-at-place books asks first, and every other shelf comes
/// apart at once, because nothing about it is a question.
pub fn ask_shelf_apart(state: AppState, shelf_id: &str) {
    match ShelfApartAsk::of(state, shelf_id) {
        Some(ask) => state.library.shelf_apart.raise(ask),
        None => delete_shelf(state, shelf_id),
    }
}

pub fn cancel_shelf_apart(state: AppState) {
    state.library.shelf_apart.dismiss();
}

/// The plain answer: one level comes apart, and the books come up to the nearest shelf the folder's
/// tree still stands on.
pub fn take_shelf_apart(state: AppState) {
    let Some(ask) = state.library.shelf_apart.ask.get_untracked() else {
        return;
    };
    cancel_shelf_apart(state);
    delete_shelf(state, &ask.shelf_id);
}

/// The copy answer, in the order that keeps it honest: the books become the library's own FIRST —
/// bytes into the store, the folder's log told they moved out — and the level comes apart once they
/// are safe. A book the store refused leaves the shelf standing, so the reader can ask again rather
/// than lose the ground the rest of the rung answers to.
pub fn take_shelf_apart_as_copies(state: AppState) {
    let Some(ask) = state.library.shelf_apart.ask.get_untracked() else {
        return;
    };
    cancel_shelf_apart(state);
    if !tauri_bridge::has_tauri() {
        return;
    }
    spawn_local(async move {
        let books = in_place_books(state, &ask.shelf_id);
        let copies = depart(state, &books).await;
        if copies.len() == books.len() {
            delete_shelf(state, &ask.shelf_id);
        }
    });
}
