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

use app_state::boundary::ShellApi;

mod cover;
mod enter;
mod outline;
mod reflow;
mod seed;
mod warmup;

use leptos::prelude::*;
// The open flow spawns on the wasm-bindgen-futures executor, NOT
// `leptos::task::spawn_local`: the latter ties the future to the reactive
// owner it is spawned under, so an open started by a book-card click would be
// CANCELLED the moment `status` flips to `Opening` — unmounting the card and
// disposing its owner — leaving the app stuck on "Opening..." forever.
use wasm_bindgen_futures::spawn_local;

use pdf_engine::api as engine;
use pdf_engine::types::DocStatus;
use reader_core::format::{Format, format_of};

use app_state::state::Toast;

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
///
/// The reader-side half of an OS file open: the Shell catches the OS event
/// (its services own the Tauri listeners) and forwards the path into the
/// LIVE session through its command export. The session decides nothing
/// about where the event came from — it opens the path it was handed.
pub fn init_open_file_handling(ctx: crate::context::ReaderContext) {
    // PULL: whatever the OS handed the backend before this session mounted
    // (initial-launch argv). One pull per session; the command clears itself.
    if !tauri_bridge::has_tauri() {
        return;
    }
    let st = ctx;
    spawn_local(async move {
        if let Some(path) = engine::take_pending_file().await {
            open_path(st, path);
        }
    });
}

/// Native open-dialog flow: pick a file, then run the shared open-flow.
///
/// Cancel (the engine's own [`CANCELLED`](pdf_engine::api::dialog::CANCELLED) sentence) is a
/// silent no-op; any other error surfaces on the doc status / status bar.
pub fn open_dialog(ctx: crate::context::ReaderContext) {
    spawn_local(async move {
        match engine::pick_document().await {
            Ok(path) => open_path(ctx, path),
            Err(msg) if msg != pdf_engine::api::dialog::CANCELLED => fail(ctx, msg),
            Err(_) => {}
        }
    });
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
pub fn open_path(ctx: crate::context::ReaderContext, path: String) {
    // An in-session open (drop, dialog): the row identity, resume point,
    // display name and cover are answered by the boundary against the
    // persisted library — the session itself holds no library state.
    let launch = ctx
        .api
        .resolve_launch(&path)
        .unwrap_or_else(|| bare_launch(&path));
    open_with_launch(ctx, launch);
}

/// A descriptor for a path nothing in the store answers for: the open
/// proceeds unnamed, and the read record mints the row.
pub fn bare_launch(path: &str) -> app_state::boundary::LaunchDocument {
    app_state::boundary::LaunchDocument {
        book_id: None,
        path: path.to_string(),
        resume_page: 1,
        saved_fraction: None,
        blend_override: false,
        cover_data_url: None,
        display_name: None,
    }
}

/// The open itself, with the launch descriptor naming the row (when the
/// library named one) and where to resume. The session mount already began
/// this runtime; an in-session open reuses the live slot.
pub(crate) fn open_with_launch(
    ctx: crate::context::ReaderContext,
    launch: app_state::boundary::LaunchDocument,
) {
    let path = launch.path.clone();
    // WORK gate: refused once the runtime entered Disposing (a session end
    // cancels a still-queued open before it reaches the engine).
    if !ctx.runtime.lifecycle().admits_work() {
        return;
    }
    // Claim the document state for THIS attempt. Every hop below re-checks
    // the stamp, so a second open's tail cannot write over the winner's.
    let stamp = session::claim();
    crate::diagnostics::note_reader_runtime_create();
    ctx.reader.document.status.set(DocStatus::Opening);
    ctx.reader.document.error.set(None);
    ctx.reader.document.book_id.set(launch.book_id.clone());
    ctx.reader.viewer.first_paint.set(false);
    let saved_page = launch.resume_page;
    let saved_fraction = launch.saved_fraction;
    ctx.launch.set(launch);

    match format_of(&path) {
        Format::Pdf => open_pdf(ctx, path, saved_page, stamp),
        fmt => reflow::open_reflowable(ctx, path, fmt, saved_page, saved_fraction, stamp),
    }
}

/// The PDF tail of the open flow: hand the path to the engine and seed from
/// its answer.
fn open_pdf(ctx: crate::context::ReaderContext, path: String, saved_page: u32, stamp: u64) {
    let pdf = ctx.runtime.pdf();
    if !pdf.work_admitted() {
        return;
    }
    spawn_local(async move {
        let opened = pdf.open(&path).await;
        // The engine answered — but a second open (or a close) may have taken
        // the document state over while it was working. Standing down here is
        // what keeps the winner's `Ready` from being followed by the loser's.
        if !session::owns(stamp) {
            return;
        }
        match opened {
            Ok(open) => ready(ctx, path, open, saved_page, stamp),
            Err(e) => fail(ctx, e.message),
        }
    });
}

/// The book opened: seed the ctx, flip the route, and start the tails.
fn ready(
    ctx: crate::context::ReaderContext,
    path: String,
    open: pdf_engine::types::OpenResult,
    saved_page: u32,
    stamp: u64,
) {
    let seeded = seed::seed(&ctx, &path, open, saved_page);

    // The book is ready. The ROUTE is the Shell's business and already
    // points here (the manager navigated when it started this session); the
    // status flip the reader owns happens inside `enter_ready`.
    enter::enter_ready(ctx);

    // The engine's own highlight layer belongs to the previous book; the
    // search reset inside `enter_ready` is the app's half of the same cleanup.
    engine::clear_highlights();

    outline::resolve(ctx, path.clone(), stamp);

    // No eager search-index build here, deliberately: extraction costs one
    // worker round trip per page and the index it fills lives on the wasm
    // heap, which never shrinks — an open-time build charged every book that
    // ratchet whether or not anyone ever searched it. The first search
    // builds the index instead (`crate::effects::reader::search`), and a
    // reopen of the same bytes adopts the retained one.

    // The read record rides the boundary: the Shell's recorder writes the
    // rows (and mints the row for a file the library never knew) — the
    // session itself holds no library ctx.
    ctx.api.read_point(&app_state::boundary::ReadPoint {
        book_id: ctx.reader.document.book_id.get_untracked(),
        path: path.clone(),
        page: seeded.resume,
        num_pages: seeded.num_pages,
        // A PDF's resume point is a page and nothing else: there is no stream
        // position to carry, so the fraction stays None rather than inheriting
        // whatever a reflowable book last left in this slot.
        fraction: None,
        title: None,
        author: None,
    });
    cover::ensure(&ctx, path, stamp);
    warmup::prewarm_thumbs(seeded.num_pages, stamp);
    // The heap probe's baseline: what the book cost to open, before any
    // reading moves it. The close line is the number to compare this one
    // against — the difference is the session's ratchet.
    app_state::memory::log_heap("open");
}

/// The document did not open: surface it on the status bar and as a toast.
fn fail(ctx: crate::context::ReaderContext, message: String) {
    ctx.reader.document.error.set(Some(message.clone()));
    ctx.reader.document.status.set(DocStatus::Error);
    ctx.ui.toast.set(Some(Toast::new(format!(
        "Could not open document: {}",
        message
    ))));
}
