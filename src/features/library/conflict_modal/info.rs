//! The two strings more than one collision sheet needs, spelled once.
//!
//! A `view!` body is a builder, not a place to compute — so the sentences are
//! built in the describers beside this file, off one snapshot of the library,
//! which is what stops a row promising one thing and doing another. What is
//! left here is what TWO of those describers would otherwise spell for
//! themselves: the level an arrival is going to, and the queue's count.

use library_core::shelf::ALL_SHELF;

use crate::state::AppState;

/// The level an arrival is going to, as a sheet's sentence says it. One
/// spelling for every question that names the level — the name sheet's, the
/// covered file's — because two sheets that worded the same shelf differently
/// would read as two different places. An empty name means the shelf went
/// while the sheet was up, which is the same answer as the root's: a level
/// with no name to speak.
pub(super) fn where_line(state: AppState, shelf_id: &str) -> String {
    if shelf_id == ALL_SHELF {
        "in your library".to_string()
    } else {
        match state.library.shelf_name(shelf_id) {
            name if !name.is_empty() => format!("on “{name}”"),
            _ => "on this shelf".to_string(),
        }
    }
}

/// A subtitle with the queue's count on it, when there is a queue:
/// "Into “Fiction” · 3 more waiting". One spelling for the three sheets whose
/// questions queue, so the count reads the same whichever question is up.
pub(super) fn more_waiting(subtitle: String, waiting: usize) -> String {
    if waiting > 0 {
        format!("{subtitle} · {} more waiting", waiting)
    } else {
        subtitle
    }
}
