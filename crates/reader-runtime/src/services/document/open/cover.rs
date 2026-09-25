//! The shelf cover: page 1 of the book, as a small JPEG.

use app_state::boundary::ShellApi;
use leptos::prelude::*;
use wasm_bindgen_futures::spawn_local;

use pdf_engine::api as engine;

use crate::services::document::session;
use app_state::state::covers::COVER_WIDTH;

/// Render and hand this book's cover to the Shell, unless the launch already
/// carried one (the library had it). Regenerating on every open re-rendered
/// page 1 through the worker — against the reader's own first paint — so the
/// launch descriptor's answer is the "already have it" gate. A failed render
/// just leaves the stylised fallback cover on the shelf.
pub(super) fn ensure(ctx: &crate::context::ReaderContext, path: String, stamp: u64) {
    if ctx.launch.with(|l| l.cover_data_url.is_some()) {
        return;
    }
    let ctx = *ctx;
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
        ctx.api.save_cover(
            &path,
            &app_state::state::covers::CoverImage {
                data_url: c.data_url,
                width: c.width,
                height: c.height,
            },
        );
    });
}
