//! The shelf cover: page 1 of the book, as a small JPEG.

use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use pdf_engine::api as engine;

use crate::services::document::session;
// The width is the cover queue's: one number for both renders of the same art,
// so the cache the open files into is the cache the queue filled.
use crate::services::library::covers::COVER_WIDTH;
use crate::state::AppState;

/// Render and store this book's cover, unless the shelf already has one.
/// Regenerating on every open re-rendered page 1 through the worker — against
/// the reader's own first paint — and re-encoded and re-saved the whole cover
/// store on the main thread, right when the reader was fighting for both. A
/// failed render just leaves the stylised fallback cover on the shelf.
pub(super) fn ensure(state: AppState, path: String, stamp: u64) {
    if state
        .library
        .covers
        .with_untracked(|covers| covers.contains_key(&path))
    {
        return;
    }
    spawn_local(async move {
        let cover = engine::cover_data_url(&path, COVER_WIDTH).await;
        // A cover rendered by a superseded attempt is page 1 of whatever the
        // engine has open NOW, not of the book it was asked for; filing it
        // under `path` would put the wrong art on the shelf.
        if !session::owns(stamp) {
            return;
        }
        let Ok(c) = cover else {
            // Stylised fallback cover; nothing to store.
            return;
        };
        // Filed through the same door the import queue uses, so the cache's
        // quota cap is enforced by whoever crosses it rather than by whoever
        // happens to prune next.
        crate::services::library::covers::file_cover(
            state,
            path,
            c.data_url,
            c.width,
            c.height,
        );
        if let Err(e) = state
            .library
            .covers
            .with_untracked(crate::storage::save_covers)
        {
            e.report();
        }
    });
}
