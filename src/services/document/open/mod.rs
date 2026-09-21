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

use library_core::book::{ReadPoint, resume_point};
use crate::state::{AppState, Toast};

use super::session;

/// Wire OS-level file opening (double-click / "Open with" / default-app
/// launch) into the shared open flow. Called once from the app root.
///
/// Two paths, one handoff point:
///   * PULL — `take_pending_file` collects whatever the OS handed the
///     backend before the webview finished mounting (initial-launch argv on
///     Windows/Linux, the macos open-file event at launch). An event emitted
///     before mount would be lost, so the command is the source of truth.
///   * PUSH — the backend emits `document-open-file` while the app runs
///     (single-instance forward, LaunchServices). The listener just re-runs
///     the pull: the command clears itself, so an event plus a stray second
///     pull can never open the same file twice.
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

    // PUSH: the listener just re-runs the pull (the doc above says why).
    let cb_state = state;
    crate::services::tauri_listen("document-open-file", move |_ev: web_sys::Event| {
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
/// [`reader_app::state::document::DocumentState::book_id`]: the resume point
/// to seed ([`library_core::book::resume_point`]), the key the highlights live
/// under ([`crate::services::document::gloss_key`]) and the rows a progress
/// write belongs to ([`library_core::book::rows_for_read`]).
///
/// A row that went between the click and the open is no open at all: there is
/// no address to read, and an error toast for a book the library no longer has
/// would be a sentence about nothing.
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
    open_at(state, None, path);
}

/// The open itself, with the row the reader named when they named one. See
/// [`open_book`] for what the id buys and [`open_path`] for the opens that
/// have nothing but an address.
fn open_at(state: AppState, book_id: Option<String>, path: String) {
    // Claim the document state for THIS attempt. Pick a second book while the
    // first is still resolving and the loser's tail would otherwise still run:
    // writing the old book's page count, geometry and scale over the new one's
    // and flipping `status` to Ready a second time, resuming the winner at the
    // loser's page. Every hop below re-checks the stamp.
    let stamp = session::claim();
    state.reader.document.status.set(DocStatus::Opening);
    state.reader.document.error.set(None);
    // Named BEFORE the resume point is read and before any tail seeds the
    // gloss: which row this open belongs to is a fact about the attempt, not
    // something the tails discover later, and a write that ran first would be
    // a write against the book that was open before this one.
    //
    // An open that arrived as nothing but an address settles onto the row the
    // library already holds for it here, rather than waiting for the tail's
    // shelf record: the highlights are keyed by the row's id and are loaded
    // before any tail runs, so a key derived later would be a key the marks
    // were not stored under — and a drop of a file already on the shelf
    // should resume where the reader left it rather than reading as a book
    // the library has never seen.
    //
    // Only a SHARED row is settled onto. A book of its own is the reader's
    // private instance of the file, and an open that could not name a row has
    // not said it meant that one — the rule `add_book` and `rows_for_read`
    // already keep, so a drop never hijacks a private book's resume point or
    // its marks. A file with no row keeps `None` and joins the library when
    // the tail records the read, exactly as before.
    let book_id = book_id.or_else(|| {
        state.library.books.with_untracked(|books| {
            library_core::book::book_rows(books)
                .find(|b| b.path() == path && !b.independent)
                .map(|b| b.id.clone())
        })
    });
    state.reader.document.book_id.set(book_id.clone());
    // Re-arm the first-paint cover for THIS document: an open over a mounted
    // reader (drag-drop, "Open with") never passes through `close_document`'s
    // reset, and the gate belongs to the open, not the close.
    state.reader.viewer.first_paint.set(false);

    // The resume point is read BEFORE the open resolves so it can't be
    // clobbered by a concurrent page-tracking write from the closing document.
    // The reflowable tail also takes the fractional stream position, when the
    // last session left one. Which ROW answers is the id's business: a book of
    // its own resumes where its own reader left off, not where the twin at
    // the address did.
    let (saved_page, saved_fraction) = state.library.books.with_untracked(|books| {
        resume_point(books, book_id.as_deref(), &path)
    });

    match format_of(&path) {
        Format::Pdf => open_pdf(state, path, saved_page, stamp),
        fmt => reflow::open_reflowable(state, path, fmt, saved_page, saved_fraction, stamp),
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
    // builds the index instead (`reader_app::effects::search`), and a
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
