//! A level going away: the rung of a read-at-place tree holds books the folder placed there, and
//! taking it apart takes them off the ground that made them. The copies are a cost, so the menus ask
//! first ([`super::asking`]), and this file owns the one act that answer buys.

use crate::state::AppState;

use super::asking::{CopyAsk, books_the_rung_takes, raise};
use super::departure::depart;
use super::shelves::delete_shelf;

/// What the menus call: a rung holding books read in place asks first, and every other shelf comes
/// apart at once, because nothing about it is a question.
pub fn ask_shelf_apart(state: AppState, shelf_id: &str) {
    match CopyAsk::of_apart(state, shelf_id) {
        Some(ask) => raise(state, ask),
        None => delete_shelf(state, shelf_id),
    }
}

/// The answer, in the order that keeps it honest: the books become the library's own FIRST — bytes
/// into the store, the folder's log told they moved out — and the level comes apart once they are
/// safe. A book the store refused leaves the shelf standing, so the reader can ask again rather than
/// lose the ground the rest of the rung answers to.
pub(super) async fn take_the_rung_apart(state: AppState, shelf_id: String) {
    let books = books_the_rung_takes(state, &shelf_id);
    let copies = depart(state, &books).await;
    if copies.len() == books.len() {
        delete_shelf(state, &shelf_id);
    }
}
