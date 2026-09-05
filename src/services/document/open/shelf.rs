//! The recent-books shelf: recording the book that was just opened.

use leptos::prelude::*;

use crate::state::library::{RecentBook, self};
use crate::state::AppState;

/// Move this book to the front of the shelf and persist it.
///
/// An entry evicted past the cap has its cover dropped with it, so the cover
/// store cannot outgrow the list it belongs to.
pub(crate) fn record(
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
    // A reflowable book's fractional stream position survives the re-open's
    // upsert: the entry below replaces this one in the same breath, and the
    // open flow just consumed this fraction to place the stream. The first
    // scroll of the new session overwrites it with the truth of this read.
    let fraction = recent.iter().find(|b| b.path == path).and_then(|b| b.fraction);
    let evicted = library::upsert(
        &mut recent,
        RecentBook {
            path: path.to_string(),
            title,
            page,
            num_pages,
            fraction,
        },
    );
    state.library.books.set(recent);
    crate::storage::persist_library(state.library);
    if let Some(evicted_path) = evicted {
        state.library.covers.update(|c| {
            c.remove(&evicted_path);
        });
        crate::storage::persist_covers(state.library);
    }
}
