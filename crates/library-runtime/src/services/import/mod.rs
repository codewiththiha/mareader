//! Importing: the shell's measurements through the library's ledger.

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
pub use gate::{GroundWatch, ground_tracking, import_folder};
pub use migrate::migrate_store_layout;
pub use replace::replace_rows_of_tree;
pub use restore::restore_deleted_book;
pub use tasks::dismiss_task;
pub use verify::{rescan_watched, set_shelf_watch, shelf_watch};

/// The card lifecycle for single-copy runs outside this module.
pub(crate) use tasks::{begin_task, fail_task, finish_task};

/// The per-file half of a store batch's answer.
pub(crate) use copy::partition_store_results;

pub(crate) use copies::{CopiesDest, copies_beside_tree, copies_over_standing_tree};
pub(crate) use files::{land_stored_copy, land_stored_copy_settling, settle_ledger};
pub(crate) use gate::{RootPlan, proceed_folder, reclaim_rung};
pub(crate) use replace::{replace_folder_with_copies, replace_shelf_with_folder};

use library_core::shelf::Shelf;

use crate::services::folder_label;

/// Which question a `run_folder` call answers; a boolean at the signature
/// could not say.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Asked {
    Explicitly,
    OnFocus,
}

/// The label a rung goes by on the shelf; not `shelf_name`.
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
