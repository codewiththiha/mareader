//! Opening a document: the dialog flow, the OS "Open with" handoff, the
//! library's own row, and the shared open sequence — one orchestration plus
//! a module per step ([`seed`], [`shelf`], [`outline`], [`cover`],
//! [`warmup`]). Every step after the engine's answer is guarded by the
//! session stamp ([`super::session`]): all of them can outlive the attempt
//! that started them.
//!
//! [`enter`] is the part the two pipelines share: the identity write, the
//! gloss marks, the resume clamp, the startup scale and the route flip. A
//! tail owns its own content seeding and calls [`enter`] for everything a
//! reader expects to behave the same whatever the file extension was.

mod cover;
mod enter;
mod outline;
mod seed;
mod shelf;
mod reflow;
mod warmup;

use leptos::prelude::*;
// The open flow spawns on the wasm-bindgen-futures executor, NOT
// `leptos::task::spawn_local`: the latter ties the future to the reactive
// owner it is spawned under, so an open started by a book-card click would be
// CANCELLED the moment `status` flips to `Opening` — unmounting the card and
// disposing its owner — leaving the app stuck on "Opening..." forever.
use wasm_bindgen_futures::spawn_local;

use reader_core::format::{Format, format_of};
use pdf_engine::api as engine;
use pdf_engine::types::DocStatus;

use crate::runtime::ReadPoint;
use crate::state::{AppState, Toast};

use super::session;

/// Native open-dialog flow: pick a file, then run the shared open-flow.
///
/// Cancel (the engine's own [`CANCELLED`](pdf_engine::api::dialog::CANCELLED) sentence) is a
/// silent no-op; any other error surfaces on the doc status / status bar.
pub fn open_dialog(state: AppState) {
    spawn_local(async move {
        match engine::pick_document().await {
            Ok(path) => open_path(state, path),
            Err(msg) if msg != pdf_engine::api::dialog::CANCELLED => fail(state, msg),
            Err(_) => {}
        }
    });
}

/// Open a library row, which is what every surface on the shelf calls: a
/// book opens; a link reveals what it points at instead
/// ([`crate::services::library::reveal_book`] for a book's link,
/// [`crate::services::library::reveal_shelf`] for a folder's) — a pointer is
/// not a file and there is nothing to open. The target's kind is its id's
/// first letter (`library_core::id::is_shelf`). A row that went between the
/// click and the open opens nothing.
#[cfg(feature = "library")]
pub fn open_row(state: AppState, row_id: String) {
    let target = state
        .library
        .row(&row_id)
        .and_then(|row| row.target().map(str::to_string));
    match target {
        Some(target) if library_core::id::is_shelf(&target) => {
            crate::services::library::reveal_shelf(state, &target)
        }
        Some(target) => crate::services::library::reveal_book(state, &target),
        None => open_book(state, row_id),
    }
}

/// Open a library book: the book's own address, and the row itself as the
/// session's identity.
///
/// What a shelf surface calls — a card, a list row, the context menu's Open —
/// and the only open that can say WHICH book the reader meant when the library
/// holds two rows of one file. Everything downstream reads the row from
/// [`crate::state::reader::document::DocumentState::book_id`]: the resume point
/// to seed ([`library_core::book::resume_point`]), the key the highlights live
/// under ([`crate::services::document::gloss_key`]) and the rows a progress
/// write belongs to ([`library_core::book::rows_for_read`]).
///
/// A row that went between the click and the open is no open at all: there is
/// no address to read, and an error toast for a book the library no longer has
/// would be a sentence about nothing.
#[cfg(feature = "library")]
pub fn open_book(state: AppState, book_id: String) {
    let Some(book) = state.library.books.with_untracked(|books| {
        library_core::book::find_by_id(books, &book_id).cloned()
    }) else {
        return;
    };
    // A row the library KNOWS is dead — a path check found its address gone —
    // does not open onto the reader's error screen. The honest answer to a
    // click on a missing book is the Find-again question
    // (`crate::services::library::ask_relink`), which keeps the row, its
    // shelf and its place in the book exactly as they are. Opening anyway
    // would be an error page the reader has to back out of — the
    // reload-and-try-again loop this gate exists to end.
    if book.missing {
        crate::services::library::ask_relink(state, book_id);
        return;
    }
    open_at(state, Some(book_id), book.path().to_string());
}

/// Shared open-flow: open `path` through the pipeline its format needs and
/// populate the whole app state (document, viewer, search, library). Resumes
/// at the saved page if this book was opened before, and records it in the
/// recent-books library. Drag-drop calls this directly.
///
/// The pipeline fork happens here and only here: PDFs go to the pdf.js
/// engine, the reflowable formats to the reflow pipeline ([`reflow`]). Both
/// tails converge on the same state contract, so everything downstream —
/// viewer, navigation, shelf — is format-agnostic.
pub fn open_path(state: AppState, path: String) {
    if crate::runtime::is_reader() {
        crate::runtime::emit(serde_json::json!({"type":"open-path", "path":path}));
        return;
    }
    #[cfg(feature = "library")]
    open_at(state, None, path);
    #[cfg(not(feature = "library"))]
    let _ = state;
}

/// The open itself, with the row the reader named when they named one. See
/// [`open_book`] for what the id buys and [`open_path`] for the opens that
/// have nothing but an address.
#[cfg(feature = "library")]
fn open_at(state: AppState, book_id: Option<String>, path: String) {
    crate::runtime::library::request_open(state, book_id, path);
}

/// Called only by this runtime's validated OPEN command.
pub fn open_selected(state: AppState, path: String, saved_page: u32, saved_fraction: Option<f64>) {
    let stamp = session::claim();
    state.reader.document.status.set(DocStatus::Opening);
    state.reader.document.error.set(None);
    state.reader.viewer.first_paint.set(false);
    match format_of(&path) {
        Format::Pdf if cfg!(feature = "pdf") => open_pdf(state, path, saved_page, stamp),
        Format::Text if cfg!(feature = "txt") => reflow::open_reflowable(state, path, Format::Text, saved_page, saved_fraction, stamp),
        Format::Markdown if cfg!(feature = "md") => reflow::open_reflowable(state, path, Format::Markdown, saved_page, saved_fraction, stamp),
        _ => fail(state, "This reader does not support the requested format".to_string()),
    }
}

/// The PDF tail of the open flow: hand the path to the engine and seed from
/// its answer.
fn open_pdf(state: AppState, path: String, saved_page: u32, stamp: u64) {
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
    // mount reads (page, heights, scale) is in its new-document state. The
    // resume page is one of them: the strip scrolls to `viewer.page` as it
    // binds its container (`ScrollShell`), so there is no second jump to
    // schedule here.
    enter::enter_ready(state);

    // The engine's own highlight layer belongs to the previous book; the
    // search reset inside `enter_ready` is the app's half of the same cleanup.
    engine::clear_highlights();

    outline::resolve(state, path.clone(), stamp);

    // No eager search-index build here, deliberately: extraction costs one
    // worker round trip per page and the index it fills lives on the wasm
    // heap, which never shrinks — an open-time build charged every book that
    // ratchet whether or not anyone ever searched it. The first search
    // builds the index instead (`crate::effects::reader::search`), and a
    // reopen of the same bytes adopts the retained one.

    shelf::record(
        state,
        &path,
        seeded.name,
        // A PDF's resume point is a page and nothing else: there is no stream
        // position to carry, so the fraction stays None rather than inheriting
        // whatever a reflowable book last left in this slot.
        ReadPoint {
            page: seeded.resume,
            num_pages: seeded.num_pages,
            fraction: None,
        },
    );
    cover::ensure(state, path, stamp);
    warmup::prewarm_thumbs(seeded.num_pages);
    // The heap probe's baseline: what the book cost to open, before any
    // reading moves it. The close line is the number to compare this one
    // against — the difference is the session's ratchet.
    crate::memory::log_heap("open");
}

/// The document did not open: surface it on the status bar and as a toast.
fn fail(state: AppState, message: String) {
    crate::runtime::emit(serde_json::json!({"type":"error", "message":message}));
    state.reader.document.error.set(Some(message.clone()));
    state.reader.document.status.set(DocStatus::Error);
    state
        .ui
        .toast
        .set(Some(Toast::new(format!(
            "Could not open document: {}",
            message
        ))));
}
