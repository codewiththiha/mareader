//! Opening a document: the dialog, the OS handoff, and the shared
//! sequence.

#[cfg(feature = "pdf")]
use runtime_contract::boundary::ShellApi;

#[cfg(feature = "pdf")]
mod cover;
#[cfg(any(feature = "pdf", feature = "reflow"))]
mod enter;
#[cfg(feature = "pdf")]
mod outline;
#[cfg(feature = "reflow")]
mod reflow;
#[cfg(feature = "pdf")]
mod seed;
#[cfg(feature = "pdf")]
mod warmup;

use leptos::prelude::*;
// Spawned on wasm-bindgen-futures: `leptos::task` cancels with its
// owner and strands "Opening...".
use wasm_bindgen_futures::spawn_local;

use reader_core::document::DocStatus;
#[cfg(feature = "pdf")]
use reader_core::format::Format;
use reader_core::format::format_of;

use app_state::state::Toast;

use crate::host::contract::{OpenRequest, Placement};
#[cfg(feature = "pdf")]
use crate::pane::session::FormatSession;

/// Wire OS file opening into the shared flow: pull first, then a
/// push.
pub fn init_open_file_handling(ctx: crate::context::ReaderContext) {
    // PULL: whatever the OS handed the backend before this session mounted.
    if !tauri_bridge::has_tauri() {
        return;
    }
    let st = ctx;
    spawn_local(async move {
        if let Some(path) = tauri_bridge::take_pending_file().await {
            open_path(st, path, Placement::Here);
        }
    });
}

/// Pick a file, then run the shared flow. Cancel is a silent no-op.
pub fn open_dialog(ctx: crate::context::ReaderContext, placement: Placement) {
    spawn_local(async move {
        match app_chrome::dialog::pick_document().await {
            Ok(path) => open_path(ctx, path, placement),
            Err(msg) if msg != app_chrome::dialog::CANCELLED => fail(ctx, msg),
            Err(_) => {}
        }
    });
}

/// Open `path` through its format's pipeline, then resume at the
/// saved page.
pub fn open_path(ctx: crate::context::ReaderContext, path: String, placement: Placement) {
    // In-session: the boundary answers the library's questions; the
    // session holds no state.
    if ctx.api == crate::context::ApiHandle::Frame {
        crate::frame::open_path_in_frame(ctx, path, placement);
        return;
    }
    // A pane frame holds no library: the host resolves and places.
    if ctx.api == crate::context::ApiHandle::Pane {
        crate::pane_frame::open_path(path, placement);
        return;
    }
    // Only the unhosted entry reads its store locally.
    let launch = storage::resolve_launch(&path).unwrap_or_else(|| bare_launch(&path));
    // The HOST places the open, never this pane on its own.
    ctx.open.try_run(OpenRequest { launch, placement });
}

/// A descriptor for a path the store cannot answer for.
pub fn bare_launch(path: &str) -> runtime_contract::boundary::LaunchDocument {
    runtime_contract::boundary::LaunchDocument {
        path: path.to_string(),
        ..crate::pane::base::empty_launch()
    }
}

/// The open itself, with the launch naming the row and resume point.
pub(crate) fn open_with_launch(
    ctx: crate::context::ReaderContext,
    launch: runtime_contract::boundary::LaunchDocument,
) {
    let path = launch.path.clone();
    // WORK gate: refused once the pane or session is Disposing.
    if !ctx.pane.admits_work() {
        return;
    }
    // Claim the pane's document state for THIS attempt; every hop below
    // re-checks the generation.
    let stamp = ctx.pane.claim_generation();
    crate::diagnostics::note_reader_runtime_create();
    ctx.reader.document.status.set(DocStatus::Opening);
    ctx.reader.document.error.set(None);
    ctx.reader.document.book_id.set(launch.book_id.clone());
    ctx.reader.viewer.first_paint.set(false);
    let saved_page = launch.resume_page;
    #[cfg(not(any(feature = "pdf", feature = "reflow")))]
    let _ = (stamp, saved_page);
    #[cfg(feature = "reflow")]
    let saved_fraction = launch.saved_fraction;
    ctx.launch.set(launch);
    // The trail: one line in, one line out if the open fails.
    web_sys::console::log_1(&wasm_bindgen::JsValue::from_str(&format!(
        "[reader] open {path}"
    )));

    match format_of(&path) {
        #[cfg(feature = "pdf")]
        Format::Pdf => open_pdf(ctx, path, saved_page, stamp),
        #[cfg(feature = "reflow")]
        fmt if fmt.is_reflowable() => {
            reflow::open_reflowable(ctx, path, fmt, saved_page, saved_fraction, stamp)
        }
        _ => fail(
            ctx,
            "This document belongs to a different pane runtime".to_string(),
        ),
    }
}

/// The PDF tail: a fresh engine session as the pane's document owner.
#[cfg(feature = "pdf")]
fn open_pdf(ctx: crate::context::ReaderContext, path: String, saved_page: u32, stamp: u64) {
    if !ctx.pane.admits_work() {
        return;
    }
    // One document, one session, through the pane's one replace path.
    let release = ctx
        .pane
        .replace_document(FormatSession::Pdf(pdf_engine::PdfSession::create()));
    let pdf = ctx.pane.pdf();
    spawn_local(async move {
        // The replaced document is released before this one loads, so the pane
        // never holds two.
        release.settled().await;
        // Superseded: whoever took the pane over disposed this session.
        if !ctx.pane.owns_generation(stamp) {
            return;
        }
        let opened = pdf.open(&path).await;
        // The engine answered; a second open may have won meanwhile, so
        // stand down.
        if !ctx.pane.owns_generation(stamp) {
            return;
        }
        match opened {
            Ok(open) => ready(ctx, path, open, saved_page, stamp),
            Err(e) => abandon(ctx, e.message),
        }
    });
}

/// The book opened: seed the ctx, flip the route, and start the tails.
#[cfg(feature = "pdf")]
fn ready(
    ctx: crate::context::ReaderContext,
    path: String,
    open: pdf_engine::types::OpenResult,
    saved_page: u32,
    stamp: u64,
) {
    let seeded = seed::seed(&ctx, &path, open, saved_page);

    // The book is ready; the route already points here.
    enter::enter_ready(ctx);

    // No highlight clear is needed: the old layer went with its session.

    outline::resolve(ctx, path.clone(), stamp);

    // No eager search index: the heap never shrinks; the first search
    // builds it.

    // The read record rides the boundary; the Shell writes the rows.
    ctx.api.read_point(&runtime_contract::boundary::ReadPoint {
        book_id: ctx.reader.document.book_id.get_untracked(),
        path: path.clone(),
        page: seeded.resume,
        num_pages: seeded.num_pages,
        // A PDF resumes by page; the fraction stays None.
        fraction: None,
        title: None,
        author: None,
    });
    cover::ensure(&ctx, path, stamp);
    warmup::prewarm_thumbs(&ctx, seeded.num_pages, stamp);
    // The heap baseline for this open, to compare the close against.
    app_state::memory::log_heap("open");
}

/// The open failed: surface it; the pane holds no document after.
#[cfg(any(feature = "pdf", feature = "reflow"))]
pub(super) fn abandon(ctx: crate::context::ReaderContext, message: String) {
    ctx.pane.abandon_document().detach();
    fail(ctx, message);
}

fn fail(ctx: crate::context::ReaderContext, message: String) {
    web_sys::console::error_1(&wasm_bindgen::JsValue::from_str(&format!(
        "[reader] open failed: {message}"
    )));
    ctx.reader.document.error.set(Some(message.clone()));
    ctx.reader.document.status.set(DocStatus::Error);
    ctx.ui.toast.set(Some(Toast::new(format!(
        "Could not open document: {}",
        message
    ))));
}
