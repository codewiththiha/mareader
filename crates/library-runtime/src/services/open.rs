//! Opening from the shelf: the row a surface named becomes a serialized
//! launch descriptor across the boundary. The library computes what the
//! reader needs (the row identity, the resume point, the name, the cover)
//! from its own state — then the Shell owns the transition.

use leptos::prelude::WithUntracked;

use crate::context::LibraryContext;
use app_state::boundary::LaunchDocument;
use app_state::boundary::ShellApi;
use library_core::book::resume_point;

/// Open a library row — what every shelf surface calls (a card, a list row,
/// the context menu's Open). A shelf target reveals; a book (or an unknown
/// row id — a link's destination) becomes the reader launch.
pub fn open_row(ctx: &LibraryContext, row_id: String) {
    let target = ctx
        .library
        .row(&row_id)
        .and_then(|row| row.target().map(str::to_string));
    match target {
        Some(target) if library_core::id::is_shelf(&target) => {
            crate::services::reveal_shelf(*ctx, &target)
        }
        Some(target) => crate::services::reveal_book(*ctx, &target),
        None => open_book(ctx, row_id),
    }
}

/// Open a library book: the book's own address, and the row itself as the
/// launch identity. A row the library KNOWS is dead asks the find-again
/// question instead of opening onto an error screen.
pub fn open_book(ctx: &LibraryContext, book_id: String) {
    let Some(book) = ctx
        .library
        .books
        .with_untracked(|books| library_core::book::find_by_id(books, &book_id).cloned())
    else {
        return;
    };
    if book.missing {
        crate::services::ask_relink(crate::context::LibraryContext::clone(ctx), book_id);
        return;
    }
    open_at(ctx, Some(book_id), book.path().to_string());
}

/// Open an address with no row (a link's target): the open settles onto the
/// SHARED row the library already holds for the path, per the same rule the
/// unified flow kept — a private twin never hijacks a drop.
pub fn open_path(ctx: &LibraryContext, path: String) {
    open_at(ctx, None, path);
}

fn open_at(ctx: &LibraryContext, book_id: Option<String>, path: String) {
    let book_id = book_id.or_else(|| {
        ctx.library.books.with_untracked(|books| {
            library_core::book::book_rows(books)
                .find(|b| b.path() == path && !b.independent)
                .map(|b| b.id.clone())
        })
    });
    let (resume_page, saved_fraction) = ctx
        .library
        .books
        .with_untracked(|books| resume_point(books, book_id.as_deref(), &path));
    let display_name = book_id
        .as_deref()
        .and_then(|id| {
            ctx.library
                .books
                .with_untracked(|books| library_core::book::find_by_id(books, id).cloned())
        })
        .map(|b| b.title());
    let cover_data_url = book_id
        .as_deref()
        .and_then(|id| {
            ctx.library
                .books
                .with_untracked(|books| library_core::book::find_by_id(books, id).cloned())
        })
        .and_then(|row| {
            ctx.library
                .covers
                .with_untracked(|covers| covers.get(row.path()).cloned())
        })
        .map(|c| c.data_url.clone());
    ctx.api.open_document(&LaunchDocument {
        book_id,
        path,
        resume_page,
        saved_fraction,
        blend_override: false,
        cover_data_url,
        display_name,
    });
}
