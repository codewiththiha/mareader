//! The one-time move of every stored copy into the book's own item folder.
//!
//! The store used to be flat and name-derived
//! (`<Library>/<format>/<stem>_<id>.<ext>`) and is now one folder per book
//! (`<Library>/items/<id>/source.<ext>`, [`library_core::store`]). Older
//! copies keep the address recorded in their row and still open; this pass
//! moves them.

use std::collections::HashMap;

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use library_core::book::{Origin, book_rows, book_rows_mut};
use library_core::wire::{BookFileRequest, RelocateResult};

use crate::services::library as ipc;
use crate::state::AppState;

/// Fire and forget: a book whose copy could not be moved still opens at the
/// address it has — no reason to interrupt a launch.
pub fn migrate_store_layout(state: AppState) {
    if !tauri_bridge::has_tauri() {
        return;
    }
    let candidates: Vec<(String, String)> = state.library.books.with_untracked(|rows| {
        book_rows(rows)
            .filter(|book| book.origin.is_stored())
            .map(|book| (book.id.clone(), book.path().to_string()))
            .collect()
    });
    if candidates.is_empty() {
        return;
    }
    spawn_local(async move {
        run(state, candidates).await;
    });
}

/// The candidate list is built without the store root: `<app_data_dir>` is
/// the shell's answer, so rows are filtered against the paths the shell
/// actually answered for.
async fn run(state: AppState, candidates: Vec<(String, String)>) {
    let requests: Vec<BookFileRequest> = candidates
        .iter()
        .map(|(id, from)| BookFileRequest {
            id: id.clone(),
            from: from.clone(),
        })
        .collect();
    let answer: RelocateResult = match ipc::relocate_stored(&requests).await {
        Ok(answer) => answer,
        Err(message) => {
            web_sys::console::warn_1(
                &format!("[library] store migration failed: {message}").into(),
            );
            return;
        }
    };
    if answer.root.is_empty() {
        return;
    }
    // id -> (old address, new address) for the rows whose address actually
    // changed: the shell answers an already-migrated row with the address it
    // wore, and rewriting that row would be a write, a persist and a cover
    // re-key for nothing. Matched by id rather than zipped by position, so
    // answer order is not a contract across a process boundary.
    let by_id: HashMap<String, &library_core::wire::StoreResult> = answer
        .results
        .iter()
        .map(|result| (result.id.clone(), result))
        .collect();
    let moved: HashMap<String, (String, String)> = candidates
        .into_iter()
        .filter_map(|(id, from)| {
            let result = by_id.get(&id)?;
            (result.is_ok() && result.store != from).then(|| (id, (from, result.store.clone())))
        })
        .collect();
    if moved.is_empty() {
        return;
    }

    // The address is the only thing that moves: fingerprint, provenance and
    // resume point describe bytes that have not changed.
    let mut rewritten = 0usize;
    state.library.books.update(|rows| {
        for book in book_rows_mut(rows) {
            let Some((_, to)) = moved.get(&book.id) else {
                continue;
            };
            if let Origin::Stored { store, .. } = &mut book.origin {
                *store = to.clone();
                rewritten += 1;
            }
        }
    });
    if rewritten == 0 {
        return;
    }
    rekey_covers(state, &moved);
    crate::storage::persist_library(state.library);
}

/// Carry each moved book's cover across to its new address.
///
/// The cover cache is keyed by address, so a move orphans the art under a key
/// nothing asks about and the card shows a fallback until the book is opened
/// again. A re-key rather than a re-render: same bytes, same page — forty
/// migrated books are not forty renders.
fn rekey_covers(state: AppState, moved: &HashMap<String, (String, String)>) {
    let mut changed = false;
    state.library.covers.update(|covers| {
        for (from, to) in moved.values() {
            let Some(cover) = covers.remove(from) else {
                continue;
            };
            covers.insert(to.clone(), cover);
            changed = true;
        }
    });
    if changed {
        crate::storage::persist_covers(state.library);
    }
}
