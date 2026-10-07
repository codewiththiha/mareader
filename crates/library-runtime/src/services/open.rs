//! Opening from the shelf: the row becomes a launch descriptor; the Shell
//! owns the transition.

use leptos::prelude::WithUntracked;

use crate::context::LibraryContext;
use library_core::book::resume_point;
use runtime_contract::boundary::LaunchDocument;
use runtime_contract::boundary::ShellApi;

/// What every shelf surface calls: a shelf target reveals, a book opens.
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

/// The book's own address, with the row as identity; a dead row asks
/// find-again.
fn open_book(ctx: &LibraryContext, book_id: String) {
    let Some(book) = ctx
        .library
        .books
        .with_untracked(|books| library_core::book::find_by_id(books, &book_id).cloned())
    else {
        return;
    };
    if book.missing {
        crate::services::ask_relink(*ctx, book_id);
        return;
    }
    open_at(ctx, Some(book_id), book.path().to_string());
}

/// No row: the open settles onto the shared row for the path.
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
    let row = book_id.as_deref().and_then(|id| {
        ctx.library
            .books
            .with_untracked(|books| library_core::book::find_by_id(books, id).cloned())
    });
    let display_name = row.as_ref().map(|b| b.title());
    let cover_data_url = row
        .as_ref()
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
