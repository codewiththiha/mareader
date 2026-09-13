//! Importing: running the shell's measurements through the library's ledger and
//! writing the answer to the state.
//!
//! One path for all three ways books arrive — the folder sheet, a handful of
//! files from the picker or a drop, and a rescan of a watched folder when the
//! window regains focus. They differ in where the measurements come from and
//! nothing else, so the deciding and the writing happen once, here.
//!
//! Four rules this module exists to keep:
//!
//!   * **nothing is written until the whole answer is known.** The scan, the
//!     ledger and the copies all run against local copies of the three lists,
//!     and the state is set once at the end. A shelf that filled in as the
//!     import went would repaint per file, and a failure half way through would
//!     leave the library holding books whose bytes never arrived.
//!   * **a rescan is invisible unless it found something.** A quiet run
//!     ([`rescan_watched`]) never raises a dock card, a toast or a state write
//!     for a folder nothing changed in — which, on every window focus, is nearly
//!     all of them.
//!   * **the tree on disk is the tree on the shelf — and a hand cannot take a
//!     read-at-place rung off it.** A folder import mints the whole chain of
//!     shelves between the watched root and each file's subfolder, and a rescan
//!     re-hangs the folder's shelves on the rung their `rel` names, so importing
//!     "1" that holds "2", "3" and four books yields "1" at the root with "2",
//!     "3" and the books inside it — one logic, one tree, rather than a flat
//!     shelf list grown beside a nested one. A hand-move of a rung a READ-AT-
//!     PLACE folder named is a departure rather than a re-hang to fight: the
//!     shelf leaves as a copy the library owns (`crate::services::library::arrange`
//!     asks, copies and converts it) and the folder's map lets the zone go, so
//!     the next walk mints the original rung back on the seat the disk names.
//!     The re-hang still passes by a shelf wearing `Shelf::manual_parent` — a
//!     merged import's rungs and a COPYING folder's shelves, which a hand may
//!     take and keep. Virtual shelves are the reader's own and no scan ever
//!     rearranges them.
//!   * **an ask outranks a rescan.** A focus rescan is a walk the app started
//!     for itself, and on a watched library it is in flight constantly —
//!     including at the moment a picker closes, because a native dialog handing
//!     the window back is a focus event like any other. An import that found its
//!     own root being walked therefore waits out the walk rather than being
//!     refused by it ([`claim::when_root_is_free`]): the rescan honours the
//!     tombstones a removal wrote, which is its whole job, and the run that
//!     lifts them is the reader's.
//!   * **an ask outranks a removal.** The tombstones a removal writes are an
//!     answer to the passive rescan — "stay quiet about this file" — and not to
//!     the reader picking the same folder again a week later. [`Asked::Explicitly`]
//!     runs the import's own ledger table ([`ledger::diff_import`]), where those
//!     tombstones stand aside, and the tombstone is lifted when each book
//!     actually lands rather than when it is merely asked for. Without this,
//!     emptying a watched folder's shelf and importing the folder again returns
//!     nothing at all, silently: a broken import wearing a rule's clothes.
//!
//! ## The file map
//!
//! | module | the question |
//! | --- | --- |
//! | [`tasks`] | the dock's cards: one run's id, and the lifecycle of the card reporting it |
//! | [`claim`] | one walk per root: the claim that keeps two runs off one ledger, and the ask that waits a rescan out |
//! | [`gate`] | the read-at-place arrival: covered ground is the covering tree's reconciliation, a rung no shelf wears is a fold, and anything else is the level's name question — and the run's [`gate::RootPlan`] |
//! | [`folder`] | the folder run itself: scan, diff, copy, land, in one write |
//! | [`files`] | the loose-file run, and the single-file landings every sheet's answer rides |
//! | [`verify`] | the startup measurement, the focus rescan, and the watch a hand turns |
//! | [`restore`] | the books a folder's own log gives back, and the files a log REPRESENTS |
//! | [`copy`] | the store batch and its per-file failure sentence |
//! | [`replace`] | the sheet's *replace*: the sweep out and the walk back in |
//! | [`migrate`] | the one-time move of every stored copy into its own item folder |
//!
//! The four rules above are this directory's, not any one file's: the stages
//! of a run live in [`folder`] and [`files`], and every other module is a
//! stage's own question, split out so the run reads as the order of its
//! stages rather than as a scroll.

mod claim;
mod copy;
mod files;
mod folder;
mod gate;
mod migrate;
mod replace;
mod restore;
mod tasks;
mod verify;

#[cfg(test)]
mod tests;

pub use files::{import_files, land_file};
pub use gate::{ground_tracking, import_folder};
pub use migrate::migrate_store_layout;
pub use replace::replace_rows_of_tree;
pub use restore::restore_deleted_book;
pub use tasks::dismiss_task;
pub use verify::{
    rescan_watched, set_shelf_watch, shelf_watch, verify_library, verify_one, ShelfWatch,
};

pub(crate) use files::{land_stored_copy, land_stored_copy_settling, settle_ledger};
pub(crate) use gate::{proceed_folder, reclaim_rung, RootPlan};
pub(crate) use replace::{replace_folder_with_copies, replace_shelf_with_folder};

use library_core::shelf::Shelf;

use crate::services::library::folder_label;

/// Who asked for a folder run, which is what the tombstones mean.
///
/// One type rather than a boolean at the call site because the two runs are not
/// two settings of one thing: they answer different questions, and a boolean at
/// `run_folder`'s signature cannot say which one it was answering.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Asked {
    /// The reader picked the folder, or dropped it on the window. An explicit
    /// ask overrides the tombstones their own earlier removals wrote.
    Explicitly,
    /// The window regaining focus asked. This is exactly the case a tombstone
    /// exists for — a removed book must stay removed on its own — so they hold.
    OnFocus,
}

/// What a shelf cut from a subfolder is called: the subfolder's own name, or the
/// watched folder's name for the shelf at its root.
pub(super) fn shelf_name(key: &str, root: &str) -> String {
    match key.rsplit('/').next() {
        Some(last) if !last.is_empty() => last.to_string(),
        _ => folder_label(root),
    }
}

/// The `rel` a folder shelf records: `None` at the watched root, so a rescan can
/// tell that shelf from a subfolder that happens to be named like the root.
pub(super) fn rel_of(key: &str) -> Option<String> {
    if key.is_empty() {
        None
    } else {
        Some(key.to_string())
    }
}

/// The shelf a folder's root files onto, if it has one.
pub(super) fn root_shelf_of(shelves: &[Shelf], folder_id: &str) -> Option<String> {
    shelves
        .iter()
        .find(|s| s.kind.is_folder_root() && s.kind.folder_id() == Some(folder_id))
        .map(|s| s.id.clone())
}
