//! The compact sheet's question: one file of a folder import merging into a
//! shelf the level already held, whose NAME a rung of that shelf carries.
//! Three answers — one book, replace, or two books — and the switch that
//! gives every waiting file of the merge the same answer in one click.

use leptos::prelude::*;

use library_core::book::{find_book_mut, find_by_id, Fingerprint};
use library_core::conflict::Placement;
use library_core::scan::FoundFile;

use super::{answer_batch, member_slot, minted_name, AskKind, ConflictAsk};
use crate::services::library::arrange::{purge_books, PurgeOpts};
use crate::services::library::covers;
use crate::state::AppState;

/// One of the compact sheet's three buttons, in the unified vocabulary.
///
/// The three are [`Placement::FOLDER_MERGE`]: *merge*, *replace* and *keep
/// both*. *Open* is not among them — the reader is importing the folder, so
/// "go and look at the shelf" is not an answer to a file inside it.
///
/// `apply_all` is the switch beside them: checked, the answer is given to every
/// folder-merge question in the queue as well — a reader who has seen one file of
/// a forty-file folder and knows what the whole folder is does not owe the sheet
/// thirty-nine more clicks. A book question that is NOT a folder-merge one stops
/// the drain: it belongs to another gesture and gets its own sheet.
pub fn answer_folder_merge(state: AppState, answer: Placement, apply_all: bool) {
    answer_batch(
        state,
        answer,
        apply_all,
        AskKind::is_folder_merge,
        apply_folder_merge,
    );
}

/// One answer, applied.
///
/// Two of the three go through the unified apply, which is the point: *merge* and
/// *replace* were each written twice, once here and once for a row dragged onto a
/// row, and the two disagreed about the edges. What a folder merge adds to them is
/// the ledger — the placement has to be recorded in the folder that is merging,
/// and a removal that was holding the file out spent — so that rides the answer
/// here rather than in the apply, which knows nothing about folders.
///
/// *Keep both* does not go through the apply at all, and the reason is the folder:
/// a file landing on a shelf the folder's own walk minted lands linked when the
/// folder reads in place and as a store copy when it does not, and only the ask's
/// kind carries that mode. The generic landing would copy a file the folder is
/// supposed to be reading in place, which is the one mistake this sheet exists to
/// avoid making twice.
fn apply_folder_merge(state: AppState, ask: &ConflictAsk, answer: Placement) {
    let answer = withhold_keep_both_from_a_twin(state, ask, answer);
    match answer {
        Placement::KeepBoth => {
            let Some(file) = ask.arrival.file.clone() else {
                return;
            };
            let name = minted_name(state, ask);
            land_answer_file(state, ask, file, Some(name), None);
        }
        Placement::Replace => {
            let Some(file) = ask.arrival.file.clone() else {
                return;
            };
            // Read the slot before the purge takes the row that holds it: an
            // overwrite stays where the thing it replaced was.
            let slot = member_slot(state, &ask.arrival.shelf_id, &ask.existing_id);
            purge_existing(state, &ask.existing_id);
            land_answer_file(state, ask, file, None, slot);
        }
        // One book — and the measurement only travels with the answer when the
        // arriving file IS the row's file, which a re-import of one folder always
        // is. A different folder's namesake is another content wearing one name:
        // the shelf's book keeps its own identity, the arriving file simply does
        // not land, and the ledger mark is what keeps the next rescan quiet.
        Placement::Merge => {
            let Some(file) = ask.arrival.file.clone() else {
                return;
            };
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
        // Neither is an answer this sheet offers, and `answer_batch` routes only
        // a folder merge here, so either is a caller bug: there is nothing honest
        // to place, and guessing at a folder would be worse than doing nothing.
        Placement::Open | Placement::LinkOnly => {}
    }
}

/// The row already on the shelf leaves the library, through the removal's own
/// sweep — receipt and all — so a replace here costs the reader exactly what a
/// replace anywhere else does.
fn purge_existing(state: AppState, existing_id: &str) {
    let ids = [existing_id.to_string()];
    purge_books(state, &ids, PurgeOpts::default());
}

/// *Keep both* withheld from a file whose address a read-at-place row already
/// reads, answered as the merge that keeps the one book the file is.
///
/// The sheet already withholds it; this is the write side of the same rule,
/// because apply-to-all can carry an answer across to a question whose sheet
/// never offered it.
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

/// Whether the row already on the shelf reads this very address.
fn is_the_same_file(state: AppState, existing_id: &str, path: &str) -> bool {
    state.library.books.with_untracked(|rows| {
        find_by_id(rows, existing_id).is_some_and(|b| b.path() == path)
    })
}

/// Record the placement in the ledger of the folder that is merging, and spend a
/// removal that was holding the file out. One spelling, so a placement cannot be
/// recorded anywhere without the removal being spent beside it.
fn settle(state: AppState, ask: &ConflictAsk, fp: Fingerprint) {
    let folder_id = ask.kind.folder_id();
    crate::services::library::import::settle_ledger(state, folder_id, fp);
}

/// Land one answered file on the merged-into shelf: now, when the folder
/// reads in place, or after the store copy the folder's options owe.
fn land_answer_file(
    state: AppState,
    ask: &ConflictAsk,
    file: FoundFile,
    name: Option<String>,
    index: Option<usize>,
) {
    // The two facts only this ask's kind carries: whether the folder reads in
    // place, so the answer lands now, or copies, so it lands after a copy that
    // can fail — and which ledger records the placement.
    let (mode, folder_id) = match &ask.kind {
        AskKind::FolderMerge { mode, folder_id } => (*mode, folder_id.as_deref()),
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
    // A folder that copies: the landing is the import module's own
    // single-file copy composition — the copy made and measured BEFORE the
    // row is promised, the row's identity the copy's own, the ledger write
    // riding the success — and a copy that fails leaves the shelf untouched
    // and the ledger unmarked, the one honest outcome for a file that could
    // not be filed.
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

// ---------------------------------------------------------------------------
// The covered file's answers: a loose import from a read-at-place folder.
// ---------------------------------------------------------------------------
