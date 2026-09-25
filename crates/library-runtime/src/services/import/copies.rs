//! The copies run that touches no ledger: a copies import of ground a
//! read-at-place tree still reads.
//!
//! Every other folder run walks on a [`library_core::folder::WatchedFolder`]
//! — resolve the row, diff against its ledger, write it back. That is the
//! right shape for a tree the library reads and the wrong one for a copies
//! import.

use std::collections::BTreeMap;

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use library_core::folder::{FolderOpts, key_chain};
use library_core::id;
use library_core::ledger;
use library_core::scan::FoundFile;
use library_core::shelf::Shelf;

use super::claim::{claim_root, start_guarded};
use super::copy::copy_batch;
use super::files::{PendingCopy, land_stored_batch};
use super::folder::heal_by_address;
use super::tasks::{FailMode, fail, finish_task, run_total, update_task};
use super::{Asked, rung_label};
use crate::services as ipc;
use crate::services::covers;
use crate::services::reveal;
use app_state::time::now_ms;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CopiesDest {
    /// Spliced right behind the shelf whose name the arrival collided with:
    /// a copy appended to the end of the level is a shelf the reader has to go
    /// and find.
    NewShelf { name: String, after: Option<String> },
    /// The *replace*'s target, whose books the sweep has just taken out.
    Into { shelf_id: String },
}

/// The one shape this module exists for; every other copies run keeps the
/// ordinary bound walk.
pub(crate) fn copies_over_standing_tree(
    state: crate::context::LibraryContext,
    root: &str,
    opts: &FolderOpts,
) -> bool {
    opts.mode().copies_files()
        && state.library.folders.with_untracked(|folders| {
            folders
                .iter()
                .any(|f| f.root == root && f.mode().reads_in_place())
        })
}

/// [`crate::services::import::proceed_folder`]'s own shape, minus the ledger.
pub(crate) fn copies_beside_tree(
    state: crate::context::LibraryContext,
    root: String,
    opts: FolderOpts,
    dest: CopiesDest,
) {
    start_guarded(state, &root, move |state, task, root| {
        start_copies_run(state, root, opts, dest, task)
    });
}

fn start_copies_run(
    state: crate::context::LibraryContext,
    root: String,
    opts: FolderOpts,
    dest: CopiesDest,
    task: String,
) {
    let Some(claim) = claim_root(&root, Asked::Explicitly) else {
        finish_task(state, &task, 0, 0);
        return;
    };
    spawn_local(async move {
        let _claim = claim;
        run_copies(state, task, root, opts, dest).await;
    });
}

async fn run_copies(
    state: crate::context::LibraryContext,
    task: String,
    root: String,
    opts: FolderOpts,
    dest: CopiesDest,
) {
    let found = match ipc::scan_folder(&task, &root, &opts).await {
        Ok(found) => found,
        Err(message) => return fail(state, &task, message, FailMode::Toast),
    };
    let mut books = state.library.books.get_untracked();
    let registry = ledger::registry_of(&books);
    // Off the ledger's own pure table: every file but the copy the library
    // already made, one book per fingerprint. The copy list is asked again
    // for the heal's skip set: a file the library reads in place is owed a
    // copy.
    let copy_paths = ledger::copy_over_paths(&found, &registry, &books);
    let mut adds = ledger::unbound_copies(&found, &registry, &books);
    let healed = heal_by_address(&mut books, &mut adds, &copy_paths);
    if !healed.is_empty() {
        state.library.books.set(books);
    }
    if adds.is_empty() && healed.is_empty() {
        return finish_task(state, &task, 0, 0);
    }

    let now = now_ms();
    let pending: Vec<(String, &FoundFile)> =
        adds.iter().map(|file| (id::next_id(now), file)).collect();
    let expected = run_total(pending.len() as u32, healed.len(), 0);
    update_task(state, &task, move |t| t.total = expected);
    let copies = match copy_batch(state, &task, &pending).await {
        Ok(copies) => copies,
        Err(message) => return fail(state, &task, message, FailMode::Toast),
    };

    // The batch the landing takes: one entry per copy that came home, with the measurement
    // the shell sent beside it.
    let batch: Vec<PendingCopy> = pending
        .iter()
        .filter_map(|(book_id, file)| {
            let (store, measured) = copies.get(book_id)?.clone();
            Some(PendingCopy {
                book_id: book_id.clone(),
                file: (*file).clone(),
                title: None,
                index: None,
                store,
                measured,
            })
        })
        .collect();

    // The seat each copy lands on, resolved in batch order before the
    // landing writes: the root shelf is minted once and a grouped run cuts
    // each file's rung under it.
    let mut root_shelf: Option<String> = None;
    let mut rungs: BTreeMap<String, String> = BTreeMap::new();
    let seats: Vec<String> = batch
        .iter()
        .map(|each| {
            let on_shelf = root_shelf
                .get_or_insert_with(|| dest_shelf(state, &dest, now))
                .clone();
            if opts.groups {
                rung_shelf(
                    state,
                    &root,
                    &on_shelf,
                    &mut rungs,
                    each.file.subfolder(),
                    now,
                )
            } else {
                on_shelf
            }
        })
        .collect();
    let landed = land_stored_batch(state, batch, |at| seats[at].clone());

    crate::services::persist_library(state.library);
    if landed > 0 {
        covers::backfill_missing(state);
    }
    match (&dest, &root_shelf) {
        (CopiesDest::Into { shelf_id }, _) => reveal::reveal_shelf(state, shelf_id),
        (_, Some(minted)) => reveal::reveal_shelf(state, minted),
        _ => {}
    }
    finish_task(state, &task, run_total(landed, healed.len(), 0), 0);
}

/// The *replace*'s standing target, or the counter-named shelf the *as new*
/// answer promised.
fn dest_shelf(state: crate::context::LibraryContext, dest: &CopiesDest, now: u64) -> String {
    match dest {
        CopiesDest::Into { shelf_id } => shelf_id.clone(),
        CopiesDest::NewShelf { name, after } => {
            let id = id::next_shelf_id(now);
            let made = id.clone();
            let name = name.clone();
            let after = after.clone();
            state.library.shelves.update(|shelves| {
                let at = after
                    .as_deref()
                    .and_then(|after| shelves.iter().position(|s| s.id == after))
                    .map_or(shelves.len(), |at| at + 1);
                shelves.insert(at, Shelf::virtual_shelf(made, name, None));
            });
            id
        }
    }
}

/// The bound run's chain rule, on a local map instead of a ledger's. A
/// folder that does not group has no rungs.
fn rung_shelf(
    state: crate::context::LibraryContext,
    root: &str,
    dest_shelf: &str,
    rungs: &mut BTreeMap<String, String>,
    key: &str,
    now: u64,
) -> String {
    let mut parent = dest_shelf.to_string();
    for rung in key_chain(key) {
        if rung.is_empty() {
            continue;
        }
        if let Some(id) = rungs.get(rung) {
            parent = id.clone();
            continue;
        }
        let id = id::next_shelf_id(now);
        let made = id.clone();
        let name = rung_label(rung, root);
        let hung = parent.clone();
        state
            .library
            .shelves
            .update(|shelves| shelves.push(Shelf::virtual_shelf(made, name, Some(hung))));
        rungs.insert(rung.to_string(), id.clone());
        parent = id;
    }
    parent
}
