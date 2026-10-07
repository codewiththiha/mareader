//! A folder import's per-file question: one book, replace, or two books.

use leptos::prelude::*;

use library_core::book::{Fingerprint, find_book_mut, find_by_id};
use library_core::conflict::Placement;
use library_core::scan::FoundFile;

use super::{AskKind, ConflictAsk, answer_batch, member_slot, minted_name};
use crate::services::arrange::{ReadingData, purge_books};
use crate::services::covers;

/// The three are [`Placement::FOLDER_MERGE`]; *open* is no answer here.
pub fn answer_folder_merge(
    state: crate::context::LibraryContext,
    answer: Placement,
    apply_all: bool,
) {
    answer_batch(
        state,
        answer,
        apply_all,
        AskKind::is_folder_merge,
        apply_folder_merge,
    );
}

/// *Merge* and *replace* use the unified apply; this adds the log.
fn apply_folder_merge(state: crate::context::LibraryContext, ask: &ConflictAsk, answer: Placement) {
    let answer = withhold_keep_both_from_a_twin(state, ask, answer);
    // Every answer here is about one arriving file; two do nothing.
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
        // The measurement travels only when the arriving file is the row's.
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
            crate::services::persist_library(state.library);
        }
        Placement::Open | Placement::LinkOnly => {}
    }
}

/// Through the removal's own sweep: a replace costs what one always costs.
fn purge_existing(state: crate::context::LibraryContext, existing_id: &str) {
    let ids = [existing_id.to_string()];
    purge_books(state, &ids, ReadingData::Delete);
}

/// The write side of the sheet's rule: apply-to-all can carry it too.
fn withhold_keep_both_from_a_twin(
    state: crate::context::LibraryContext,
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

fn is_the_same_file(state: crate::context::LibraryContext, existing_id: &str, path: &str) -> bool {
    state
        .library
        .books
        .with_untracked(|rows| find_by_id(rows, existing_id).is_some_and(|b| b.path() == path))
}

/// One spelling, so a placement never lands without the ledger write.
fn settle(state: crate::context::LibraryContext, ask: &ConflictAsk, fp: Fingerprint) {
    let folder_id = ask.kind.folder_id();
    crate::services::import::settle_ledger(state, folder_id, fp);
}

fn land_answer_file(
    state: crate::context::LibraryContext,
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
        crate::services::import::land_file(state, &file, name, &shelf_id, index);
        crate::services::import::settle_ledger(state, folder_id, file.fp);
        covers::backfill_missing(state);
        crate::services::persist_library(state.library);
        return;
    }
    // The import module's own single-file copy composition.
    let fp = file.fp;
    crate::services::import::land_stored_copy_settling(
        state,
        file,
        name,
        shelf_id,
        index,
        folder_id.map(|id| (id.to_string(), fp)),
    );
}
