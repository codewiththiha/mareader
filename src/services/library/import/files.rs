//! The loose-file run — a picker's or a drop's handful of documents — and
//! the single-file landings every answered sheet rides: a linked landing for
//! the file a folder answered for, the library's own stored copy for the file
//! nobody did, and the ledger's settle for a placement an answer made.

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

/// Import files picked from the dialog or dropped on the library.
///
/// The library's own copies: a loose file has no folder to rescan it and no
/// structure to preserve, and a linked row no ledger answers for is a row no
/// rule can keep honest — so the bytes go into the store, the row's identity
/// is the copy's own measurement, and the source address stays provenance.
/// A file an in-place folder's tree already holds is the exception that asks
/// rather than copies: the folder answers for it, and the import becomes the
/// covered question or, when the folder's log remembers the file, the book
/// coming back in its folder's place (see [`run_files`]). `target` is the
/// shelf a drop landed on; `None` files onto no shelf, which leaves the books
/// in "All" and nowhere else — the honest answer for a handful of loose files.
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

/// The read-at-place folder's answer for every loose file, before the
/// level's name question: a file an in-place tree holds is a file the library
/// already has a book for, and what the drop means is the folder's to say — a
/// logged book comes back where the folder holds it, a standing book asks the
/// covered question, and only a file no tree answers for stays in the walk as
/// an ordinary import. Answers with the rows that came back and the covered
/// questions the reader owes; `found` keeps only the ordinary half.
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
            // Named before the id is moved: the row's own name is what the
            // sheet prints, and reading it after the constructor took the id
            // would be reading a value that is no longer here.
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

/// The library's own answer for every loose file, after the folder's and before
/// the level's name question: a file whose CONTENT the library already holds is a
/// file the reader already has, wherever it is filed and whatever it is called.
///
/// A folder walk has always asked this, through the ledger's registry, which is
/// why re-importing a watched folder reconciles instead of duplicating. A loose
/// file did not, so the same PDF dropped twice landed twice — and the app's own
/// duplicate action could make a third — with no "you already have this" in
/// between. The identity infrastructure was there; it was wired to one door.
///
/// Asked BEFORE the name question because the two are different questions and
/// this one is the stronger: a file whose twin sits on another shelf under another
/// name is not a name collision at all, and answering it as one would offer the
/// reader a second copy of a book they already have. A file no folder answers for
/// and no content matches stays in the walk as an ordinary import.
///
/// A file whose row is MISSING is left in the walk rather than asked about: the
/// honest answer to "the library has this" is a book the reader can be taken to,
/// and a row whose address died is not one. Relinking it is the removal sheet's
/// and the relink dialog's question, not an import's.
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

/// Import loose files: measure them, then answer each one by the ground it
/// stands on — a file of a read-at-place folder is that folder's business
/// first, and the rest land as the library's own stored copies, except the
/// ones whose NAME the target level already holds, which ask (see
/// [`crate::services::library::conflict`]).
///
/// A loose file has no folder to rescan it and no structure to preserve, and
/// a linked row no ledger answers for is a row no rule can keep honest — so
/// the import is a copy: the bytes go into the store, the row's identity is
/// the copy's own measurement, and the source file's fingerprint stays free
/// for any folder that reads it. The one file this does NOT copy silently is
/// a file an in-place folder's tree already holds: that folder already
/// answers for it, so the drop becomes the folder's own two-answer question
/// — the library's copy here, or the folder's book lit where it stands — and
/// a file the folder's log remembers removing or moving out comes BACK in its
/// folder's place instead, the log spent by the explicit ask.
async fn run_files(state: AppState, task: String, paths: Vec<String>, target: Option<String>) {
    let checks = match wire::verify_paths(paths).await {
        Ok(checks) => checks,
        Err(message) => return fail(state, &task, message, FailMode::Toast),
    };
    let mut found: Vec<FoundFile> = checks.iter().filter_map(found_from_check).collect();
    // A file some folder's log says is REPRESENTED by a returned stored copy
    // succeeds the drop by lighting that row up rather than landing a linked
    // neighbour beside it — the folder rule, on the loose-file side. No scope:
    // a loose file has not named the folder whose log answers for it.
    let represented: Vec<String> = take_represented(state, None, &mut found);
    if found.is_empty() && represented.is_empty() {
        return fail(
            state,
            &task,
            "None of those files could be opened.".to_string(),
            FailMode::Toast,
        );
    }
    // Measuring a file the library already holds refreshes its fingerprint and
    // clears a migrated book's pending mark; it has to land before the adds
    // below, which dedupe against exactly those fingerprints.
    apply_checks(state, &checks);

    let shelf_id = target
        .clone()
        .unwrap_or_else(|| shelves_ops::ALL_SHELF.to_string());

    // The read-at-place folder's answer, before the level's name question.
    let (restored, covered_asks) = screen_files(state, &mut found, &shelf_id);
    // The books that came back are on their shelf already; write them before
    // the copies run, so a failure below cannot lose a restoration.
    if !restored.is_empty() {
        crate::storage::persist_library(state.library);
    }

    // The library's own answer, before the level's: content the reader already
    // holds is a question about the library rather than about a name on a shelf.
    let held_asks = screen_content(state, &mut found, &shelf_id);

    // Every file whose NAME the target level already holds is a question
    // rather than a placement — the old rule resolved the file to the row the
    // library already had and skipped the placement, which to the reader was a
    // book swallowed by the shelf it was dropped on. The question is the level's
    // and is about a name, so a file whose twin sits on another shelf is not
    // one: the row the library already has simply gains this level as well,
    // which is a landing the reader can see and the one they asked for by
    // dropping here.
    let arrivals: Vec<Arrival> = found
        .iter()
        .map(|file| Arrival::import(file.clone(), shelf_id.clone(), None))
        .collect();
    let (clean, conflicts) = conflict::screen(state, arrivals);

    // The clean half lands as the library's own copies: one batch for the
    // whole drop rather than one copy per file, and one measurement pass over
    // the copies that landed — a drop of four hundred files is one walk of
    // the store and one write of the blob.
    let now = now_ms();
    let mut pending: Vec<(String, FoundFile, Option<String>, Option<usize>)> = Vec::new();
    let mut stone_landings: Vec<String> = Vec::new();
    for arrival in &clean {
        let Some(file) = arrival.file.clone() else {
            continue;
        };
        // A file some folder remembers removing comes back wearing the name
        // its shelf showed, wherever it lands, and the removal is spent by
        // the explicit ask. An IN-PLACE tree's log never reaches here — the
        // folder's answer above owns those files — so what this lifts is a
        // copying folder's log, or one whose file has since left the tree.
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
                // The questions survive the failure: they are about the
                // library rather than about the store, and every answer can
                // still land — or fail — on its own terms.
                let mut asks = covered_asks;
                asks.extend(held_asks.clone());
                asks.extend(conflicts);
                conflict::raise(state, asks);
                return fail(state, &task, message, FailMode::Toast);
            }
        }
    };
    // The copies' own measurements, in one pass: a stored row's identity is
    // the copy's fingerprint and the source file's stays free — the departure
    // rule's arithmetic on the import's side. A copy that cannot be measured
    // leaves the pending flag, which the startup sweep finishes.
    let stores: Vec<String> = pending
        .iter()
        .filter_map(|(book_id, _, _, _)| copies.get(book_id).cloned())
        .collect();
    let measured = measure_stores(stores).await;
    let mut landed = 0u32;
    for (book_id, file, title, index) in pending {
        // A per-file failure was the batch's own toast; a copy that did not
        // land leaves no row behind.
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
    // A represented file is a succeeded import as much as a landed one: the
    // card says what the drop was worth, and the highlight below says where.
    let placed = landed
        + (restored.len() + stone_landings.len()) as u32
        + represented.len() as u32;
    let waiting = (conflicts.len() + covered_asks.len() + held_asks.len()) as u32;
    // The landed rows may read from addresses the cover cache has no art for:
    // one ask for the batch rather than one per file.
    if placed > 0 {
        covers::backfill_missing(state);
    }
    // One light for the books that CAME BACK — the folder's restorations
    // first, then the landings a log was spent by, then the rows the logs
    // named as represented: "it is back" is worth a highlight and no
    // sentence.
    let came_back = restored
        .into_iter()
        .chain(stone_landings)
        .chain(represented)
        .next();
    if let Some(first) = came_back {
        reveal::reveal_book(state, &first);
    }
    // Raised after the clean half landed: the sheet counts the questions, and
    // a landing that shifted a member list is one the answers resolve against.
    // The order is the order the screens asked in — the folder's ground first,
    // then the library's content, then the level's name.
    let mut asks = covered_asks;
    asks.extend(held_asks);
    asks.extend(conflicts);
    conflict::raise(state, asks);
    crate::storage::persist_library(state.library);
    finish_task(state, &task, placed, waiting);
}

/// Land one measured file as a book row on one level.
///
/// The landing of a file a FOLDER answers for: an in-place merge seating a
/// file its own rung holds, and a restoration putting a logged book back in
/// its folder's place. A loose import does not come through here — a file no
/// folder answers for is the library's own stored copy
/// ([`land_stored_copy`]), because a linked row no ledger keeps is a row no
/// rescan can heal, tombstone or hand back. `name` is the title the row is
/// given: `None` leaves it without one, so the stem of its address is the name
/// the shelf shows, which is the honest name for a file nobody has opened yet,
/// and `Some` is the name a log or a sheet gave it.
///
/// A second row of an address the library already reads is a book of its own
/// ([`Book::independent`]): its highlights live under a key carrying its id and
/// its resume point is written by itself, which is what makes "add as new"
/// mean something a reader can see rather than a second name for one book.
///
/// The row is always a new one, named or not. Either way the reader asked for
/// a book HERE, and a resolve to the row another level holds would answer
/// with nothing new on the level they dropped on — the vanishing the
/// collision sheet exists to stop.
///
/// Writes no persist and queues no cover, because a caller that lands four
/// hundred files owes ONE write and ONE cover ask — the queue re-derives its
/// whole want-list on every call — and only the caller knows whether this is
/// one file or four hundred.
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

/// Land a file whose bytes the app copied into its store: the row
/// [`land_file`] lands, at a stored address, under an id minted BEFORE the
/// copy — the stored file is named after it, and a mint afterwards would
/// leave two copies of one name fighting over one slot in the store.
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

/// Land one loose file as the library's own stored copy: the single-file form
/// of the batch [`run_files`] lands, for the two answers that place a file
/// after a question — the name sheet's *add as new* and the covered sheet's
/// *import here*.
///
/// The copy is made BEFORE the row is promised — a failure to copy is a toast
/// and a level left untouched, the one honest outcome for a file that could
/// not be filed — and the id is minted now because the stored file is named
/// after it. The row then takes the copy's own measurement as its identity
/// ([`adopt_copy_measurement`]), which is what leaves the source file's
/// fingerprint free for the folders that read it.
pub(crate) fn land_stored_copy(
    state: AppState,
    file: FoundFile,
    name: Option<String>,
    shelf_id: String,
    index: Option<usize>,
) {
    land_stored_copy_settling(state, file, name, shelf_id, index, None);
}

/// [`land_stored_copy`] with the folder's ledger write riding the landing:
/// `settle` is the `(folder, fingerprint)` a merged folder's copy answer owes
/// when the book lands — the placement recorded and the removal spent, through
/// [`settle_ledger`]. The copy is made and MEASURED before the row is promised
/// (the row's identity is the copy's own, the source file's fingerprint stays
/// free), a failure to copy leaves the shelf untouched and the ledger unmarked,
/// and one spelling serves both the loose-import answers and the folder
/// sheet's, which used to hand-roll this same sequence each.
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

/// The copy's own measurement becomes the row's identity, and the source
/// file's fingerprint is left free — the departure rule's arithmetic on the
/// import's side. A stored book is the library's own instance, and an OS file
/// that keeps its own fingerprint can still be placed by any folder that
/// reads it, as its own linked book, instead of being answered away as
/// "content the library holds" by a registry that only saw the store's copy
/// of it. A copy that could not be measured leaves the pending flag rather
/// than blocking the landing; the startup sweep re-measures the store path
/// and finishes the job.
pub(super) fn adopt_copy_measurement(state: AppState, row_id: &str, measured: Option<Fingerprint>) {
    state.library.books.update(|rows| {
        if let Some(book) = find_book_mut(rows, row_id) {
            book.adopt_measurement(measured);
        }
    });
}

/// The row both landings mint.
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
    // Always a row of its own, and never a resolve to the row the library
    // already holds: the reader asked for THIS file on THIS level, and filing
    // another level's row here leaves the level they dropped on with a book
    // that is not its own — one removal from both shelves, one resume point
    // between them, and a shelf that shows a book the reader never put there.
    // Content identity is the ledger's business, which is a rescan's and a
    // watched folder's; it is not an answer to a hand. What keeps two rows of
    // one file honest is the mark above, not a dedupe here.
    let placed = book.id.clone();
    state.library.books.update(|rows| rows.push(Row::Book(book)));
    // The root has no member list to place into, so the write is skipped rather
    // than made and answered with nothing: a batch landing four hundred files
    // "on All" would otherwise notify the shelf list four hundred times.
    if shelf_id != shelves_ops::ALL_SHELF {
        state.library.shelves.update(|shelves| {
            if let Some(shelf) = shelves_ops::find_mut(shelves, shelf_id) {
                shelves_ops::place(&mut shelf.books, &placed, index);
            }
        });
    }
    placed
}

/// A path check turned into a found file, so loose files and a folder walk feed
/// the same placement code. `None` for an address that did not resolve, or one
/// the format registry does not know.
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

/// The folder's ledger half of a landed file: the placement is recorded, and
/// a removal that was holding the file out is spent — the two writes
/// `run_folder` makes when a file lands, made here because this file landed
/// after an ANSWER (a sheet's copy, a covered restore) rather than after a
/// walk. One spelling, so a placement cannot be recorded anywhere without the
/// removal being spent beside it.
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
