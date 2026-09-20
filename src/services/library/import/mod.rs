//! Importing: running the shell's measurements through the library's ledger
//! and writing the answer to the state.
//!
//! One path for all three ways books arrive — the folder sheet, files from
//! the picker or a drop, and a focus rescan of a watched folder. They differ
//! in where the measurements come from and nothing else.

mod claim;
mod copies;
mod copy;
mod files;
mod folder;
mod gate;
mod kept;
mod migrate;
mod replace;
mod reshape;
mod restore;
mod tasks;
mod verify;

#[cfg(test)]
mod tests;

pub use files::{import_files, land_file};
pub use gate::{ground_tracking, import_folder, GroundWatch};
pub use migrate::migrate_store_layout;
pub use replace::replace_rows_of_tree;
pub use restore::restore_deleted_book;
pub use tasks::dismiss_task;
pub use verify::{rescan_watched, set_shelf_watch, shelf_watch, verify_one};

/// The card lifecycle for single-copy runs outside this module — a relink, a
/// duplicate, a replace's conversion, departures: their beats need a card to
/// land on, and a run that mints a phantom id emits beats nobody sees.
pub(crate) use tasks::{begin_task, fail_task, finish_task};

/// The per-file half of a store batch's answer, for copy runs outside this
/// module: which copies came home with measurements, and a toast for the ones
/// the store refused.
pub(crate) use copy::partition_store_results;

pub(crate) use copies::{copies_beside_tree, copies_over_standing_tree, CopiesDest};
pub(crate) use files::{land_stored_copy, land_stored_copy_settling, settle_ledger};
pub(crate) use gate::{proceed_folder, reclaim_rung, RootPlan};
pub(crate) use replace::{replace_folder_with_copies, replace_shelf_with_folder};

use library_core::shelf::Shelf;

use crate::services::library::folder_label;

/// Which question a `run_folder` call answers; a boolean at the signature
/// could not say.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Asked {
    Explicitly,
    OnFocus,
}

/// The label a rung goes by on the shelf: the last segment of its key, the
/// folder's own name for the root. Not `shelf_name`: that name belongs to
/// [`crate::state::library::LibraryState::shelf_name`], and two jobs wearing
/// one name is a lookup that finds the wrong one.
pub(super) fn rung_label(key: &str, root: &str) -> String {
    match key.rsplit('/').next() {
        Some(last) if !last.is_empty() => last.to_string(),
        _ => folder_label(root),
    }
}

pub(super) fn rel_of(key: &str) -> Option<String> {
    if key.is_empty() {
        None
    } else {
        Some(key.to_string())
    }
}

pub(super) fn root_shelf_of(shelves: &[Shelf], folder_id: &str) -> Option<String> {
    shelves
        .iter()
        .find(|s| s.kind.is_folder_root() && s.kind.folder_id() == Some(folder_id))
        .map(|s| s.id.clone())
}
