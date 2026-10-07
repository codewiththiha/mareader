//! The shelf cover: page 1 of the book, as a small JPEG.

use leptos::prelude::*;
use runtime_contract::boundary::ShellApi;
use wasm_bindgen_futures::spawn_local;

use runtime_contract::covers::COVER_WIDTH;

/// Render this book's cover for the Shell unless the launch carried one.
pub(super) fn ensure(ctx: &crate::context::ReaderContext, path: String, stamp: u64) {
    if ctx.launch.with(|l| l.cover_data_url.is_some()) {
        return;
    }
    let ctx = *ctx;
    // Rendered by the pane's OWN session, for its own document.
    let pdf = ctx.pane.pdf();
    spawn_local(async move {
        let cover = pdf.cover_data_url(&path, COVER_WIDTH).await;
        // A cover for a superseded attempt belongs to a document no longer up.
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
