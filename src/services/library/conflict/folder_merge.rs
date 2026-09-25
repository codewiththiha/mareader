//! The compact sheet's question: one file of a folder import merging into a
//! shelf the level already held. Three answers — one book, replace, or two
//! books — and the switch that gives every waiting file the same answer.

use leptos::prelude::*;

use library_core::book::{Fingerprint, find_book_mut, find_by_id};
use library_core::conflict::Placement;
use library_core::scan::FoundFile;

use super::{AskKind, ConflictAsk, answer_batch, member_slot, minted_name};
use crate::services::library::arrange::{ReadingData, purge_books};
use crate::services::library::covers;
use crate::state::AppState;

/// The three are [`Placement::FOLDER_MERGE`]: *merge*, *replace* and *keep both*. *Open* is not
/// among them — the reader is importing the folder, so "go and look at the shelf" is not an
/// answer to a file inside it.
pub fn answer_folder_merge(state: AppState, answer: Placement, apply_all: bool) {
    answer_batch(
        state,
        answer,
        apply_all,
        AskKind::is_folder_merge,
        apply_folder_merge,
    );
}

/// Two of the three go through the unified apply: *merge* and *replace* were each written
/// twice, once here and once for a row dragged onto a row, and the two disagreed about the
/// edges. What a folder merge adds is the ledger write.
fn apply_folder_merge(state: AppState, ask: &ConflictAsk, answer: Placement) {
    let answer = withhold_keep_both_from_a_twin(state, ask, answer);
    // Every answer here is about one arriving file; the two that do nothing are
    // the two this sheet never offers.
    let Some(file) = ask.arrival.file.clone() else {
        return;
    };
    match answer {
        Placement::KeepBoth => {
            let name = minted_name(state, ask);
            land_answer_file(state, ask, file, Some(name), None);
        }
        Placement::Replace => {
            let slot = member_slot(state, &ask.arrival.shelf_id, &ask.existing_id);
            purge_existing(state, &ask.existing_id);
            land_answer_file(state, ask, file, None, slot);
        }
        // The measurement only travels with the answer when the arriving file IS the row's file, which a re-import of one folder always is. A different folder's namesake is another content wearing one name.
        Placement::Merge => {
            if is_the_same_file(state, &ask.existing_id, &file.path) {
                let existing = ask.existing_id.clone();
                let fp = file.fp;
                state.library.books.update(|rows| {
                    if let Some(book) = find_book_mut(rows, &existing) {
                        book.heal(fp);
                    }
                });
            }
            settle(state, ask, file.fp);
            crate::storage::persist_library(state.library);
        }
        Placement::Open | Placement::LinkOnly => {}
    }
}

/// Through the removal's own sweep, receipt and all: a replace here costs
/// the reader exactly what a replace anywhere else does.
fn purge_existing(state: AppState, existing_id: &str) {
    let ids = [existing_id.to_string()];
    purge_books(state, &ids, ReadingData::Delete);
}

/// The sheet already withholds it; this is the write side of the same rule,
/// because apply-to-all can carry an answer to a question whose sheet never
/// offered it.
fn withhold_keep_both_from_a_twin(
    state: AppState,
    ask: &ConflictAsk,
    answer: Placement,
) -> Placement {
    if answer != Placement::KeepBoth {
        return answer;
    }
    let reads_in_place = ask.kind.reads_in_place();
    let Some(file) = ask.arrival.file.as_ref() else {
        return answer;
    };
    if reads_in_place && is_the_same_file(state, &ask.existing_id, &file.path) {
        Placement::Merge
    } else {
        answer
    }
}

fn is_the_same_file(state: AppState, existing_id: &str, path: &str) -> bool {
    state
        .library
        .books
        .with_untracked(|rows| find_by_id(rows, existing_id).is_some_and(|b| b.path() == path))
}

/// One spelling, so a placement cannot be recorded anywhere without the removal being spent beside it.
fn settle(state: AppState, ask: &ConflictAsk, fp: Fingerprint) {
    let folder_id = ask.kind.folder_id();
    crate::services::library::import::settle_ledger(state, folder_id, fp);
}

fn land_answer_file(
    state: AppState,
    ask: &ConflictAsk,
    file: FoundFile,
    name: Option<String>,
    index: Option<usize>,
) {
    let (mode, folder_id) = match &ask.kind {
        AskKind::FolderMerge { mode, folder_id } => (*mode, Some(folder_id.as_str())),
        _ => return,
    };
    let shelf_id = ask.arrival.shelf_id.clone();
    if mode.reads_in_place() {
        crate::services::library::import::land_file(state, &file, name, &shelf_id, index);
        crate::services::library::import::settle_ledger(state, folder_id, file.fp);
        covers::backfill_missing(state);
        crate::storage::persist_library(state.library);
        return;
    }
    // The import module's own single-file copy composition: the copy made and measured BEFORE the row is promised, and a copy that fails leaves the shelf untouched and the ledger unmarked.
    let fp = file.fp;
    crate::services::library::import::land_stored_copy_settling(
        state,
        file,
        name,
        shelf_id,
        index,
        folder_id.map(|id| (id.to_string(), fp)),
    );
}
