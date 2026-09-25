//! A dead address, re-pointed: the reader picks a file — or a FOLDER, and the app walks it
//! looking for the book's own name — and the book reads from there. A linked book takes the
//! address, a stored book takes a fresh copy of it, made before anything is written so a
//! failure leaves the row exactly as it was.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use app_chrome::dialog::CANCELLED;
use library_core::book::{Origin, find_book_mut, find_row, stem_of};
use library_core::folder::FolderOpts;
use library_core::scan::selectable_formats;

use super::super::{file_name, pick_folder};
use crate::services as ipc;
use crate::services::covers::{self, prune_now};
use crate::services::import;
use crate::services::toast;
use crate::state::library::RelinkAsk;

/// A linked book takes the new address. A stored book does NOT become linked — that would
/// quietly turn "the app keeps its own copy" back into "the app reads your folder again" —
/// so the pick is copied into the store once more, from wherever the file lives now.
///
/// The new copy is MEASURED before the row is written to it: the backend stamps a copy with
/// its own time, so healing the row with the PICKED file's fingerprint would leave it
/// describing a file it does not stand at — the next verify pass would mark and heal it
/// unpredictably, and the ledger's placed-matching could mis-fire. The measurement the
/// copy came home with is the row's, exactly as every other landing takes it.
fn relink_book(state: crate::context::LibraryContext, book_id: String, path: String) {
    relink_book_on(state, book_id, path, None);
}

/// [`relink_book`] when a card for the gesture is already up — a Find-again search that
/// ends in a relink keeps the one card its scan started, rather than lighting a second
/// beside the first for the copy the first was leading to.
fn relink_book_on(
    state: crate::context::LibraryContext,
    book_id: String,
    path: String,
    on_task: Option<String>,
) {
    if !tauri_bridge::has_tauri() {
        return;
    }
    spawn_local(async move {
        let name = state.library.row_name(&book_id);
        let task = on_task.unwrap_or_else(|| import::begin_task(state, name));
        let checks = match ipc::verify_paths(vec![path.clone()]).await {
            Ok(checks) => checks,
            Err(message) => return import::fail_task(state, &task, message),
        };
        let Some(fp) = checks.first().and_then(|c| c.fingerprint()) else {
            return import::fail_task(
                state,
                &task,
                "That file is not there any more. Pick the book's current location.".to_string(),
            );
        };
        let origin = state.library.books.with_untracked(|rows| {
            library_core::book::find_by_id(rows, &book_id).map(|b| b.origin.clone())
        });
        let Some(origin) = origin else {
            return;
        };

        let stored = match origin {
            Origin::Linked { .. } => None,
            Origin::Stored { .. } => match ipc::copy_one(&task, &path, &book_id).await {
                Ok((store, measured)) => Some((store, measured)),
                Err(message) => return import::fail_task(state, &task, message),
            },
        };

        state.library.books.update(|rows| {
            let Some(book) = find_book_mut(rows, &book_id) else {
                return;
            };
            book.heal(fp);
            match &mut book.origin {
                Origin::Linked { src } => *src = path.clone(),
                Origin::Stored { src, store: at } => {
                    *src = Some(path.clone());
                    if let Some((store, measured)) = stored.as_ref() {
                        *at = store.clone();
                        // A copied file's own measurement supersedes the
                        // picked source's fingerprint. With no measurement,
                        // healing keeps the same fallback as before.
                        if measured.is_some() {
                            book.adopt_measurement(*measured);
                        }
                    }
                }
            }
        });
        state.library.relink.dismiss();
        prune_now(state);
        crate::services::persist_library(state.library);
        crate::services::persist_covers(state.library);
        covers::backfill_missing(state);
        import::finish_task(state, &task, 1, 0);
    });
}

/// The platform's picker rather than a second dialog implementation: the
/// same question with the same filter. Cancel is compared against
/// chrome's constant, so a wording change on either side is a compile error
/// rather than a cancel that quietly turns into an error toast.
pub fn relink_dialog(state: crate::context::LibraryContext, book_id: String) {
    spawn_local(async move {
        match app_chrome::dialog::pick_document().await {
            Ok(path) => relink_book(state, book_id, path),
            Err(message) if message == CANCELLED => {}
            Err(message) => toast(state, message),
        }
    });
}

/// A reader-page open (the sheet lives on the library page) falls back to
/// the file picker itself — the answer the sheet's first row would have run.
pub fn ask_relink(state: crate::context::LibraryContext, book_id: String) {
    let name = state.library.row_name(&book_id);
    state.library.relink.raise(RelinkAsk { book_id, name });
}

/// The book stays missing, its row and its shelf memberships stay exactly as they were.
pub fn cancel_relink(state: crate::context::LibraryContext) {
    state.library.relink.dismiss();
}

/// The walk is the shell's own (`scan_folder`: one measurement per file,
/// every format, no size floor — a lost book is not a file to filter).
pub fn relink_search_folder(state: crate::context::LibraryContext, book_id: String) {
    spawn_local(async move {
        let known = state.library.books.with_untracked(|rows| {
            find_row(rows, &book_id)
                .and_then(|row| row.book())
                .map(|book| (book.title(), book.path().to_string()))
        });
        let Some((name, old_path)) = known else {
            return;
        };
        let root = match pick_folder().await {
            Ok(Some(root)) => root,
            Ok(None) => return,
            Err(message) => return toast(state, message),
        };
        // One card for the whole gesture: the scan's beats and the copy's beats land on it
        // in sequence, which is what "looking for the book, then bringing it home" reads as.
        let task = import::begin_task(state, name.clone());
        let opts = FolderOpts {
            formats: selectable_formats().into_iter().collect(),
            include_selected: true,
            min_size: 0,
            ..FolderOpts::default()
        };
        let found = match ipc::scan_folder(&task, &root, &opts).await {
            Ok(found) => found,
            Err(message) => return import::fail_task(state, &task, message),
        };
        match found
            .iter()
            .find(|file| is_the_book(&file.path, &name, &old_path))
        {
            Some(file) => {
                relink_book_on(state, book_id, file.path.clone(), Some(task));
            }
            None => {
                toast(
                    state,
                    format!("Nothing called “{name}” inside that folder."),
                );
                // Nothing found is no run to report: the toast carries the news, and the
                // card goes rather than finishing on a count it never counted.
                import::dismiss_task(state, &task);
            }
        }
    });
}

/// Case aside: a folder that answers in capitals is still the folder the
/// book lives in. Content is nobody's question here — the relink that
/// follows re-measures the file.
fn is_the_book(found_path: &str, name: &str, old_path: &str) -> bool {
    let stem = stem_of(found_path);
    let file = file_name(found_path);
    let old_stem = stem_of(old_path);
    let old_file = file_name(old_path);
    [name, old_stem.as_str(), old_file.as_str()]
        .into_iter()
        .any(|known| stem.eq_ignore_ascii_case(known) || file.eq_ignore_ascii_case(known))
}

#[cfg(test)]
mod tests {
    use super::is_the_book;

    #[test]
    fn the_name_the_shelf_shows_finds_the_book() {
        assert!(is_the_book("/found/Dune.pdf", "Dune", "/old/gone.pdf"));
        assert!(is_the_book("/found/DUNE.pdf", "Dune", "/old/gone.pdf"));
    }

    #[test]
    fn the_address_it_used_to_wear_finds_the_book() {
        assert!(is_the_book(
            "/found/mathematical-proofs.pdf",
            "A Book",
            "/old/mathematical-proofs.pdf"
        ));
    }

    #[test]
    fn the_file_name_finds_the_book_extension_and_all() {
        assert!(is_the_book(
            "/found/notes.md",
            "nothing alike",
            "/old/notes.md"
        ));
        assert!(is_the_book("/found/report.pdf", "x", "/old/report.docx"));
    }

    #[test]
    fn a_neighbour_of_another_name_is_not_the_book() {
        assert!(!is_the_book(
            "/found/dune-messiah.pdf",
            "Dune",
            "/old/dune.pdf"
        ));
        assert!(!is_the_book("/found/other.pdf", "Dune", "/old/gone.pdf"));
    }
}
