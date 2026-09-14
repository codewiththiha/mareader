//! The one-time move of every stored copy into the book's own item folder.
//!
//! The store used to be flat and name-derived — `<Library>/<format>/<stem>_<id>.<ext>`
//! — and is now one folder per book (`<Library>/items/<id>/source.<ext>`,
//! [`library_core::store`]). Copies made since that change are already there;
//! copies made before it keep the address recorded in their row, so they still
//! open, but they sit in a layout nothing writes any more and have no directory
//! of their own for a cover or a set of marks to live in.
//!
//! This is the pass that brings them across, and it is self-limiting rather than
//! run-once: the rows it moves are written back, so the next launch finds nothing
//! in the old buckets and asks for nothing. A row the shell could not move keeps
//! the address it has, which still opens, and is offered again next launch — a
//! migration that gave up on a locked file permanently would strand a book in a
//! bucket for one busy moment.
//!
//! Two ordering rules, both load-bearing:
//!
//!   * it runs BEFORE the startup measurement pass. A move changes the address a
//!     row holds, and measuring the old one would mark the book `missing` for a
//!     file this pass is about to put somewhere else;
//!   * it leaves the copy's modification time alone. A stored row's identity is
//!     the measurement of its OWN bytes, and every ledger entry, tombstone and
//!     `placed` fingerprint names that measurement — re-stamping on a migration
//!     would change a fingerprint the folder ledgers still hold, and turn every
//!     stored book into a file its own folder has never seen.

use std::collections::HashMap;

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use library_core::book::{book_rows, book_rows_mut, Origin};
use library_core::wire::RelocateRequest;

use crate::services::library as wire;
use crate::state::AppState;

/// Move every stored copy still sitting in the old flat buckets into its item
/// folder, and write the rows that moved. Fire and forget: a book whose copy
/// could not be moved still opens at the address it has, which is a worse shelf
/// than the migration was for and not a reason to interrupt a launch.
pub fn migrate_store_layout(state: AppState) {
    if !tauri_bridge::has_tauri() {
        return;
    }
    // Every stored row, with the address its copy wore when this pass started —
    // which is also the key its cover is cached under, and the half of the
    // question the answer cannot supply.
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

/// One pass: ask the shell to move the copies, then rewrite the rows whose copy
/// landed somewhere new.
///
/// The candidate list is built WITHOUT the store root, because the frontend
/// cannot compute one — `<app_data_dir>` is the shell's answer — and the pass
/// that does the moving hands it back. So the rows are filtered here, against the
/// paths the shell actually answered for, rather than before the call: a row
/// already in its item folder travels along and comes back wearing the address it
/// had, which is one entry in a batch and not one decision got wrong.
async fn run(state: AppState, candidates: Vec<(String, String)>) {
    let requests: Vec<RelocateRequest> = candidates
        .iter()
        .map(|(id, from)| RelocateRequest {
            id: id.clone(),
            from: from.clone(),
        })
        .collect();
    let answer = match wire::relocate_stored(&requests).await {
        Ok(answer) => answer,
        Err(message) => {
            web_sys::console::warn_1(&format!("[library] store migration failed: {message}").into());
            return;
        }
    };
    if answer.root.is_empty() {
        // No store directory on this host: nothing moved, and nothing will until
        // there is one. Not worth a sentence — every row still opens.
        return;
    }
    // id -> where its copy lived and where it lives now, for the rows whose
    // address actually changed. The shell answers an already-migrated row with
    // the address it wore, and rewriting that row would be a write, a persist
    // and a cover re-key for nothing. Both halves are consumed once, in the
    // order the shell answered in, which is what pairs a row with its result.
    let results = answer.results.into_iter();
    let moved: HashMap<String, (String, String)> = candidates
        .into_iter()
        .zip(results)
        .filter(|((_, from), result)| result.is_ok() && &result.store != from)
        .map(|((id, from), result)| (id, (from, result.store)))
        .collect();
    if moved.is_empty() {
        return;
    }

    // The address is the only thing that moves. The fingerprint, the provenance
    // and the resume point all describe bytes that have not changed, and a row
    // that arrived here with a pending measurement keeps it — the startup pass
    // measures the copy at its new address and finishes the job, exactly as it
    // would have at the old one.
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
/// The cover cache is keyed by the address a book reads, so a move orphans the
/// art under a key nothing asks about any more and leaves the card showing a
/// fallback until the book is opened again. A re-key rather than a re-render: the
/// image is the same page of the same bytes, and a shelf of forty migrated books
/// is forty renders nobody asked for.
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
