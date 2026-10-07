//! A level going away, and the one act its answer buys.

use super::asking::{CopyAsk, books_the_rung_takes, raise};
use super::departure::depart;
use super::shelves::delete_shelf;

/// A rung holding books read in place asks first; the rest is no question.
pub fn ask_shelf_apart(state: crate::context::LibraryContext, shelf_id: &str) {
    match CopyAsk::of_apart(state, shelf_id) {
        Some(ask) => raise(state, ask),
        None => delete_shelf(state, shelf_id),
    }
}

/// The books become the library's own first, then the level comes apart.
pub(super) async fn take_the_rung_apart(state: crate::context::LibraryContext, shelf_id: String) {
    let books = books_the_rung_takes(state, &shelf_id);
    let copies = depart(state, &books).await;
    if copies.len() == books.len() {
        delete_shelf(state, &shelf_id);
    }
}
