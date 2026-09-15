//! The library's two automatic measurements, in the one order they owe: FIRST a pass over
//! every address the library holds — what turns a book `missing` when its file was deleted
//! or moved out from under it, and what replaces a migrated book's placeholder fingerprint
//! with a real one — and THEN the walk of every watched folder when the window regains focus.
//! The walk alone only ever SEES what the measure pass made legible.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use library_core::book::{apply_check, book_rows};
use library_core::folder::{self as folder_ops, FolderOpts};
use library_core::governance::Governance;
use library_core::wire::PathCheck;

use super::claim::claim_root;
use super::folder::run_folder;
use super::gate::RootPlan;
use super::tasks::task_id;
use super::Asked;
use crate::services::library::{folder_label, picker_focus, toast};
use crate::services::library as ipc;
use crate::state::AppState;

/// Called on startup and whenever the window regains focus; both moments owe both passes, measure first.
pub fn rescan_watched(state: AppState) {
    if !tauri_bridge::has_tauri() {
        return;
    }
    let paths: Vec<String> = state
        .library
        .books
        .with_untracked(|rows| book_rows(rows).map(|b| b.path().to_string()).collect());
    spawn_local(async move {
        if !paths.is_empty() {
            match ipc::verify_paths(paths).await {
                Ok(checks) => apply_checks(state, &checks),
                Err(message) => {
                    web_sys::console::warn_1(&format!("[library] verify failed: {message}").into());
                }
            }
        }
        run_watched(state);
    });
}

/// The measurement pass has just replaced every placeholder it could, so the guard here only holds the walk back for a book whose address the shell could not read at all.
fn run_watched(state: AppState) {
    if state
        .library
        .books
        .with_untracked(|rows| book_rows(rows).any(|b| b.fp_pending))
    {
        return;
    }
    // A focus the app's own picker caused is not a reader coming back to the window: the import
    // that picker closed on is about to walk this very ground, and better. The measure pass above
    // still ran, because a book whose file died while a dialog was up is a book the library
    // should know about; it is the walk that waits.
    if picker_focus() {
        return;
    }
    let watched: Vec<(String, FolderOpts)> = state
        .library
        .folders
        .get_untracked()
        .iter()
        // Watched ANYWHERE, not only at the root: a tree the reader turned off at the root while one
        // subfolder stayed on still owes the walk, and the ledger's per-rung gate keeps the off rungs
        // quiet inside it. A folder nothing watches is the one that owes nothing.
        .filter(|f| f.owes_walk())
        .map(|f| (f.root.clone(), f.opts.clone()))
        .collect();
    for (root, opts) in watched {
        walk_one(state, root, opts);
    }
}

/// No card unless it found something, no toast for a folder that cannot be read, and the
/// ledger's rescan table, where the tombstones a removal wrote still hold. Two callers and one
/// spelling, because the two are the same walk asked by two different moments.
pub(super) fn walk_one(state: AppState, root: String, opts: FolderOpts) {
    let Some(claim) = claim_root(&root, Asked::OnFocus) else {
        return;
    };
    let task = task_id();
    spawn_local(async move {
        let _claim = claim;
        run_folder(state, task, root, opts, Asked::OnFocus, RootPlan::default()).await;
    });
}

/// Called when a book joins the library through the reader rather than through an import: an
/// open proves the file is there and measures nothing, so the row it leaves behind carries a
/// placeholder — and a placeholder is exactly what [`rescan_watched`] refuses to diff
/// against.
pub fn verify_one(state: AppState, path: String) {
    if !tauri_bridge::has_tauri() {
        return;
    }
    spawn_local(async move {
        match ipc::verify_paths(vec![path]).await {
            Ok(checks) => apply_checks(state, &checks),
            Err(message) => {
                web_sys::console::warn_1(&format!("[library] verify failed: {message}").into());
            }
        }
    });
}

/// Split out of [`rescan_watched`] because a relink asks for exactly the same thing about one address.
pub(super) fn apply_checks(state: AppState, checks: &[PathCheck]) {
    let mut changed = false;
    state.library.books.update(|rows| {
        for check in checks {
            if !apply_check(rows, check).is_empty() {
                changed = true;
            }
        }
    });
    if !changed {
        return;
    }
    crate::storage::persist_library(state.library);
}


/// A value rather than a `bool` because the menu row that asks is a label and a sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShelfWatch {
    pub folder_id: String,
    /// The shelf's own `rel` for a shelf the tree cut (`""` at the watched root), and the closest folder shelf's rung for a shelf the reader made inside the tree: a made shelf is no rung the disk names, but it stands on a seat, and the seat is what answers.
    pub rung: String,
    pub on: bool,
    /// A watch sentence from three shelves deep names the tree it is about, so the reader is told what stops being watched.
    pub label: String,
    pub rung_label: Option<String>,
}

/// The seat is [`library_core::governance::Governance::seat_of`]'s answer: a shelf the
/// folder's own tree cut answers with its own rung, and a shelf the reader made answers with
/// the rung it stands on.
pub fn shelf_watch(state: AppState, shelf_id: &str) -> Option<ShelfWatch> {
    let (folders, shelves) = (
        state.library.folders.get_untracked(),
        state.library.shelves.get_untracked(),
    );
    let seat = Governance::new(&folders, &shelves).seat_of(shelf_id)?;
    let folder = folder_ops::find(&folders, &seat.folder_id)?;
    let rung_label = (!seat.rung.is_empty())
        .then(|| folder_label(&folder_ops::dir_of_rung(&folder.root, &seat.rung)));
    Some(ShelfWatch {
        on: folder.tracks_rung(&seat.rung),
        label: folder_label(&folder.root),
        rung_label,
        folder_id: seat.folder_id,
        rung: seat.rung,
    })
}

/// The write is the SEAT's rung rather than the tree's root, because tracking is a tree
/// (`library_core::tracking`) and a shelf is a seat in it: "stop watching" asked of a rung is
/// an explicit Off at that rung while the tree above keeps watching its own.
pub fn set_shelf_watch(state: AppState, shelf_id: &str, on: bool) {
    let Some(watch) = shelf_watch(state, shelf_id) else {
        return;
    };
    if watch.on == on {
        return;
    }
    state.library.folders.update(|folders| {
        if let Some(folder) = folder_ops::find_mut(folders, &watch.folder_id) {
            if watch.rung.is_empty() {
                folder.set_tracking_whole(on);
            } else {
                folder.set_tracking(&watch.rung, on);
            }
        }
    });
    crate::storage::persist_library(state.library);
    // Read AFTER the write: the run the walk starts resolves the folder against the flag it
    // carries, so a walk handed the flag from BEFORE the toggle would resolve the tree straight
    // back to the state the reader just turned off.
    let Some((root, opts)) = state.library.folders.with_untracked(|folders| {
        folder_ops::find(folders, &watch.folder_id).map(|f| (f.root.clone(), f.opts.clone()))
    }) else {
        return;
    };
    // The sentence names the ground the decision was about: a rung says which
    // subfolder, in which tree, because a "no longer watched" that read as the
    // whole tree would be a surprise three shelves deep.
    let ground = match &watch.rung_label {
        Some(rung) => format!("“{rung}” in {}", watch.label),
        None => watch.label.clone(),
    };
    toast(
        state,
        if on {
            format!("Watching {ground} for new books.")
        } else {
            format!("{ground} is no longer watched for new books.")
        },
    );
    if on {
        walk_one(state, root, opts);
    }
}
