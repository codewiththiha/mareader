//! The departure: a read-at-place book leaving the ground that made it
//! becomes the library's own stored copy on the way out — its bytes its own,
//! its identity the copy's own fingerprint, the ORIGINAL fingerprint left
//! free for the folder's log to keep, and its highlights following the
//! address.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use library_core::book::{Book, Origin, find_book_mut, find_row};
use library_core::conflict::same_name;
use library_core::folder::{self as folder_ops, Tombstone};
use library_core::ledger::tombstone;
use library_core::shelf::ALL_SHELF;

use crate::services::library::covers::{self, prune_now};
use crate::services::library::toast;
use crate::services::library as wire;
use crate::state::AppState;
use crate::time::now_ms;

use super::folder_shelf_of;

/// Copy every read-at-place book about to leave its folder's ground, then run
/// the move again over the survivors. Answers whether a conversion started, which
/// is the caller's whole question: it did, so the move has not happened yet and
/// the caller returns.
///
/// One spelling for the three moves that owe a departure — a drag between
/// shelves, a lift out to the root, and the one-row form the conflict sheet rides
/// — because the ORDER is the whole of the rule. The copy is made and the row is
/// converted BEFORE any shelf write happens, so every screen, sheet and
/// membership edit downstream sees the books as what they are about to be rather
/// than as what they were when the hand lifted. Three copies of this were three
/// places to get that order wrong.
///
/// A copy that fails costs that book its move and nothing else: it stays where it
/// was, linked, the toast says so, and the other books in the same drag still go.
/// `retry` runs the move over the survivors and is called from the spawned task,
/// which is what lets the caller return at once and keep its own shape. The
/// copying itself is [`depart`], the one primitive a shelf's departure rides too.
///
/// `retry` takes the survivors AND the rows this gate turned into copies, because
/// the landing owes them one exception: a departure writes a moved-out log, and
/// the copy then lands — often on another shelf of the very folder it left, which
/// is where a reader re-arranging a watched tree puts it. [`bind_returned`] reads
/// a stored book landing on a shelf of the folder it left as the book coming
/// HOME, and a log bound to a row is one a later import answers by lighting that
/// row up instead of bringing the linked book back. One gesture cannot be both
/// the departure and the return, so the rows this gate converted are named to the
/// landing and the bind stands aside for them; a drag of the same row back on a
/// LATER gesture is a return and binds as it always did.
pub(super) fn convert_departures(
    state: AppState,
    ids: &[String],
    to: &str,
    retry: impl FnOnce(Vec<String>, Vec<String>) + 'static,
) -> bool {
    if !tauri_bridge::has_tauri() {
        return false;
    }
    let converting: Vec<String> = ids
        .iter()
        .filter(|id| converts_on_move_to(state, id, to))
        .cloned()
        .collect();
    if converting.is_empty() {
        return false;
    }
    let all: Vec<String> = ids.to_vec();
    spawn_local(async move {
        let departed = depart(state, &converting).await;
        let failed: Vec<String> = converting
            .into_iter()
            .filter(|id| !departed.contains(id))
            .collect();
        let rest: Vec<String> = all.into_iter().filter(|id| !failed.contains(id)).collect();
        if !rest.is_empty() {
            retry(rest, departed);
        }
    });
    true
}

/// Leave governance, for a batch of rows: copy every one of them into the
/// library's own store and write each folder's moved-out log, then answer with
/// the rows that actually left.
///
/// This is the ONE "a read-at-place book is leaving the ground that made it"
/// primitive, and both machines that need it call it rather than each writing
/// their own — a drag or a lift out of a hand-move (`convert_departures` above),
/// and a whole shelf of rungs leaving at once
/// (`crate::services::library::arrange::shelf_departure`). The 90% the two used
/// to duplicate is the loop below: copy, log, count what landed, queue the covers
/// the copies have never had rendered. What is genuinely different between them
/// stays theirs — a hand-move owes a landing and a retry over the survivors, a
/// shelf owes a seam position and an answer about whether the shelf itself comes
/// back as one of the reader's own.
///
/// A copy that fails costs that book its departure and nothing else: it stays
/// where it was, linked, with no log written for it, the reader hears one
/// sentence, and the other rows in the same batch still go. That is why the
/// answer is the rows that LEFT rather than a count — a caller has to know which
/// ones to leave out of the landing it is about to make.
pub(super) async fn depart(state: AppState, rows: &[String]) -> Vec<String> {
    let mut departed: Vec<String> = Vec::with_capacity(rows.len());
    for id in rows {
        match convert_to_stored(state, id).await {
            Ok(()) => departed.push(id.clone()),
            Err(message) => toast(state, message),
        }
    }
    // The copies have never been rendered, and the linked covers they left behind
    // belong to files the rows no longer read. One ask for the batch rather than
    // one per row: the queue re-derives its whole want-list on every call.
    covers::backfill_missing(state);
    departed
}

/// Whether moving this row to this level is a departure that owes a copy: a
/// read-at-place book an in-place folder placed, leaving the rung that folder's
/// own tree names for the file's address.
///
/// The three negatives are as load-bearing as the positive. A STORED book is
/// already the library's own and simply moves. A book no in-place folder placed
/// — a loose file the reader dropped, a book of a copying folder — has no ledger
/// waiting on its fingerprint and moves as a membership. And a re-order on the
/// book's OWN rung is the folder's business: the file is still standing on the
/// ground that made it, so no copy is made and no log is written.
///
/// Everywhere else is a departure, **including another rung of the very folder
/// that placed the book.** What ties a read-at-place book to a folder is the
/// ground its file stands on, not the folder's shelf tree: a book dragged up
/// from `Sci-Fi/` to `Fiction/` is no longer where the folder's ledger says it
/// is, so it becomes the library's own copy on the way out and the ORIGINAL
/// fingerprint goes free for the log to keep. Reading the tie as the tree
/// instead is what let a moved book go on wearing the address — the next import
/// of that same file then found a living row at it, asked the reader to choose
/// between a collision and a highlight, and pointed at the row they had dragged
/// away rather than bringing the file home to the rung it belongs on.
pub(crate) fn converts_on_move_to(state: AppState, row_id: &str, to: &str) -> bool {
    let Some((fp, path)) = state.library.books.with_untracked(|rows| {
        find_row(rows, row_id)
            .and_then(|row| row.book())
            .filter(|book| matches!(book.origin, Origin::Linked { .. }))
            .map(|book| (book.fp, book.path().to_string()))
    }) else {
        return false;
    };
    // One entry per in-place folder whose ledger answers for this fingerprint:
    // the rung that folder names for the address, or `None` when it names none
    // — a rung the reader has deleted since the walk that placed the file, whose
    // book has left the ground all the same. An EMPTY list is therefore the only
    // thing the length says: no ledger is waiting, so no departure is owed.
    let rungs: Vec<Option<String>> = state.library.folders.with_untracked(|folders| {
        folders
            .iter()
            .filter(|f| f.mode().reads_in_place() && f.placed.contains(&fp))
            .map(|f| f.rungs_for(&path).0.map(str::to_string))
            .collect()
    });
    if rungs.is_empty() {
        return false;
    }
    // The root is nobody's rung: "All" is the library's own list rather than a
    // shelf, so no folder's map can name it and a book that lands there has left
    // every ground. Spelled out because it is the departure readers make most,
    // and because reading it off the list alone would leave the answer depending
    // on a map never holding a level that is not a shelf.
    to == ALL_SHELF || !rungs.iter().any(|rung| rung.as_deref() == Some(to))
}

/// Make a read-at-place book the library's own stored copy: the departure
/// half of a move, and the only byte a hand-move ever writes.
///
/// Why a move copies: the book is leaving the ground that made it. Inside its
/// folder's tree the row IS the OS file — the ledger answers for it, a rescan
/// keeps it in place, a removal logs it. On a shelf of its own choosing it can
/// be none of those things without becoming a book the library holds outright,
/// so it becomes one: the bytes go into the store, the row's identity becomes
/// the copy's own measurement, and — the point of the whole rule — the
/// ORIGINAL fingerprint is left free. The folder takes a moved-out log for it,
/// which keeps every rescan quiet, keeps the restore menu honest (the book is
/// not gone), and lets a later import of the OS file bring the linked book
/// back beside the copy that left: two books of one content, each with one
/// address, no twins.
///
/// Everything the reader put into the row travels with it. The visible name
/// moves into `title`, because the store file is named after the row's id and
/// a shelf reading "b1c2d3" is a shelf that renamed the book. The resume point
/// and the format ride the row. The highlights move their key from the old
/// address to the copy's — moved outright when no twin still reads the old
/// address, copied when one does. A measurement of the fresh copy that fails
/// leaves the old fingerprint flagged pending rather than blocking the move:
/// the startup sweep re-measures the store path and finishes the job.
pub(crate) async fn convert_to_stored(state: AppState, row_id: &str) -> Result<(), String> {
    let Some(book) = state.library.books.with_untracked(|rows| {
        find_row(rows, row_id)
            .and_then(|row| row.book())
            .cloned()
    }) else {
        return Err("That book is no longer in the library.".to_string());
    };
    if book.origin.is_stored() {
        // Already the library's own: a second departure of one book must not
        // make a second copy of it.
        return Ok(());
    }
    let path = book.path().to_string();
    // The highlights do not travel: they are keyed by this row's id, which a
    // conversion leaves alone. That is the whole of why the move needed a
    // `migrate_gloss` and no longer does.
    let (store, measured) =
        wire::copy_and_measure(&format!("move-{row_id}"), &path, row_id).await?;

    // The log first, while the row still sits on the folder's shelf: the
    // tombstone records the shelf it was filed on, and that is a fact about
    // the world before the move, not after it.
    write_moved_stones(state, &book, None);

    state.library.books.update(|rows| {
        if let Some(book) = find_book_mut(rows, row_id) {
            book.become_stored(&path, store.clone(), measured);
        }
    });
    // The old address's cover belongs to the file the row no longer reads, and
    // the copy has never been rendered: prune one, queue the other.
    prune_now(state);
    crate::storage::persist_library(state.library);
    Ok(())
}

/// The moved-out log for a read-at-place book: every folder that placed its
/// fingerprint records that the book left as the library's own copy rather
/// than died.
///
/// `returned_row` names the row the file is represented by, for the two
/// answers that dissolve a linked row into a book the library already holds —
/// a merge into its stored copy and a link at one. A conversion leaves it
/// `None`: nothing represents the file yet, and an import of it is owed a real
/// linked book rather than a highlight.
pub(crate) fn write_moved_stones(state: AppState, book: &Book, returned_row: Option<&str>) {
    let home = {
        let shelves = state.library.shelves.get_untracked();
        let folders = state.library.folders.get_untracked();
        folders
            .iter()
            .find(|f| f.placed.contains(&book.fp))
            .and_then(|f| folder_shelf_of(&shelves, &f.id, &book.id))
    };
    // The removal's own constructor, with the two facts a departure adds: this
    // book left as the library's copy rather than died, and — when one answer
    // named it — the row the file is represented by from now on. Spelling all
    // eight fields here instead would be a second place a new `Tombstone` field
    // has to be remembered, and the removal's receipt already owns the first six.
    let entry = Tombstone {
        moved: true,
        returned_row: returned_row.map(str::to_string),
        ..Tombstone::of(book, home, now_ms())
    };
    // `ledger::tombstone` writes it only into the folders that placed the
    // fingerprint and do not already hold a log for it — the removal's own
    // rule, and the right one here.
    state
        .library
        .folders
        .update(|folders| tombstone(folders, &entry));
    crate::storage::persist_library(state.library);
}

/// A stored book landing on a shelf of the folder it once left is a return:
/// the folder's moved-out log binds itself to the row, and from then on an
/// import of the OS file highlights THIS row instead of minting a linked
/// neighbour beside the copy that came home.
///
/// The bind is by NAME, which is the whole of the condition: the log remembers
/// the name the shelf showed, and a row wearing that exact name is the book
/// the reader moved back. A row renamed since the move binds nothing — the
/// folder does not recognise it, the log stays unbound, and a later import of
/// the file simply brings the linked book back and lights it up, which is the
/// honest answer for a name the folder has never seen.
///
/// Callers that seat a row THIS gesture turned into a copy do not reach here at
/// all. A departure writes a log and then lands, and a reader re-arranging a
/// watched tree lands it on another shelf of the same folder — which is the
/// shape of a return without being one, and a bind there would spend the log on
/// the row that just left, so the file could never come home again.
/// [`convert_departures`] names those rows to the landing; [`file_many`] has
/// none to name, because a second membership departs nothing.
pub(super) fn bind_returned(state: AppState, row_id: &str, shelf_id: &str) {
    if shelf_id == ALL_SHELF {
        return;
    }
    let Some(name) = state.library.books.with_untracked(|rows| {
        find_row(rows, row_id)
            .filter(|row| row.book().is_some_and(|b| b.origin.is_stored()))
            .map(|row| row.display_name())
    }) else {
        return;
    };
    let Some(folder_id) = state.library.shelf_folder_id(shelf_id) else {
        return;
    };
    let mut bound = false;
    state.library.folders.update(|folders| {
        let Some(folder) = folder_ops::find_mut(folders, &folder_id) else {
            return;
        };
        if let Some(entry) = folder
            .ignored
            .iter_mut()
            .find(|entry| entry.moved && same_name(&entry.label(), &name))
        {
            if entry.returned_row.as_deref() != Some(row_id) {
                entry.returned_row = Some(row_id.to_string());
                bound = true;
            }
        }
    });
    if bound {
        crate::storage::persist_library(state.library);
    }
}
