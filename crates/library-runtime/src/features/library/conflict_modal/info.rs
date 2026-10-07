//! The strings more than one collision sheet needs, spelled once.

use library_core::shelf::ALL_SHELF;

/// One spelling per question naming the level; empty means it went.
pub(super) fn where_line(state: crate::context::LibraryContext, shelf_id: &str) -> String {
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
