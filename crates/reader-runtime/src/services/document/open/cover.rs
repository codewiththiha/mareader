//! The shelf cover: page 1 of the book, as a small JPEG.

use leptos::prelude::*;
use runtime_contract::boundary::ShellApi;
use wasm_bindgen_futures::spawn_local;

use runtime_contract::covers::COVER_WIDTH;

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
    // Rendered by the pane's OWN session, for its own document.
    let pdf = ctx.pane.pdf();
    spawn_local(async move {
        let cover = pdf.cover_data_url(&path, COVER_WIDTH).await;
        // A cover finished for a superseded attempt belongs to a document
        // the pane no longer shows; filing it now could race the winner's.
        if !ctx.pane.owns_generation(stamp) {
            return;
        }
        let Ok(c) = cover else {
            // Stylised fallback cover; nothing to store.
            return;
        };
        ctx.api.save_cover(
            &path,
            &runtime_contract::covers::CoverImage {
                data_url: c.data_url,
                width: c.width,
                height: c.height,
            },
        );
    });
}
