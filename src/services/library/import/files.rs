//! The loose-file run — a picker's or a drop's handful of documents — and the single-file
//! landings every answered sheet rides.

use std::collections::HashMap;

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use library_core::book::{book_rows, find_book_mut, Book, Fingerprint, Origin, Row};
use library_core::conflict::Arrival;
use library_core::folder::{self as folder_ops};
use library_core::id;
use library_core::ledger;
use library_core::scan::FoundFile;
use library_core::shelf::{self as shelves_ops};
use library_core::wire::PathCheck;
use reader_core::format::{Format, is_supported_path};

use super::copy::{copy_batch, measure_stores};
use super::kept;
use super::restore::{covered_fate, lift_stone_for, restore_covered_file, take_represented, CoveredFate};
use super::tasks::{fail, finish_task, push_task, task_id, FailMode};
use super::verify::apply_checks;
use crate::services::library::conflict::{self, ConflictAsk};
use crate::services::library::covers;
use crate::services::library::reveal;
use crate::services::library::{file_name, toast};
use crate::services::library as wire;
use crate::state::library::ImportTask;
use crate::state::AppState;
use crate::time::now_ms;

/// The library's own copies: a loose file has no folder to rescan it and no structure to
/// preserve, and a linked row no ledger answers for is a row no rule can keep honest. The
/// source address stays provenance, and a file an in-place folder's tree already holds goes
/// to that folder's own answer first ([`screen_files`]).
pub fn import_files(state: AppState, paths: Vec<String>, target: Option<String>) {
    if paths.is_empty() {
        return;
    }
    let task = task_id();
    let label = match paths.len() {
        1 => file_name(&paths[0]),
        n => format!("{n} files"),
    };
    push_task(state, ImportTask::new(task.clone(), label));
    spawn_local(async move {
        run_files(state, task, paths, target).await;
    });
}

/// A file an in-place tree holds is a file the library already has a book for, and what the
/// drop means is the folder's to say. Answers with the books that came back and the asks the
/// standing ones raise.
fn screen_files(
    state: AppState,
    found: &mut Vec<FoundFile>,
    shelf_id: &str,
) -> (Vec<String>, Vec<ConflictAsk>) {
    let mut restored: Vec<String> = Vec::new();
    let mut covered_asks: Vec<ConflictAsk> = Vec::new();
    found.retain(|file| match covered_fate(state, file) {
        CoveredFate::Ordinary => true,
        CoveredFate::Restore { folder_id, stone } => {
            restored.push(restore_covered_file(state, file, &folder_id, &stone));
            false
        }
        CoveredFate::Ask { folder_id, row_id } => {
            // Named before the id is moved: the row's own name is what the sheet prints.
            let existing_name = state.library.row_name(&row_id);
            covered_asks.push(ConflictAsk::covered(
                Arrival::import(file.clone(), shelf_id.to_string(), None),
                row_id,
                existing_name,
                folder_id,
            ));
            false
        }
    });
    (restored, covered_asks)
}

/// A file whose CONTENT the library already holds is a file the reader already has, wherever
/// it is filed and whatever it is called. A folder walk has always asked this, through the
/// ledger's registry; a loose file did not.
pub(super) fn screen_content(
    state: AppState,
    found: &mut Vec<FoundFile>,
    shelf_id: &str,
) -> Vec<ConflictAsk> {
    let rows = state.library.books.get_untracked();
    let mut asks: Vec<ConflictAsk> = Vec::new();
    found.retain(|file| {
        let Some(held) = ledger::existing_for(&rows, file.fp) else {
            return true;
        };
        if held.missing {
            return true;
        }
        let existing_name = state.library.row_name(&held.row_id);
        asks.push(ConflictAsk::already_have(
            Arrival::import(file.clone(), shelf_id.to_string(), None),
            held.row_id,
            existing_name,
        ));
        false
    });
    asks
}

/// Answer each file by the ground it stands on — a file of a read-at-place folder is that
/// folder's business first, and the rest land as the library's own stored copies, except the
/// ones whose NAME the target level already holds, which ask (see
/// [`crate::services::library::conflict`]).
async fn run_files(state: AppState, task: String, paths: Vec<String>, target: Option<String>) {
    let checks = match wire::verify_paths(paths).await {
        Ok(checks) => checks,
        Err(message) => return fail(state, &task, message, FailMode::Toast),
    };
    let mut found: Vec<FoundFile> = checks.iter().filter_map(found_from_check).collect();
    let represented: Vec<String> = take_represented(state, None, &mut found);
    if found.is_empty() && represented.is_empty() {
        return fail(
            state,
            &task,
            "None of those files could be opened.".to_string(),
            FailMode::Toast,
        );
    }
    apply_checks(state, &checks);

    let shelf_id = target
        .clone()
        .unwrap_or_else(|| shelves_ops::ALL_SHELF.to_string());

    let (restored, covered_asks) = screen_files(state, &mut found, &shelf_id);
    if !restored.is_empty() {
        crate::storage::persist_library(state.library);
    }

    let held_asks = screen_content(state, &mut found, &shelf_id);

    // Every file whose NAME the target level already holds is a question rather than a placement, so a file whose twin sits on another shelf is not one.
    let arrivals: Vec<Arrival> = found
        .iter()
        .map(|file| Arrival::import(file.clone(), shelf_id.clone(), None))
        .collect();
    let (clean, conflicts) = conflict::screen(state, arrivals);

    let now = now_ms();
    let mut pending: Vec<(String, FoundFile, Option<String>, Option<usize>)> = Vec::new();
    let mut stone_landings: Vec<String> = Vec::new();
    for arrival in &clean {
        let Some(file) = arrival.file.clone() else {
            continue;
        };
        // The removal is spent by the explicit ask, and only a COPYING folder's log reaches here: an in-place tree's log is the folder's answer above.
        let stone = lift_stone_for(state, &file);
        let title = stone.as_ref().and_then(|s| s.title.clone());
        let book_id = id::next_id(now);
        if stone.is_some() {
            stone_landings.push(book_id.clone());
        }
        pending.push((book_id, file, title, arrival.index));
    }
    let requests: Vec<(String, &FoundFile)> = pending
        .iter()
        .map(|(book_id, file, _, _)| (book_id.clone(), file))
        .collect();
    let copies = if requests.is_empty() {
        HashMap::new()
    } else {
        match copy_batch(state, &task, &requests).await {
            Ok(copies) => copies,
            Err(message) => {
                let mut asks = covered_asks;
                asks.extend(held_asks.clone());
                asks.extend(conflicts);
                conflict::raise(state, asks);
                return fail(state, &task, message, FailMode::Toast);
            }
        }
    };
    let stores: Vec<String> = pending
        .iter()
        .filter_map(|(book_id, _, _, _)| copies.get(book_id).cloned())
        .collect();
    let measured = measure_stores(stores).await;
    let mut landed = 0u32;
    for (book_id, file, title, index) in pending {
        let Some(store) = copies.get(&book_id) else {
            continue;
        };
        mint_stored_row(
            state,
            book_id.clone(),
            &file,
            store.clone(),
            title,
            &shelf_id,
            index,
        );
        adopt_copy_measurement(state, &book_id, measured.get(store).copied());
        landed += 1;
    }
    let stone_landings: Vec<String> = stone_landings
        .into_iter()
        .filter(|book_id| copies.contains_key(book_id))
        .collect();
    let placed = landed
        + (restored.len() + stone_landings.len()) as u32
        + represented.len() as u32;
    let waiting = (conflicts.len() + covered_asks.len() + held_asks.len()) as u32;
    if placed > 0 {
        covers::backfill_missing(state);
    }
    // One light for the books that CAME BACK: the folder's restorations first, then the landings a log was spent by, then the rows the logs named as represented.
    let came_back = restored
        .into_iter()
        .chain(stone_landings)
        .chain(represented)
        .next();
    if let Some(first) = came_back {
        reveal::reveal_book(state, &first);
    }
    let mut asks = covered_asks;
    asks.extend(held_asks);
    asks.extend(conflicts);
    conflict::raise(state, asks);
    crate::storage::persist_library(state.library);
    finish_task(state, &task, placed, waiting);
}

/// The landing of a file a FOLDER answers for: an in-place merge seating a file its own rung
/// holds, and a restoration putting a logged book back in its folder's place. A loose import
/// does not come through here — a file no folder answers for is the library's own stored
/// copy.
pub fn land_file(
    state: AppState,
    file: &FoundFile,
    name: Option<String>,
    shelf_id: &str,
    index: Option<usize>,
) -> String {
    mint_row(
        state,
        file,
        Origin::Linked {
            src: file.path.clone(),
        },
        None,
        name,
        shelf_id,
        index,
    )
}

pub(crate) fn mint_stored_row(
    state: AppState,
    book_id: String,
    file: &FoundFile,
    store: String,
    name: Option<String>,
    shelf_id: &str,
    index: Option<usize>,
) -> String {
    mint_row(
        state,
        file,
        Origin::Stored {
            src: Some(file.path.clone()),
            store,
        },
        Some(book_id),
        name,
        shelf_id,
        index,
    )
}

/// The single-file form of the batch [`run_files`] lands, for the two answers that place a
/// file after a question. The copy is made BEFORE the row is promised: a failure to copy is
/// a toast and a level left untouched.
pub(crate) fn land_stored_copy(
    state: AppState,
    file: FoundFile,
    name: Option<String>,
    shelf_id: String,
    index: Option<usize>,
) {
    land_stored_copy_settling(state, file, name, shelf_id, index, None);
}

/// [`land_stored_copy`] with the folder's ledger write riding the landing.
pub(crate) fn land_stored_copy_settling(
    state: AppState,
    file: FoundFile,
    name: Option<String>,
    shelf_id: String,
    index: Option<usize>,
    settle: Option<(String, Fingerprint)>,
) {
    let book_id = id::next_id(now_ms());
    let task = format!("import-{book_id}");
    spawn_local(async move {
        match wire::copy_and_measure(&task, &file.path, &book_id).await {
            Ok((store, measured)) => {
                let placed =
                    mint_stored_row(state, book_id, &file, store, name, &shelf_id, index);
                adopt_copy_measurement(state, &placed, measured);
                if let Some((folder_id, fp)) = settle {
                    settle_ledger(state, Some(&folder_id), fp);
                }
                covers::backfill_missing(state);
                crate::storage::persist_library(state.library);
            }
            Err(message) => toast(state, message),
        }
    });
}

/// The copy's own measurement becomes the row's identity and the source file's fingerprint
/// stays free, so an OS file that keeps its own can still be placed by any folder that reads
/// it, as its own linked book.
pub(super) fn adopt_copy_measurement(state: AppState, row_id: &str, measured: Option<Fingerprint>) {
    state.library.books.update(|rows| {
        if let Some(book) = find_book_mut(rows, row_id) {
            book.adopt_measurement(measured);
        }
    });
}

fn mint_row(
    state: AppState,
    file: &FoundFile,
    origin: Origin,
    book_id: Option<String>,
    name: Option<String>,
    shelf_id: &str,
    index: Option<usize>,
) -> String {
    let now = now_ms();
    let independent = state
        .library
        .books
        .with_untracked(|rows| book_rows(rows).any(|b| b.path() == file.path));
    let book = Book {
        title: name,
        independent,
        ..Book::new(
            book_id.unwrap_or_else(|| id::next_id(now)),
            file.fp,
            file.format().unwrap_or(Format::Pdf),
            origin,
            now,
        )
    };
    // Always a row of its own, and never a resolve to the row the library already holds: the
    // reader asked for THIS file on THIS level. Content identity is the ledger's business.
    let placed = book.id.clone();
    state.library.books.update(|rows| {
        rows.push(Row::Book(book));
        // A file that comes back is the file that left: whatever a removal kept for it lands here.
        kept::reclaim(rows, file, &placed);
    });
    // The root has no member list to place into, so the write is skipped rather than made and answered with nothing.
    if shelf_id != shelves_ops::ALL_SHELF {
        state.library.shelves.update(|shelves| {
            if let Some(shelf) = shelves_ops::find_mut(shelves, shelf_id) {
                shelves_ops::place(&mut shelf.books, &placed, index);
            }
        });
    }
    placed
}

/// `None` for an address that did not resolve, or one the format registry does not know.
pub(super) fn found_from_check(check: &PathCheck) -> Option<FoundFile> {
    if !is_supported_path(&check.path) {
        return None;
    }
    let fp = check.fingerprint()?;
    let name = file_name(&check.path);
    let ext = name
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_lowercase())
        .unwrap_or_default();
    Some(FoundFile {
        rel: name,
        path: check.path.clone(),
        ext,
        size: check.size,
        fp,
    })
}

/// Record the placement and spend a removal that was holding the file out — the two writes
/// `run_folder` makes when a file lands, made here because this file landed after an ANSWER
/// rather than after a walk.
pub(crate) fn settle_ledger(state: AppState, folder_id: Option<&str>, fp: Fingerprint) {
    let Some(folder_id) = folder_id else {
        return;
    };
    state.library.folders.update(|folders| {
        if let Some(folder) = folder_ops::find_mut(folders, folder_id) {
            ledger::restore_deleted(folder, &fp);
            folder.mark_placed(fp);
        }
    });
}
