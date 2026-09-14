//! The books a folder gives back: the restore menu's own answer, and the files a moved-out log
//! REPRESENTS — an import of those succeeds by lighting the row the log names rather than by landing
//! a neighbour beside it.

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

/// A file whose log binds itself to a LIVING row is an import that succeeds by lighting that
/// row up, not by landing a linked neighbour beside the copy that came home. `scope` narrows
/// the search to ONE folder's log.
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

/// Not a rescan with the tombstone lifted: an explicit restore honours the folder's
/// read-in-place-or-copy answer and ignores the format and size filters a passive scan
/// applies.
pub fn restore_deleted_book(state: AppState, folder_id: String, fp: Fingerprint) {
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
            title: entry.title.clone(),
            ..Book::new(
                book_id,
                found.fp,
                found.format().unwrap_or(Format::Pdf),
                origin,
                now,
            )
        };
        if opts.mode().copies_files() {
            book.adopt_measurement(measured);
        }
        let mut placed_id = String::new();
        state.library.books.update(|books| {
            placed_id = add_book(books, book);
        });

        // One write: a fingerprint the ledger skips with no book behind it is the one state a folder cannot recover from on its own.
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
        covers::backfill_missing(state);
        finish_task(state, &task, 1, 0);
    });
}

#[derive(Debug)]
pub(super) enum CoveredFate {
    /// A tree that covers the ground but never placed THIS file answers here too: the folder places its own linked book on the walk that finds it.
    Ordinary,
    /// An explicit import spends the log the way a folder walk does: the book comes back as the folder's own linked book, in its folder's place, wearing the name the shelf showed.
    Restore { folder_id: String, stone: Tombstone },
    Ask { folder_id: String, row_id: String },
}

/// A living row at the file's very address answers FIRST, and the order is the walk's own
/// rather than a preference: an explicit folder run reads the registry before it reads the
/// logs.
pub(super) fn covered_fate(state: AppState, file: &FoundFile) -> CoveredFate {
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

/// The folder's book comes back the way a folder walk brings it back — a LINKED book at the
/// file's address, wearing the name the shelf showed, on the folder's own ground — and the log
/// is spent by the landing. The shelf is the one the log remembers when it still stands.
pub(super) fn restore_covered_file(
    state: AppState,
    file: &FoundFile,
    folder_id: &str,
    stone: &Tombstone,
) -> String {
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
    // The ledger's own settle, the one spelling of the pair: a fingerprint the ledger skips with no book behind it is the one state a folder cannot recover from on its own.
    settle_ledger(state, Some(folder_id), file.fp);
    let shelf_id = target.unwrap_or_else(|| shelves_ops::ALL_SHELF.to_string());
    land_file(state, file, stone.title.clone(), &shelf_id, None)
}

/// The match is the FINGERPRINT, not the address: a file removed from a folder, moved across
/// the disk, and dropped back into the library is the same file the log was written for.
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
