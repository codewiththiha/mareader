//! The copies run that touches no ledger: a copies import of ground a read-at-place tree
//! still reads.
//!
//! Every other folder run walks on a [`library_core::folder::WatchedFolder`] — it resolves
//! the folder's row, diffs against its ledger and writes the row back. That is the right
//! shape for a tree the library READS, and the wrong one for a copies import.

use std::collections::BTreeMap;

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use library_core::folder::{key_chain, FolderOpts};
use library_core::id;
use library_core::ledger;
use library_core::scan::FoundFile;
use library_core::shelf::{Shelf, ShelfKind};

use super::claim::{already_importing, claim_root, when_root_is_free};
use super::copy::{copy_batch, measure_stores};
use super::files::{adopt_copy_measurement, mint_stored_row};
use super::folder::heal_by_address;
use super::tasks::{fail, finish_task, push_task, task_id, update_task, FailMode};
use super::{shelf_name, Asked};
use crate::services::library::covers;
use crate::services::library::reveal;
use crate::services::library::folder_label;
use crate::services::library as wire;
use crate::state::library::ImportTask;
use crate::state::AppState;
use crate::time::now_ms;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CopiesDest {
    /// Spliced right behind the shelf whose name the arrival collided with — the duplicate's
    /// own placement rule, because a copy appended to the end of the level is a shelf the reader
    /// has to go and find.
    NewShelf {
        name: String,
        after: Option<String>,
    },
    /// The *replace*'s target, whose books the sweep has just taken out.
    Into { shelf_id: String },
}

/// The one shape this module exists for — every other copies run keeps the ordinary bound walk.
pub(crate) fn copies_over_standing_tree(state: AppState, root: &str, opts: &FolderOpts) -> bool {
    opts.mode().copies_files()
        && state.library.folders.with_untracked(|folders| {
            folders.iter().any(|f| f.root == root && f.mode().reads_in_place())
        })
}

/// [`crate::services::library::import::proceed_folder`]'s own shape, minus the ledger.
pub(crate) fn copies_beside_tree(
    state: AppState,
    root: String,
    opts: FolderOpts,
    dest: CopiesDest,
) {
    let task = task_id();
    let card = task.clone();
    let walking = root.clone();
    if !when_root_is_free(&root, move || {
        start_copies_run(state, walking, opts, dest, card)
    }) {
        already_importing(state, &root);
        return;
    }
    push_task(state, ImportTask::new(task, folder_label(&root)));
}

fn start_copies_run(
    state: AppState,
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
    state: AppState,
    task: String,
    root: String,
    opts: FolderOpts,
    dest: CopiesDest,
) {
    let found = match wire::scan_folder(&task, &root, &opts).await {
        Ok(found) => found,
        Err(message) => return fail(state, &task, message, FailMode::Toast),
    };
    let mut books = state.library.books.get_untracked();
    let registry = ledger::registry_of(&books);
    // Off the ledger's own pure table: every file but the copy the library already made, one
    // book per fingerprint. The copy list is asked again for the heal's skip set, because a file
    // the library reads in place is owed a COPY.
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
    let pending: Vec<(String, &FoundFile)> = adds
        .iter()
        .map(|file| (id::next_id(now), file))
        .collect();
    let expected = (pending.len() + healed.len()) as u32;
    update_task(state, &task, move |t| t.total = expected);
    let copies = match copy_batch(state, &task, &pending).await {
        Ok(copies) => copies,
        Err(message) => return fail(state, &task, message, FailMode::Toast),
    };
    let stores: Vec<String> = pending
        .iter()
        .filter_map(|(book_id, _)| copies.get(book_id).cloned())
        .collect();
    let measured = measure_stores(stores).await;

    let mut root_shelf: Option<String> = None;
    let mut rungs: BTreeMap<String, String> = BTreeMap::new();
    let mut landed = 0u32;
    for (book_id, file) in pending {
        let Some(store) = copies.get(&book_id) else {
            continue;
        };
        let on_shelf = root_shelf.get_or_insert_with(|| dest_shelf(state, &dest, now));
        let shelf_id = if opts.groups {
            rung_shelf(state, &root, on_shelf, &mut rungs, file.subfolder(), now)
        } else {
            on_shelf.clone()
        };
        // The shelf shows the source's stem, which is the name the reader knows the file by.
        mint_stored_row(state, book_id.clone(), file, store.clone(), None, &shelf_id, None);
        adopt_copy_measurement(state, &book_id, measured.get(store).copied());
        landed += 1;
    }
    crate::storage::persist_library(state.library);
    if landed > 0 {
        covers::backfill_missing(state);
    }
    match (&dest, &root_shelf) {
        (CopiesDest::Into { shelf_id }, _) => reveal::reveal_shelf(state, shelf_id),
        (_, Some(minted)) => reveal::reveal_shelf(state, minted),
        _ => {}
    }
    finish_task(state, &task, landed + healed.len() as u32, 0);
}

/// The *replace*'s standing target, or the counter-named shelf of the reader's own the *as new* promised.
fn dest_shelf(state: AppState, dest: &CopiesDest, now: u64) -> String {
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
                shelves.insert(
                    at,
                    Shelf {
                        id: made,
                        name,
                        kind: ShelfKind::Virtual,
                        books: Vec::new(),
                        parent: None,
                        manual_parent: false,
                    },
                );
            });
            id
        }
    }
}

/// The bound run's chain rule, on a local map instead of a ledger's. A folder that does not group has no rungs.
fn rung_shelf(
    state: AppState,
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
        let name = shelf_name(rung, root);
        let hung = parent.clone();
        state.library.shelves.update(|shelves| {
            shelves.push(Shelf {
                id: made,
                name,
                kind: ShelfKind::Virtual,
                books: Vec::new(),
                parent: Some(hung),
                manual_parent: false,
            })
        });
        rungs.insert(rung.to_string(), id.clone());
        parent = id;
    }
    parent
}
