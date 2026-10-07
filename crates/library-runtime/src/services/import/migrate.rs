//! The one-time move of stored copies into the book's item folder.

use std::collections::HashMap;

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use library_core::book::{Origin, book_rows, book_rows_mut};
use library_core::wire::{BookFileRequest, RelocateResult};

use crate::services as ipc;

/// Fire and forget: an unmoved copy still opens where it is.
pub fn migrate_store_layout(state: crate::context::LibraryContext) {
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

/// Candidates are filtered against the paths the shell answered for.
async fn run(state: crate::context::LibraryContext, candidates: Vec<(String, String)>) {
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
    // id -> (old, new) for the rows whose address actually changed.
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

    // Only the address moves; the bytes have not changed.
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
    crate::services::persist_library(state.library);
}

/// Carry each moved book's cover to its new address.
fn rekey_covers(state: crate::context::LibraryContext, moved: &HashMap<String, (String, String)>) {
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
        crate::services::persist_covers(state.library);
    }
}
