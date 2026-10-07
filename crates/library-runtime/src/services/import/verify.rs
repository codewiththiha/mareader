//! The library's two automatic measurements: measure, then backfill.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use library_core::book::{apply_check, book_rows};
use library_core::folder::{self as folder_ops, FolderOpts};
use library_core::governance::Governance;
use library_core::wire::PathCheck;

use super::Asked;
use super::claim::claim_root;
use super::folder::run_folder;
use super::gate::RootPlan;
use super::tasks::task_id;
use crate::services as ipc;
use crate::services::{folder_label, picker_focus, toast};

/// Called on startup and on focus; both owe both passes.
pub fn rescan_watched(state: crate::context::LibraryContext) {
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

/// Guard for a book whose address the shell could not read at all.
fn run_watched(state: crate::context::LibraryContext) {
    if state
        .library
        .books
        .with_untracked(|rows| book_rows(rows).any(|b| b.fp_pending))
    {
        return;
    }
    // A focus the app's own picker caused is not a reader coming back.
    if picker_focus() {
        return;
    }
    let watched: Vec<(String, FolderOpts)> = state
        .library
        .folders
        .get_untracked()
        .iter()
        // Watched anywhere, not only at the root.
        .filter(|f| f.owes_walk())
        .map(|f| (f.root.clone(), f.opts.clone()))
        .collect();
    for (root, opts) in watched {
        walk_one(state, root, opts);
    }
}

/// No card unless it found something; two callers, one spelling.
fn walk_one(state: crate::context::LibraryContext, root: String, opts: FolderOpts) {
    let Some(claim) = claim_root(&root, Asked::OnFocus) else {
        return;
    };
    let task = task_id();
    spawn_local(async move {
        let _claim = claim;
        run_folder(state, task, root, opts, Asked::OnFocus, RootPlan::default()).await;
    });
}

/// Split out of [`rescan_watched`]: a relink asks the same thing about one
/// address.
pub(super) fn apply_checks(state: crate::context::LibraryContext, checks: &[PathCheck]) {
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
    crate::services::persist_library(state.library);
}

/// A value rather than a `bool`: the row is a label and a sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShelfWatch {
    pub folder_id: String,
    /// The shelf's own `rel`, or the seat's rung for a made shelf.
    pub rung: String,
    pub on: bool,
    /// The sentence names the tree the watch decision was about.
    pub label: String,
    pub rung_label: Option<String>,
}

/// The seat as `Governance::seat_of` answers it.
pub fn shelf_watch(state: crate::context::LibraryContext, shelf_id: &str) -> Option<ShelfWatch> {
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

/// The write is the seat's rung, not the tree's root.
pub fn set_shelf_watch(state: crate::context::LibraryContext, shelf_id: &str, on: bool) {
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
    crate::services::persist_library(state.library);
    // Read after the write, so the walk sees the new flag.
    let Some((root, opts)) = state.library.folders.with_untracked(|folders| {
        folder_ops::find(folders, &watch.folder_id).map(|f| (f.root.clone(), f.opts.clone()))
    }) else {
        return;
    };
    // The sentence names the ground the decision was about.
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
