//! The recent-books shelf: recording the book that was just opened.

use leptos::prelude::*;

use crate::state::library::{self, RecentBook};
use crate::state::AppState;
use crate::storage::{save_covers, save_library};

/// Move this book to the front of the shelf and persist it.
///
/// An entry evicted past the cap has its cover dropped with it, so the cover
/// store cannot outgrow the list it belongs to.
pub(super) fn record(
    state: AppState,
    path: &str,
    title: Option<String>,
    page: u32,
    num_pages: u32,
) {
    // Persist last path (the settings-watch effect writes localStorage
    // automatically). Kept for schema stability; the library below is the
    // real "recent books" store.
    state
        .settings
        .update(|s| s.last_path = Some(path.to_string()));

    let mut recent = state.library.books.get_untracked();
    let evicted = library::upsert(
        &mut recent,
        RecentBook {
            path: path.to_string(),
            title,
            page,
            num_pages,
        },
    );
    state.library.books.set(recent);
    if let Err(e) = state.library.books.with_untracked(|books| save_library(books)) {
        e.report();
    }
    if let Some(evicted_path) = evicted {
        state.library.covers.update(|c| {
            c.remove(&evicted_path);
        });
        if let Err(e) = state.library.covers.with_untracked(save_covers) {
            e.report();
        }
    }
}
