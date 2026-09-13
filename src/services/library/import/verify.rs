//! The library's two automatic measurements, in the one order they owe:
//! FIRST a pass over every address the library holds — what turns a book
//! `missing` when its file was deleted or moved out from under it, and what
//! replaces a migrated book's placeholder fingerprint with a real one — and
//! THEN the walk of every watched folder when the window regains focus. The
//! walk alone only ever SEES what is still on disk: a shelf that gained a
//! book while another quietly died would be a shelf lying about one of them,
//! so both automatic moments run both passes, in this order, in one task.
//! Diffing a library that is still carrying placeholder fingerprints would
//! add a second copy of every book a watched folder already holds, which is
//! why the measure runs first and the walk's guard reads what it left.
//!
//! The watch a HAND turns is here too, at the bottom, for the reason the two
//! automatic moments are one function: turning a folder's watch on is asking
//! for a walk of it, and the walk it owes is the same quiet rescan the focus
//! owes — same claim, same ledger table, same answer about a book the reader
//! removed. A second spelling of "walk one folder now" would be a second place
//! for the two to disagree about what a rescan skips.

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
use crate::services::library as wire;
use crate::state::AppState;

/// Measure every address the library holds, then walk every watched folder.
/// Called on startup (as [`verify_library`]) and whenever the window regains
/// focus; both moments owe both passes, measure first.
pub fn rescan_watched(state: AppState) {
    if !tauri_bridge::has_tauri() {
        return;
    }
    // A link has no address to measure, and a pointer at a book is as alive
    // or as dead as the book it points at, which the book's own row is
    // already in this list to answer for.
    let paths: Vec<String> = state
        .library
        .books
        .with_untracked(|rows| book_rows(rows).map(|b| b.path().to_string()).collect());
    spawn_local(async move {
        if !paths.is_empty() {
            match wire::verify_paths(paths).await {
                Ok(checks) => apply_checks(state, &checks),
                Err(message) => {
                    web_sys::console::warn_1(&format!("[library] verify failed: {message}").into());
                }
            }
        }
        run_watched(state);
    });
}

/// The walk half: claim every watched folder's root and start its quiet run.
/// The measurement pass has just replaced every placeholder it could, so the
/// guard here only holds the walk back for a book whose address the shell
/// could not read at all — a diff against a placeholder would add a second
/// copy of every book the folder already holds.
fn run_watched(state: AppState) {
    if state
        .library
        .books
        .with_untracked(|rows| book_rows(rows).any(|b| b.fp_pending))
    {
        return;
    }
    // A focus the app's own picker caused is not a reader coming back to the
    // window: the import that picker closed on is about to walk this very
    // ground, and better — it is the run that lifts a tombstone, which this one
    // honours. The measure pass above still ran, because a book whose file died
    // while a dialog was up is a book the library should know about; it is the
    // walk that waits for a focus that means it.
    if picker_focus() {
        return;
    }
    let watched: Vec<(String, FolderOpts)> = state
        .library
        .folders
        .get_untracked()
        .iter()
        // Watched ANYWHERE, not only at the root: a tree the reader turned off
        // at the root while one subfolder stayed on still owes the walk, and
        // the ledger's per-rung gate is what keeps the off rungs quiet inside
        // it. A folder nothing watches is the one that owes nothing.
        .filter(|f| f.tracks_anything())
        .map(|f| (f.root.clone(), f.opts.clone()))
        .collect();
    for (root, opts) in watched {
        walk_one(state, root, opts);
    }
}

/// Walk ONE folder now, quietly, as the rescan it is: no card unless it found
/// something, no toast for a folder that cannot be read, and the ledger's
/// rescan table, where the tombstones a removal wrote still hold.
///
/// Two callers and one spelling, because the two are the same walk asked by two
/// different moments — every watched folder when the window regains focus, and
/// the one folder a hand just turned a watch on.
pub(super) fn walk_one(state: AppState, root: String, opts: FolderOpts) {
    // A folder a previous run is still walking keeps its walk: a rescan is
    // a question, and the run in flight is already answering it. An explicit
    // run in flight keeps its walk for the stronger reason — it is the reader's.
    let Some(claim) = claim_root(&root, Asked::OnFocus) else {
        return;
    };
    let task = task_id();
    spawn_local(async move {
        let _claim = claim;
        run_folder(state, task, root, opts, Asked::OnFocus, RootPlan::default()).await;
    });
}

/// The startup's name for the same two passes [`rescan_watched`] runs:
/// measure every address the library holds, then walk the watched folders.
/// One function under two names because the two moments read differently —
/// a launch owes the reader a library that knows what it holds, a focus owes
/// a shelf that noticed the folder — and the work is one.
pub fn verify_library(state: AppState) {
    rescan_watched(state);
}

/// Measure one address and write the result.
///
/// Called when a book joins the library through the reader rather than through an
/// import: an open proves the file is there and measures nothing, so the row it
/// leaves behind carries a placeholder identity — and a placeholder is exactly
/// what [`rescan_watched`] refuses to diff against. One file's metadata is a
/// cheap way to keep a hand-opened book from holding every watched folder off
/// until the next launch.
pub fn verify_one(state: AppState, path: String) {
    if !tauri_bridge::has_tauri() {
        return;
    }
    spawn_local(async move {
        match wire::verify_paths(vec![path]).await {
            Ok(checks) => apply_checks(state, &checks),
            Err(message) => {
                web_sys::console::warn_1(&format!("[library] verify failed: {message}").into());
            }
        }
    });
}

/// Write a batch of path checks into the library. Split out of
/// [`verify_library`] because a relink asks for exactly the same thing about one
/// address, and one definition of "what a measurement does to a book" is one
/// fewer place for the two to disagree.
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

// ---------------------------------------------------------------------------
// The watch a hand turns.
// ---------------------------------------------------------------------------

/// The watch a shelf answers for: which folder's tree it stands in, which rung
/// of that tree its menu row turns, whether that rung is watched, and what the
/// folder and the rung are called wherever the library names one.
///
/// A value rather than a `bool` because the menu row that asks is a label and a
/// sentence, and a caller that fetched the folder a second time to spell them
/// would be a second reader of a ledger the first one just read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShelfWatch {
    /// The folder whose tracking tree holds the decision.
    pub folder_id: String,
    /// The rung this shelf stands on — the rung a toggle from its menu writes.
    /// The shelf's own `rel` for a shelf the tree cut (`""` at the watched
    /// root), and the closest folder shelf's rung for a shelf the reader made
    /// inside the tree: a made shelf is no rung the disk names, but it stands
    /// on a seat, and the seat is what answers — the same seat the watch dot
    /// on the card reads (`library_core::governance::Governance::seat_of`),
    /// so the row and the dot cannot disagree about the state they show.
    pub rung: String,
    /// The effective answer at that rung: the rung's own decision, or the
    /// closest ancestor that has one.
    pub on: bool,
    /// What the folder's root is called. A watch sentence from three shelves
    /// deep names the tree it is about, so the reader is told what stops
    /// being watched rather than finding out at the next focus.
    pub label: String,
    /// What the rung itself is called — the last segment of the subfolder's
    /// path — for a seat deeper than the root, and `None` at the watched root,
    /// where the folder's own name is the whole sentence.
    pub rung_label: Option<String>,
}

/// The watch a SHELF answers for, when it answers for one: the shelf stands on
/// ground a folder the library reads in place owns, so the watch is a fact
/// about the seat under this shelf rather than about the shelf itself.
///
/// The seat is [`library_core::governance::Governance::seat_of`]'s answer, and
/// the walk that used to be here is that resolver's: a shelf the folder's own
/// tree cut answers with its own rung, however deep, and a shelf the reader
/// MADE inside such a tree answers with the closest folder shelf's rung — it
/// is not a rung the disk names, but it is standing inside the tree, and
/// "stop watching" asked from there is a decision at the seat it stands on.
///
/// `None` for a shelf with no read-at-place seat — the reader's own shelf on
/// the reader's own ground, which has no watch to turn — and for a shelf of a
/// COPYING folder: the import sheet does not offer the watch beside a copy, so
/// the shelf's menu does not either, and the two surfaces that can set the
/// flag stay one rule.
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

/// Turn the watch of the ground a shelf stands on, from the shelf's own menu.
///
/// The write is the SEAT's rung rather than the tree's root, because tracking
/// is a tree (`library_core::tracking`) and a shelf is a seat in it: "stop
/// watching" asked of a rung is an explicit Off at that rung while the tree
/// above keeps watching its own, and asked of the root shelf it is the whole
/// tree, which is the root's seat. A shelf the reader made inside the tree
/// writes the closest rung the disk named for it — the seat its dot already
/// showed. The rescan honours the same tree the write lands on: a rung turned
/// off adds nothing on a quiet walk (`library_core::ledger::decide`), which is
/// what makes the row mean what it says.
///
/// A folder that already answers this way is not a write, and is not a walk
/// either: toggling it on again would be a second rescan of a ground the first
/// one has just covered.
///
/// Turning it ON owes a walk, and owes it quietly: the reader just asked the
/// library to look at this ground, so a file that arrived while nobody was
/// watching should show up now rather than at the next focus. The walk is the
/// rescan's own, which is the point of it being [`walk_one`] rather than an
/// import — the tombstones a removal wrote still hold, because turning a watch
/// on is not asking back for the books the reader took out, and handing them
/// over would be the resurrection a tombstone exists to prevent. Turning it OFF
/// writes nothing else: the ledger stays exactly as it was, so the books this
/// folder placed are still the books it placed if the watch ever comes back.
pub fn set_shelf_watch(state: AppState, shelf_id: &str, on: bool) {
    let Some(watch) = shelf_watch(state, shelf_id) else {
        return;
    };
    if watch.on == on {
        return;
    }
    let Some((root, opts)) = state.library.folders.with_untracked(|folders| {
        folder_ops::find(folders, &watch.folder_id).map(|f| (f.root.clone(), f.opts.clone()))
    }) else {
        return;
    };
    state.library.folders.update(|folders| {
        if let Some(folder) = folder_ops::find_mut(folders, &watch.folder_id) {
            folder.set_tracking(&watch.rung, on);
        }
    });
    crate::storage::persist_library(state.library);
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
