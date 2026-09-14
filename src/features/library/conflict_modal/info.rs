//! The two strings more than one collision sheet needs, spelled once.
//!
//! The sentences themselves are built in the describers beside this file, off one snapshot of the
//! library.

use library_core::shelf::ALL_SHELF;

use crate::state::AppState;

/// One spelling for every question that names the level, because two sheets that worded the same shelf differently would read as two different places. An empty name means the shelf went while the sheet was up.
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

pub(super) fn more_waiting(subtitle: String, waiting: usize) -> String {
    if waiting > 0 {
        format!("{subtitle} · {} more waiting", waiting)
    } else {
        subtitle
    }
}
