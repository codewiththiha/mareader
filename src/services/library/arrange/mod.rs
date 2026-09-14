//! The moves a reader makes by hand: a drag between shelves, a removal, a relink.
//!
//! One rule covers the membership half of all three — **a move never touches a file the
//! reader owns.** A shelf holds book ids, so a drag edits a list of ids, and the OS file a
//! read-in-place book points at is never renamed, moved or deleted from here.

mod departure;
mod moves;
mod purge;
mod relink;
mod shelf_departure;
mod shelves;

#[cfg(test)]
mod tests;

pub use moves::{also_show, file_many, move_many_to_shelf, move_row, unfile_books};
pub use purge::{purge_books, PurgeOpts};
pub use relink::{ask_relink, cancel_relink, relink_dialog, relink_search_folder};
pub use shelf_departure::{
    answer_departure_return, cancel_departure, confirm_departure, ReturnPath, SeamSide,
    ShelfDepartureAsk,
};
pub use shelves::{
    create_shelf_and_enter, create_shelf_here, delete_shelf, memberships, nest_many, nest_shelf,
    rename_shelf, reorder_shelves_to_anchor,
};

pub(crate) use departure::{convert_to_stored, converts_on_move_to, write_moved_stones};
pub(crate) use moves::Departed;
pub(crate) use purge::{drop_row, unlist_row};

use library_core::shelf::{self as shelf, Shelf};

/// One answer rather than every answer, because a removed book comes back to ONE shelf.
pub(super) fn folder_shelf_of(shelves: &[Shelf], folder_id: &str, book_id: &str) -> Option<String> {
    shelf::containing(shelves, book_id)
        .into_iter()
        .find(|s| s.kind.folder_id() == Some(folder_id))
        .map(|s| s.id.clone())
}
