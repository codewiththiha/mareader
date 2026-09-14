//! The books a folder gives back: the restore menu's own answer, the
//! covered file whose folder log remembers it, and the files a moved-out log
//! REPRESENTS — an import of those succeeds by lighting the row the log names
//! rather than by landing a neighbour beside it.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use library_core::book::{add_book, book_rows, find_row, Book, Fingerprint, Origin};
use library_core::folder::{self as folder_ops, rel_under, Tombstone};
use library_core::id;
use library_core::ledger;
use library_core::scan::FoundFile;
use library_core::shelf::{self as shelves_ops};
use reader_core::format::Format;

use super::files::{found_from_check, land_file, settle_ledger};
use super::tasks::{fail, finish_task, push_task, task_id, FailMode};
use super::root_shelf_of;
use crate::services::library::covers;
use crate::services::library as wire;
use crate::state::library::ImportTask;
use crate::state::AppState;
use crate::time::now_ms;

/// The walked files a moved-out log already REPRESENTS, taken out of the
/// walk: a file whose log binds itself to a LIVING row is an import that
/// succeeds by lighting that row up, not by landing a linked neighbour beside
/// the copy that came home. Answers the row ids; the files leave `found`.
///
/// `scope` narrows the search to ONE folder's log — a folder walk asks only
/// its own ledger — and `None` searches every log, the loose-drop shape of
/// the question, where no folder has been named yet. A binding that names a
/// dead row is spent of its meaning, and the file stays in the walk to take
/// the ordinary import route, which lifts the log when the book lands.
pub(super) fn take_represented(
    state: AppState,
    scope: Option<&str>,
    found: &mut Vec<FoundFile>,
) -> Vec<String> {
    let mut represented = Vec::new();
    found.retain(|file| {
        let Some(row_id) = state.library.folders.with_untracked(|folders| {
            folders
                .iter()
                .filter(|folder| scope.is_none_or(|id| folder.id == id))
                .find_map(|folder| {
                    ledger::find_tombstone(folder, &file.fp)
                        .and_then(|entry| entry.returned_row.clone())
                })
        }) else {
            return true;
        };
        let alive = state
            .library
            .books
            .with_untracked(|rows| find_row(rows, &row_id).is_some());
        if alive {
            represented.push(row_id);
            false
        } else {
            true
        }
    });
    represented
}

/// Put a removed book back, from the folder's own import menu.
///
/// Not a rescan with the tombstone lifted: an explicit restore is an explicit
/// choice, so it honours the folder's read-in-place-or-copy answer and ignores the
/// format and size filters that a passive scan applies — a reader who removed a
/// 12 KB text file and then asks for it back is not asking to be told it is too
/// small. The file is measured first, and a measurement that comes back empty
/// leaves the tombstone exactly where it was, because losing it would lose the
/// only record the book was ever there.
pub fn restore_deleted_book(state: AppState, folder_id: String, fp: Fingerprint) {
    // The folder's options decide read-in-place-or-copy; its root does not appear,
    // because a restore measures the address the tombstone recorded rather than
    // assuming the file is still where the folder put it.
    let taken = state.library.folders.with_untracked(|folders| {
        folder_ops::find(folders, &folder_id).and_then(|f| {
            ledger::find_tombstone(f, &fp)
                .map(|entry| (f.opts.clone(), entry.clone()))
        })
    });
    let Some((opts, entry)) = taken else {
        return;
    };

    let task = task_id();
    push_task(state, ImportTask::new(task.clone(), entry.label()));
    spawn_local(async move {
        let checks = match wire::verify_paths(vec![entry.last_path.clone()]).await {
            Ok(checks) => checks,
            Err(message) => return fail(state, &task, message, FailMode::Toast),
        };
        let Some(found) = checks.first().and_then(found_from_check) else {
            return fail(
                state,
                &task,
                format!("{} is not there any more.", entry.label()),
                FailMode::Toast,
            );
        };

        let now = now_ms();
        let book_id = id::next_id(now);
        let (origin, measured) = if opts.mode().reads_in_place() {
            (
                Origin::Linked {
                    src: found.path.clone(),
                },
                None,
            )
        } else {
            match wire::copy_and_measure(&task, &found.path, &book_id).await {
                Ok((store, measured)) => (
                    Origin::Stored {
                        src: Some(found.path.clone()),
                        store,
                    },
                    measured,
                ),
                Err(message) => return fail(state, &task, message, FailMode::Toast),
            }
        };

        let mut book = Book {
            // The name the shelf showed before the removal, so a restored book
            // comes back as the book the reader remembers rather than as a
            // file stem.
            title: entry.title.clone(),
            // The file's fingerprint as it is NOW, which is not necessarily the
            // one the tombstone carries: a book can be edited between being
            // removed and being asked for back.
            ..Book::new(
                book_id,
                found.fp,
                found.format().unwrap_or(Format::Pdf),
                origin,
                now,
            )
        };
        // A restored COPY wears the copy's own measurement, the way every
        // stored landing does: its identity is its own bytes and the source
        // file's fingerprint stays free for the folder's ledger (which is
        // marked with it below). A copy that could not be weighed keeps the
        // pending flag the startup sweep finishes — not the source's stamp.
        if opts.mode().copies_files() {
            book.adopt_measurement(measured);
        }
        let mut placed_id = String::new();
        state.library.books.update(|books| {
            placed_id = add_book(books, book);
        });

        // The removal comes out only now that the book is back, and `placed` goes
        // in at the same moment: a fingerprint the ledger skips with no book
        // behind it is the one state a folder cannot recover from on its own.
        let stale = entry.fp;
        state.library.folders.update(|folders| {
            let Some(folder) = folder_ops::find_mut(folders, &folder_id) else {
                return;
            };
            ledger::restore_deleted(folder, &stale);
            folder.mark_placed(found.fp);
            if found.fp != stale {
                // The file changed while it was gone, so the old fingerprint's
                // tombstone describes a file that no longer exists. Drop it rather
                // than leave a restore row that measures nothing.
                folder.ignored.retain(|t| t.fp != found.fp);
            }
        });

        // Back on the shelf it was filed on, or on the folder's root shelf if that
        // shelf has since gone: a restored book should not come back somewhere new.
        state.library.shelves.update(|shelves| {
            let known = entry
                .shelf_id
                .as_deref()
                .filter(|id| shelves.iter().any(|s| s.id == **id));
            let target = known
                .map(str::to_string)
                .or_else(|| root_shelf_of(shelves, &folder_id));
            let Some(shelf_id) = target else {
                return;
            };
            if let Some(shelf) = shelves_ops::find_mut(shelves, &shelf_id) {
                shelves_ops::shelf_add(shelf, &placed_id);
            }
        });
        crate::storage::persist_library(state.library);
        // A removed book took its cover with it; a restored one gets it back
        // without asking to be opened first.
        covers::backfill_missing(state);
        finish_task(state, &task, 1, 0);
    });
}

/// What the read-at-place folder this file stands in already says about a
/// loose import of it.
#[derive(Debug)]
pub(super) enum CoveredFate {
    /// No in-place folder's tree holds this address: the file is an ordinary
    /// import, and lands as the library's own copy through the level's name
    /// question. A tree that covers the ground but never placed THIS file —
    /// new since the last scan, or outside the folder's filters — answers
    /// here too: the import is the library's copy, and the folder places its
    /// own linked book on the walk that finds it, as it always would.
    Ordinary,
    /// A folder that holds the file has an answer for it — a log, or the
    /// folder's own `placed` set standing in for a log an older build dropped —
    /// and an explicit import spends it the way a folder walk does: the book
    /// comes back as the folder's own linked book, in its folder's place,
    /// wearing the name the shelf showed, and the run lights it up. A restore
    /// with no log behind it writes no ledger entry, so it is the folder's rung
    /// rather than a remembered shelf that says where the book comes back to.
    Restore { folder_id: String, stone: Tombstone },
    /// The folder's book for this file is alive and standing: the import is
    /// the covered question — the library's own stored copy on this level,
    /// or the folder's book lit where it stands — because a second linked
    /// row of one read-at-place file is the one thing the folder rule never
    /// makes.
    Ask { folder_id: String, row_id: String },
}

/// Ask the in-place folders what they hold for one loose file.
///
/// A living row at the file's very address answers FIRST, and the order is
/// the walk's own rather than a preference: an explicit folder run reads the
/// registry before it reads the logs (`decide_import`), and a log standing
/// beside a living row of its fingerprint is a stale state the next walk's
/// `prune_tombstones` drops — a hand-open between the removal and the import
/// is how it happens. Answering the row writes nothing, so it cannot
/// duplicate the book that is there; answering the log would mint a second
/// linked row over it, which is the one thing the covered question exists to
/// prevent. The row IS the folder's book — a linked row stays in its
/// folder's tree, because every departure converts it — and an import of the
/// file is the question, never a second linked instance beside it.
///
/// The LOG answers second: a removal, or a moved-out log whose copy has
/// since died, is spent by the explicit import, and the book comes back in
/// its folder's place, exactly as a walk re-importing the folder brings it.
/// (A moved-out log BOUND to a living copy was spent by the `represented`
/// check before this runs, so it never reaches here.)
///
/// The folder's own `placed` set answers third, and only when no log does: a
/// fingerprint this folder placed, with no row at the address and nothing in
/// the ledger, is a book that left and lost its paperwork. Two ways happen —
/// a storage trim dropped the row, and a departure logged by a build whose
/// copy wore the SOURCE's fingerprint, which the next walk's prune then read
/// as a book come back and dropped. Both owe the answer the log would have
/// given, and the alternative is a second stored copy of a file this folder
/// reads in place, landed beside the copy that left. A folder that never
/// placed the file answers `Ordinary` exactly as before: a log is the third
/// arm's evidence, and `placed` is what stands in for one.
pub(super) fn covered_fate(state: AppState, file: &FoundFile) -> CoveredFate {
    // The in-place folders whose tree holds the file's address.
    let covering: Vec<String> = state.library.folders.with_untracked(|folders| {
        folders
            .iter()
            .filter(|f| f.mode().reads_in_place() && rel_under(&file.path, &f.root).is_some())
            .map(|f| f.id.clone())
            .collect()
    });
    if covering.is_empty() {
        return CoveredFate::Ordinary;
    }
    let row_id = state.library.books.with_untracked(|rows| {
        book_rows(rows)
            .find(|b| b.path() == file.path)
            .map(|b| b.id.clone())
    });
    if let Some(row_id) = row_id {
        // The folder the sheet names is the one that PLACED the file; a tree
        // whose ledger lost the fingerprint — the file changed since the walk
        // that placed it — falls back to the first that covers the address.
        let folder_id = state
            .library
            .folders
            .with_untracked(|folders| {
                covering
                    .iter()
                    .find(|id| {
                        folders
                            .iter()
                            .any(|f| &f.id == *id && f.placed.contains(&file.fp))
                    })
                    .cloned()
            })
            .unwrap_or_else(|| covering[0].clone());
        return CoveredFate::Ask { folder_id, row_id };
    }
    let stoned = state.library.folders.with_untracked(|folders| {
        covering.iter().find_map(|id| {
            folder_ops::find(folders, id)
                .and_then(|f| ledger::find_tombstone(f, &file.fp).cloned())
                .map(|stone| (id.clone(), stone))
        })
    });
    if let Some((folder_id, stone)) = stoned {
        return CoveredFate::Restore { folder_id, stone };
    }
    // No row and no log, so the folder's own membership is the evidence: it
    // PLACED this fingerprint, which means a linked book of this file stood
    // here and is not standing now. The name comes from the copy the library
    // holds of this very address when there is one, because a departure moved
    // the shelf's name into it and the book that comes back should wear the
    // name the reader remembers rather than the file's stem.
    let placed_by = state.library.folders.with_untracked(|folders| {
        covering
            .iter()
            .find(|id| {
                folders
                    .iter()
                    .any(|f| &f.id == *id && f.placed.contains(&file.fp))
            })
            .cloned()
    });
    match placed_by {
        Some(folder_id) => {
            let title = state.library.books.with_untracked(|rows| {
                book_rows(rows)
                    .find(|b| b.origin.is_store_copy_of(&file.path))
                    .and_then(|b| b.title.clone())
            });
            CoveredFate::Restore {
                folder_id,
                stone: Tombstone {
                    fp: file.fp,
                    title,
                    format: file.format().unwrap_or(Format::Pdf),
                    last_path: file.path.clone(),
                    // No shelf to remember, so the landing falls through to
                    // the folder's own mapped rung for this file's subfolder —
                    // the ground the book left, which is the answer the log
                    // would have given.
                    shelf_id: None,
                    removed_ms: now_ms(),
                    moved: true,
                    returned_row: None,
                },
            }
        }
        None => CoveredFate::Ordinary,
    }
}

/// The write half of [`CoveredFate::Restore`]: the folder's book comes back
/// the way a folder walk brings it back — a LINKED book at the file's
/// address, wearing the name the shelf showed, on the folder's own ground —
/// and the log is spent by the landing, when there is one to spend. Returns
/// the row so the run can light it up.
///
/// The shelf is the one the log remembers when it still stands, then the
/// folder's mapped rung for the file's subfolder, then the folder's root
/// shelf: a book that came back should not come back somewhere new, and
/// least of all on the level the file happened to be dropped on — the drop
/// asked for a file the folder owns, and the folder's place is the answer.
/// A restore with no log behind it — a departure whose paperwork an older
/// build dropped — has no shelf to remember and starts at the rung, which is
/// the same answer the log would have given. A folder with no shelf left at
/// all leaves the book in the library unfiled, which is the restore menu's
/// own fallback.
pub(super) fn restore_covered_file(
    state: AppState,
    file: &FoundFile,
    folder_id: &str,
    stone: &Tombstone,
) -> String {
    // The folder's two rungs for this file: the one its subfolder maps to,
    // and the one at its root. The ledger's own arithmetic — the same one that
    // decides whether a book has left the folder's ground — so a book comes
    // back to the rung it departed from rather than to wherever a second copy
    // of that lookup happens to land.
    let (rung, root_rung) = state.library.folders.with_untracked(|folders| {
        folder_ops::find(folders, folder_id)
            .map(|f| {
                let (rung, root) = f.rungs_for(&file.path);
                (rung.map(str::to_string), root.map(str::to_string))
            })
            .unwrap_or_default()
    });
    let target = state.library.shelves.with_untracked(|shelves| {
        let standing = |id: &Option<String>| {
            id.as_deref()
                .filter(|sid| shelves.iter().any(|s| s.id == **sid))
                .map(str::to_string)
        };
        standing(&stone.shelf_id)
            .or_else(|| standing(&rung))
            .or_else(|| standing(&root_rung))
            .or_else(|| root_shelf_of(shelves, folder_id))
    });
    // The log comes out and the placement is marked in one write — the
    // ledger's own settle, the one spelling of the pair: a fingerprint the
    // ledger skips with no book behind it is the one state a folder cannot
    // recover from on its own. (`placed` kept the fingerprint through the
    // removal and the departure alike; the mark is the guarantee.)
    settle_ledger(state, Some(folder_id), file.fp);
    let shelf_id = target.unwrap_or_else(|| shelves_ops::ALL_SHELF.to_string());
    land_file(state, file, stone.title.clone(), &shelf_id, None)
}

/// The removal a folder holds for this file, lifted — `None` when no folder
/// remembers removing it.
///
/// The match is the FINGERPRINT, not the address: a file that was removed
/// from a folder, moved across the disk by the OS or by hand, and dropped
/// back into the library is the same file the log was written for, and an
/// explicit import of it is the reader asking for that book again. The log
/// that kept a watchful rescan from resurrecting the book is spent by the
/// ask; what comes back is the book, in its old name, with a highlight.
pub(super) fn lift_stone_for(state: AppState, file: &FoundFile) -> Option<Tombstone> {
    let owner = state.library.folders.with_untracked(|folders| {
        folders
            .iter()
            .find(|f| ledger::find_tombstone(f, &file.fp).is_some())
            .map(|f| f.id.clone())
    })?;
    let mut stone = None;
    state.library.folders.update(|folders| {
        if let Some(folder) = folder_ops::find_mut(folders, &owner) {
            stone = ledger::restore_deleted(folder, &file.fp);
        }
    });
    stone
}
