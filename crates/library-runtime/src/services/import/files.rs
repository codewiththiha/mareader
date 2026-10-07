//! The loose-file run, and the single-file landings sheets ride.

use std::collections::HashMap;

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use library_core::book::{Book, Fingerprint, Origin, Row, book_rows, find_book_mut};
use library_core::conflict::Arrival;
use library_core::folder::{self as folder_ops};
use library_core::id;
use library_core::ledger;
use library_core::paths;
use library_core::scan::FoundFile;
use library_core::shelf::{self as shelves_ops};
use library_core::wire::PathCheck;
use reader_core::format::is_supported_path;

use super::copy::copy_batch;
use super::kept;
use super::restore::{
    CoveredFate, covered_fate, lift_stone_for, restore_covered_file, take_represented,
};
use super::tasks::{FailMode, begin_task, fail, finish_task, push_task, run_total, task_id};
use super::verify::apply_checks;
use crate::services as ipc;
use crate::services::conflict::{self, ConflictAsk};
use crate::services::covers;
use crate::services::file_name;
use crate::services::reveal;
use crate::state::library::ImportTask;
use runtime_contract::time::now_ms;

/// Loose files land as the library's own copies.
pub fn import_files(
    state: crate::context::LibraryContext,
    paths: Vec<String>,
    target: Option<String>,
) {
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

/// A file an in-place tree holds is the folder's to answer for.
fn screen_files(
    state: crate::context::LibraryContext,
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
            // Named before the id moves: the sheet prints the row's name.
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

/// A file whose content the library holds is already the reader's.
pub(super) fn screen_content(
    state: crate::context::LibraryContext,
    found: &mut Vec<FoundFile>,
    shelf_id: &str,
) -> Vec<ConflictAsk> {
    let rows = state.library.books.get_untracked();
    let mut asks: Vec<ConflictAsk> = Vec::new();
    found.retain(|file| {
        let Some(held) = ledger::existing_for(&rows, file.fp) else {
            return true;
        };
        // A missing row is no answer: the file lands as a copy.
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

/// A copy about to land: its measurement, name and slot.
pub(super) struct PendingCopy {
    pub(super) book_id: String,
    pub(super) file: FoundFile,
    pub(super) title: Option<String>,
    pub(super) index: Option<usize>,
    pub(super) store: String,
    pub(super) measured: Option<Fingerprint>,
}

/// A copy queued for the batch: id, file, title and slot.
type QueuedCopy = (String, FoundFile, Option<String>, Option<usize>);

/// Answer each file by its ground; names the level holds ask.
async fn run_files(
    state: crate::context::LibraryContext,
    task: String,
    paths: Vec<String>,
    target: Option<String>,
) {
    let checks = match ipc::verify_paths(paths).await {
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
        crate::services::persist_library(state.library);
    }

    let held_asks = screen_content(state, &mut found, &shelf_id);

    // A clashing name on the target level is a question.
    let arrivals: Vec<Arrival> = found
        .iter()
        .map(|file| Arrival::import(file.clone(), shelf_id.clone(), None))
        .collect();
    let (clean, conflicts) = conflict::screen(state, arrivals);

    let now = now_ms();
    let mut pending: Vec<QueuedCopy> = Vec::new();
    let mut stone_landings: Vec<String> = Vec::new();
    for arrival in &clean {
        let Some(file) = arrival.file.clone() else {
            continue;
        };
        // The explicit ask spends it; an in-place log is the folder's.
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

    // One write per collection for the drop: two pulses, not per file.
    let landed_batch: Vec<PendingCopy> = pending
        .into_iter()
        .filter_map(|(book_id, file, title, index)| {
            let (store, measured) = copies.get(&book_id)?.clone();
            Some(PendingCopy {
                book_id,
                file,
                title,
                index,
                store,
                measured,
            })
        })
        .collect();
    let landed = land_stored_batch(state, landed_batch, |_| shelf_id.clone());

    let stone_landings: Vec<String> = stone_landings
        .into_iter()
        .filter(|book_id| copies.contains_key(book_id))
        .collect();
    let placed = run_total(
        landed,
        restored.len() + stone_landings.len(),
        represented.len(),
    );
    let waiting = (conflicts.len() + covered_asks.len() + held_asks.len()) as u32;
    if placed > 0 {
        covers::backfill_missing(state);
    }
    // One light for the first book that came back.
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
    crate::services::persist_library(state.library);
    finish_task(state, &task, placed, waiting);
}

/// The landing of a file a folder answers for, not a loose import.
pub fn land_file(
    state: crate::context::LibraryContext,
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

fn mint_stored_row(
    state: crate::context::LibraryContext,
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

/// Land a batch of stored copies in one write per collection.
pub(super) fn land_stored_batch(
    state: crate::context::LibraryContext,
    batch: Vec<PendingCopy>,
    seat_of: impl Fn(usize) -> String,
) -> u32 {
    let now = now_ms();
    // The independence check reads the list before the batch lands.
    let independents: Vec<bool> = state.library.books.with_untracked(|rows| {
        batch
            .iter()
            .map(|each| book_rows(rows).any(|b| b.path() == each.file.path))
            .collect()
    });
    state.library.books.update(|rows| {
        for (each, independent) in batch.iter().zip(independents) {
            let mut book = Book {
                title: each.title.clone(),
                independent,
                ..Book::new(
                    each.book_id.clone(),
                    each.file.fp,
                    each.file.admitted_format(),
                    Origin::Stored {
                        src: Some(each.file.path.clone()),
                        store: each.store.clone(),
                    },
                    now,
                )
            };
            // The measurement becomes the row's identity.
            book.adopt_measurement(each.measured);
            let placed_id = book.id.clone();
            rows.push(Row::Book(book));
            // A returning file keeps what a removal stored.
            kept::reclaim(rows, &each.file, &placed_id);
        }
    });
    state.library.shelves.update(|shelves| {
        for (at, each) in batch.iter().enumerate() {
            let shelf_id = seat_of(at);
            // The root has no member list, so the placement write is
            // skipped.
            if shelf_id != shelves_ops::ALL_SHELF
                && let Some(shelf) = shelves_ops::find_mut(shelves, &shelf_id)
            {
                shelves_ops::place(&mut shelf.books, &each.book_id, each.index);
            }
        }
    });
    batch.len() as u32
}

/// The single-file form of the batch, for a sheet's answer.
pub(crate) fn land_stored_copy(
    state: crate::context::LibraryContext,
    file: FoundFile,
    name: Option<String>,
    shelf_id: String,
    index: Option<usize>,
) {
    land_stored_copy_settling(state, file, name, shelf_id, index, None);
}

/// [`land_stored_copy`] with the folder's ledger write riding the landing.
pub(crate) fn land_stored_copy_settling(
    state: crate::context::LibraryContext,
    file: FoundFile,
    name: Option<String>,
    shelf_id: String,
    index: Option<usize>,
    settle: Option<(String, Fingerprint)>,
) {
    let book_id = id::next_id(now_ms());
    // A card the beats can find; an unknown id rings nowhere.
    let task = begin_task(state, file_name(&file.path));
    spawn_local(async move {
        match ipc::copy_one(&task, &file.path, &book_id).await {
            Ok((store, measured)) => {
                let placed = mint_stored_row(state, book_id, &file, store, name, &shelf_id, index);
                adopt_copy_measurement(state, &placed, measured);
                if let Some((folder_id, fp)) = settle {
                    settle_ledger(state, Some(&folder_id), fp);
                }
                covers::backfill_missing(state);
                crate::services::persist_library(state.library);
                finish_task(state, &task, 1, 0);
            }
            Err(message) => fail(state, &task, message, FailMode::Toast),
        }
    });
}

/// The copy's measurement becomes the row, freeing the source's.
fn adopt_copy_measurement(
    state: crate::context::LibraryContext,
    row_id: &str,
    measured: Option<Fingerprint>,
) {
    state.library.books.update(|rows| {
        if let Some(book) = find_book_mut(rows, row_id) {
            book.adopt_measurement(measured);
        }
    });
}

fn mint_row(
    state: crate::context::LibraryContext,
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
            file.admitted_format(),
            origin,
            now,
        )
    };
    // Always its own row; content identity is the ledger's business.
    let placed = book.id.clone();
    state.library.books.update(|rows| {
        rows.push(Row::Book(book));
        kept::reclaim(rows, file, &placed);
    });
    // The root has no member list, so the placement write is skipped.
    if shelf_id != shelves_ops::ALL_SHELF {
        state.library.shelves.update(|shelves| {
            if let Some(shelf) = shelves_ops::find_mut(shelves, shelf_id) {
                shelves_ops::place(&mut shelf.books, &placed, index);
            }
        });
    }
    placed
}

/// `None` for an address that did not resolve, or an unknown format.
pub(super) fn found_from_check(check: &PathCheck) -> Option<FoundFile> {
    if !is_supported_path(&check.path) {
        return None;
    }
    let fp = check.fingerprint()?;
    Some(FoundFile {
        rel: paths::file_name(&check.path),
        path: check.path.clone(),
        ext: paths::extension(&check.path),
        size: check.size,
        fp,
    })
}

/// Record the placement and spend the removal holding the file out.
pub(crate) fn settle_ledger(
    state: crate::context::LibraryContext,
    folder_id: Option<&str>,
    fp: Fingerprint,
) {
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
