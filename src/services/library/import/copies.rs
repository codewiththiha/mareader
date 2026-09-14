//! The copies run that touches no ledger: a copies import of ground a
//! read-at-place tree still reads.
//!
//! Every other folder run walks on a [`library_core::folder::WatchedFolder`] —
//! the run resolves the folder's row, diffs against its ledger, mints its
//! shelves through its map and writes the row back. That is the right shape
//! for a tree the library READS, and the wrong one for a copies import of the
//! very ground such a tree stands on: one directory is one ledger row, so a
//! copies run over a standing tree's root would resolve to THAT row, flip its
//! mode to copies, clear the map its seats hang on and write it back — the
//! linked tree the reader asked to KEEP would stop being read, stop watching
//! and lose every seat, as a side effect of an answer that promised a second
//! shelf beside it.
//!
//! So this run is unbound. It walks the directory, asks the ledger's own pure
//! table what an unbound copies run owes (`library_core::ledger::unbound_copies`
//! — every file but the copy the library already made, one book per
//! fingerprint), copies those files into the store, and files the copies onto
//! shelves of the READER's own: `ShelfKind::Virtual`, minted fresh, bound to
//! no folder row — the departure's copies' own rule, because a copy of a tree
//! is bound to nothing: no rescan, no import and no watch ever answers through
//! it. The standing tree's ledger, mode, map and watch are not read for a
//! decision and not written at all.
//!
//! The two doors are the shelf sheet's stored answers over a standing tree:
//! *as new*, which mints the counter-named shelf beside the tree, and
//! *replace* of a shelf the tree does not own, whose copies file into the
//! shelf the sweep just emptied. A copies import of ground NO tree reads keeps
//! the ordinary bound run: there the fresh ledger row is the copies folder's
//! own and nothing is hijacked.

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

/// Where an unbound copies run files its books.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CopiesDest {
    /// A fresh shelf of the reader's own at the root level, under the counter
    /// name the sheet promised, spliced right behind the shelf whose name the
    /// arrival collided with — the duplicate's own placement rule, because a
    /// copy appended to the end of the level is a shelf the reader has to go
    /// and find. Its subfolder shelves hang inside it.
    NewShelf {
        name: String,
        after: Option<String>,
    },
    /// A shelf that already stands — the *replace*'s target, whose books the
    /// sweep has just taken out. The copies file into it, and its subfolder
    /// shelves hang inside it.
    Into { shelf_id: String },
}

/// Whether a copies run over `root` would resolve onto a standing read-at-place
/// tree's ledger: the mode is copies and a folder the library READS owns the
/// very same ground. The one shape this module exists for — every other copies
/// run keeps the ordinary bound walk.
pub(crate) fn copies_over_standing_tree(state: AppState, root: &str, opts: &FolderOpts) -> bool {
    opts.mode().copies_files()
        && state.library.folders.with_untracked(|folders| {
            folders.iter().any(|f| f.root == root && f.mode().reads_in_place())
        })
}

/// Put the unbound run in motion — [`crate::services::library::import::proceed_folder`]'s
/// own shape, minus the ledger: the card goes up on the click that asked, the
/// walk starts now unless a rescan is walking this very root, in which case it
/// starts on that rescan's release, and a second ask the reader made gets the
/// one refusal sentence.
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

/// Claim the root and start the walk. The claim is the bound run's own: two
/// walks of one directory — this one and a rescan of the tree that reads it —
/// are two answers about the same files, and the copies run waiting its turn
/// is the ask-outranks-a-rescan rule with the ledger half taken out.
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

/// The walk itself: scan, decide against the library rather than a ledger,
/// copy, and file the copies onto the reader's own shelves.
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
    // What the run owes, off the ledger's own pure table: every file but the
    // copy the library already made, one book per fingerprint. The copy list
    // is asked again for the heal's skip set — a file the library reads in
    // place is owed a COPY, so the row at its address is the one file the
    // heal must not fold it into.
    let copy_paths = ledger::copy_over_paths(&found, &registry, &books);
    let mut adds = ledger::unbound_copies(&found, &registry, &books);
    // A row migrated from the old schema carries a placeholder identity, so
    // the table saw its file as unknown: the walk has just measured it, and a
    // file at an address the library holds IS that book. The same heal the
    // bound run runs, on the same rule.
    let healed = heal_by_address(&mut books, &mut adds, &copy_paths);
    if !healed.is_empty() {
        state.library.books.set(books);
    }
    if adds.is_empty() && healed.is_empty() {
        // The card still owes its answer: an import that found every file
        // already copied says so rather than leaving the dock guessing.
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
    // The copies' own measurements, in one pass, before a row is promised: a
    // copy of a file a tree still reads must not wear the ORIGINAL's
    // fingerprint — that identity stays the linked book's, and the copy is
    // known by its own bytes.
    let stores: Vec<String> = pending
        .iter()
        .filter_map(|(book_id, _)| copies.get(book_id).cloned())
        .collect();
    let measured = measure_stores(stores).await;

    // The destination shelf is minted on the first landing rather than at the
    // start: a run whose every copy failed leaves no empty counter-named shelf
    // behind to explain.
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
        // The row is minted with no title of its own: the shelf shows the
        // source's stem (`Book::title`'s fallback for a copy), which is the
        // name the reader knows the file by. `mint_stored_row` marks it
        // independent over the linked twin it copies, so its highlights and
        // its place in the book are its own.
        mint_stored_row(state, book_id.clone(), file, store.clone(), None, &shelf_id, None);
        adopt_copy_measurement(state, &book_id, measured.get(store).copied());
        landed += 1;
    }
    crate::storage::persist_library(state.library);
    if landed > 0 {
        covers::backfill_missing(state);
    }
    // A folder import lights the folder: the shelf the copies landed on, lit
    // on the level that holds it, the bound run's own ending.
    match (&dest, &root_shelf) {
        (CopiesDest::Into { shelf_id }, _) => reveal::reveal_shelf(state, shelf_id),
        (_, Some(minted)) => reveal::reveal_shelf(state, minted),
        _ => {}
    }
    finish_task(state, &task, landed + healed.len() as u32, 0);
}

/// The shelf the run files into: the *replace*'s standing target, or the
/// counter-named shelf of the reader's own the *as new* promised — virtual,
/// at the root level, right behind the shelf whose name the arrival wore.
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

/// The shelf a found file's subfolder files onto, minting every rung between
/// the destination and the file's own subfolder — the bound run's chain rule,
/// on a local map instead of a ledger's: virtual shelves, hung parent to
/// child, in the order the walk found them. A folder that does not group has
/// no rungs and never asks.
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
