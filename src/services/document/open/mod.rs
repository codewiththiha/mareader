//! Opening a document: the dialog flow, the OS "Open with" handoff, and the
//! shared open sequence.
//!
//! The sequence is one orchestration here plus a module per step —
//! [`seed`] fills the app state, [`shelf`] records the book, [`outline`]
//! resolves the chapter tree, [`cover`] renders the shelf art and [`warmup`]
//! primes the thumbnail cache. Every step after the engine's answer is
//! guarded by the session stamp (see [`super::session`]), because all of them
//! can outlive the attempt that started them.

mod cover;
mod outline;
mod seed;
mod shelf;
mod warmup;

use leptos::prelude::*;
// NOTE: the open flow spawns on the wasm-bindgen-futures executor, NOT
// `leptos::task::spawn_local`. The latter ties the future to the reactive
// owner it is spawned under, so an open initiated from a book-card click would
// be CANCELLED the moment `status` flips to `Opening` (that unmounts the card,
// disposing its owner) — leaving the app stuck on "Opening…" forever. The
// wasm-bindgen executor runs the future to completion regardless of owner.
use wasm_bindgen_futures::spawn_local;

use pdf_engine::api as engine;
use pdf_engine::types::DocStatus;

use crate::state::library;
use crate::state::{AppState, Toast};

use super::session;

/// Wire OS-level file opening (double-click / "Open with" / default-app
/// launch) into the shared open flow. Called once from the app root.
///
/// Two paths, one handoff point:
///   * PULL — `take_pending_file` collects whatever the OS handed to the
///     backend before the webview finished mounting (initial-launch argv on
///     Windows/Linux, the macOS open-file event at launch). An event emitted
///     before mount would be lost, so the command is the source of truth.
///   * PUSH — the backend emits `pdf-open-file` for files opened while the
///     app is already running (single-instance forward on Windows/Linux,
///     LaunchServices on macOS). The listener just re-runs the pull: the
///     command clears itself, so an event + a stray second pull can never
///     open the same file twice.
pub fn init_open_file_handling(state: AppState) {
    let st = state;
    spawn_local(async move {
        if let Some(path) = engine::take_pending_file().await {
            open_path(st, path);
        }
    });

    if !tauri_bridge::has_tauri() {
        return;
    }

    // PUSH — the listener just re-runs the pull (the command clears itself,
    // so an event + a stray second pull can never open the same file twice).
    let cb_state = state;
    crate::services::tauri_listen("pdf-open-file", move |_ev: web_sys::Event| {
        let st = cb_state;
        spawn_local(async move {
            if let Some(path) = engine::take_pending_file().await {
                open_path(st, path);
            }
        });
    });
}

/// Native open-dialog flow: pick a file, then run the shared open-flow.
///
/// Cancel ("Open cancelled") is a silent no-op; any other error surfaces on the
/// doc status / status bar.
pub fn open_dialog(state: AppState) {
    spawn_local(async move {
        match engine::pick_pdf().await {
            Ok(path) => open_path(state, path),
            Err(msg) => {
                if msg != "Open cancelled" {
                    state.reader.document.error.set(Some(msg.clone()));
                    state.reader.document.status.set(DocStatus::Error);
                    state.ui.toast.set(Some(Toast::new(format!("Could not open PDF: {}", msg))));
                }
            }
        }
    });
}
/// Shared open-flow: open `path` in the engine and populate the whole app state
/// (document, viewer, search, library). Resumes at the saved page if this book
/// was opened before, and records it in the recent-books library. Drag-drop
/// calls this directly.
pub fn open_path(state: AppState, path: String) {
    // Claim the document state for THIS attempt. Pick a second book while the
    // first is still resolving and the loser's tail would otherwise still run:
    // it would write the old book's page count, geometry and scale over the
    // new one's and flip `status` to Ready a second time, resuming the winner
    // at the loser's page. Every hop below re-checks the stamp.
    let stamp = session::claim();
    state.reader.document.status.set(DocStatus::Opening);
    state.reader.document.error.set(None);

    // The resume point is read BEFORE the open resolves so it can't be
    // clobbered by a concurrent page-tracking write from the closing document.
    let saved_page = state
        .library
        .books
        .with_untracked(|books| library::find_page(books, &path))
        .unwrap_or(1);

    spawn_local(async move {
        let opened = engine::open(&path).await;
        // The engine answered — but a second open (or a close) may have taken
        // the document state over while it was working. Standing down here is
        // what keeps the winner's `Ready` from being followed by the loser's.
        if !session::owns(stamp) {
            return;
        }
        match opened {
            Ok(open) => ready(state, path, open, saved_page, stamp),
            Err(e) => fail(state, e.message),
        }
    });
}

/// The book opened: seed the state, flip the route, and start the tails.
fn ready(
    state: AppState,
    path: String,
    open: pdf_engine::types::OpenResult,
    saved_page: u32,
    stamp: u64,
) {
    let seeded = seed::seed(state, &path, open, saved_page);

    // The book is ready: flip the route LAST, after every signal the fresh
    // mount reads (page, heights, scale) is already in its new-document
    // state. The resume page is one of them: the strip scrolls to
    // `viewer.page` as it binds its container (`ScrollShell`), so there is no
    // second jump to schedule here.
    state.reader.document.error.set(None);
    state.reader.document.status.set(DocStatus::Ready);
    // A successful open dismisses any stale error toast.
    state.ui.toast.set(None);

    // Reset search + clear stale highlights. The floating search overlay must
    // not linger after opening a new document.
    state.reader.search.reset();
    engine::clear_highlights();

    outline::resolve(state, path.clone(), stamp);

    // Fire index build in the background; result is ignored (search effects
    // call it too when needed). The page count is read up front: search's own
    // index uses it to know how many pages to ask the engine for.
    let search_pages = seeded.num_pages;
    spawn_local(async move {
        _ = engine::build_search_index(search_pages).await;
    });

    shelf::record(state, &path, seeded.name, seeded.resume, seeded.num_pages);
    cover::ensure(state, path, stamp);
    warmup::prewarm_thumbs(seeded.num_pages);
}

/// The book did not open: surface it on the status bar and as a toast.
fn fail(state: AppState, message: String) {
    state.reader.document.error.set(Some(message.clone()));
    state.reader.document.status.set(DocStatus::Error);
    state
        .ui
        .toast
        .set(Some(Toast::new(format!("Could not open PDF: {}", message))));
}
