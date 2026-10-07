//! Writing the open book's resume point into the library now.

use leptos::prelude::*;

use reader_core::document::DocStatus;
use reader_core::view::ViewMode;

use runtime_contract::boundary::ShellApi;

/// Carry the open book's position into the library and save it.
pub(crate) fn flush_read_point(ctx: &crate::context::ReaderContext) {
    if ctx.reader.document.status.get_untracked() != DocStatus::Ready {
        return;
    }
    let Some(path) = ctx.reader.document.path.get_untracked() else {
        return;
    };
    // Clamped to the open book, like an open's resume point.
    let num_pages = ctx.reader.document.num_pages.get_untracked();
    let page = ctx
        .reader
        .viewer
        .page
        .get_untracked()
        .clamp(1, num_pages.max(1));
    // The stream's fraction rides along with the page.
    let streaming = ctx.reader.reflowable_now()
        && ctx.reader.viewer.mode.get_untracked() == ViewMode::ScrollVertical;
    let fraction = if streaming {
        ctx.reader.stream_fraction()
    } else {
        None
    };
    // The rows this read belongs to, by the rows_for_read rule.
    ctx.api.read_point(&runtime_contract::boundary::ReadPoint {
        book_id: ctx.reader.document.book_id.get_untracked(),
        path,
        page,
        num_pages,
        fraction,
        title: ctx.reader.document.title.get_untracked(),
        author: ctx.reader.document.author.get_untracked(),
    });
}
