//! The disposable Reader workspace-host iframe. The URL authenticates the
//! Shell's channel offer; the host's own WASM owns workspace chrome and
//! independent PDF/reflow child realms. Removing this iframe retires them all.
//!
//! The boot every artifact frame shares — the URL marker, the channel offer,
//! the boundary, the runtime root, the paint report — is
//! [`frame_transport::artifact`]. Two reader-only shapes sit on top:
//!
//! - The launch descriptor rides the Shell's `init`: the frame cannot mount
//!   before it arrives (a reader IS the document it was opened with), so the
//!   session starts inside the init handler, not at adoption.
//! - In-session path opens await one port-owned launch future. Disposal
//!   cancels/wakes it; there is no synchronous miss or duplicate open map.

use frame_transport::wasm::PortWire;
use leptos::prelude::Callable;
use runtime_contract::boundary::LaunchDocument;
use runtime_contract::protocol::{BootStage, RuntimeFrame, RuntimeKind, ShellFrame};

use crate::context::{ApiHandle, ReaderContext};
use crate::host::contract::{OpenRequest, Placement};

pub use frame_transport::artifact::{
    emit, mount_root, report_painted, session, set_session, take_session, with_api,
};

/// Boot through the frame when this artifact's URL names one. `true` as soon
/// as the marker stands: a hosted boot never falls back to standalone (§6).
pub fn boot_if_hosted() -> bool {
    frame_transport::artifact::boot_if_hosted(adopt)
}

/// The Shell's channel offer, adopted: from here the boundary is live, the
/// adoption's own status has gone out (that emission is how the Shell learns
/// which of its offered channels this boot took) and every envelope this
/// generation carries reaches [`on_frame`]. A re-offer for the same boot
/// adopts nothing — the first channel answered first.
fn adopt(wire: PortWire, generation: u64) {
    frame_transport::artifact::adopt(wire, generation, move |body| on_frame(body, generation));
}

/// One Shell envelope for this frame, already generation-guarded.
///
/// The reader cannot mount before the Shell's `init` (a reader IS its
/// document), so an envelope that arrives first has no session to address and
/// is dropped by the protocol, not by accident.
fn on_frame(body: ShellFrame, generation: u64) {
    match body {
        ShellFrame::Init {
            runtime,
            launch,
            hidden: _,
        } => {
            // A Reader init must name the document this fresh disposable host
            // was opened for.
            if runtime == RuntimeKind::Reader {
                on_init(launch, generation);
            }
        }
        ShellFrame::Launch { document } => {
            // An in-session open keeps this workspace and targets an
            // independent document pane, never a retained host.
            app_ui::frame_theme::mark_frame_hidden(false);
            if let Some(id) = session() {
                crate::command(id, *document);
            }
        }
        ShellFrame::Refresh => {
            // Refresh activates the Library, never a Reader host. Reader
            // document state stays in its independent panes.
        }
        ShellFrame::ResolveLaunchAnswer { request, document } => {
            with_api(|api| api.settle_launch(request, document.map(|document| *document)));
        }
        ShellFrame::Dispose => {
            with_api(|api| api.cancel_resolves());
            if let Some(id) = take_session() {
                let promise = crate::dispose(id);
                crate::diagnostics::set_reader_live(false);
                wasm_bindgen_futures::spawn_local(async move {
                    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
                    emit(RuntimeFrame::DisposeComplete);
                });
            } else {
                // Nothing mounted (an init never arrived, or a duplicate
                // dispose): the answer is still owed, or the Shell waits out
                // its forced-removal timeout.
                emit(RuntimeFrame::DisposeComplete);
            }
        }
        ShellFrame::CoverBaked { .. } | ShellFrame::ImportFiles { .. } => {
            // Cover answers and imports belong to Library.
        }
    }
}

/// The Shell's `init`: the frame's one launch. Mounts the runtime root the
/// handshake names (§9) and starts the session; everything after is the boot
/// stages on the port. Every Reader host is a fresh realm.
fn on_init(launch: Option<Box<LaunchDocument>>, generation: u64) {
    if session().is_some() {
        return;
    }
    let Some(root) = mount_root("reader", generation) else {
        return;
    };
    let Some(launch) = launch.filter(|launch| !launch.path.is_empty()) else {
        emit(RuntimeFrame::Failed {
            stage: BootStage::Failed,
            cause: "Reader host init requires a document".to_string(),
        });
        return;
    };
    app_ui::frame_theme::mark_frame_hidden(false);
    crate::diagnostics::begin_epoch(false);
    let id = crate::start_session(&root, *launch, ApiHandle::Frame);
    set_session(id);
    crate::diagnostics::set_reader_live(true);
    emit(RuntimeFrame::Status {
        stage: BootStage::Mounted,
    });
    emit(RuntimeFrame::Ready);
    report_painted();
}

/// The frame-side open flow: ask the Shell to resolve `path` against the
/// persisted library and await the answer before placing the document. The
/// frame has no synchronous boundary query — the only honest async form of
/// "open, resumed where the library says" is to wait for the answer.
///
/// The awaiting continuation keeps its placement: where the document goes was
/// decided when the user asked, not when the answer lands.
pub fn open_path_in_frame(ctx: ReaderContext, path: String, placement: Placement) {
    if session() != Some(ctx.id) || !ctx.pane.admits_work() {
        return;
    }
    let Some(ticket) = with_api(|api| api.resolve_launch(&path)) else {
        return;
    };
    wasm_bindgen_futures::spawn_local(async move {
        let document = ticket.await;
        if session() == Some(ctx.id) && ctx.pane.admits_work() {
            let launch =
                document.unwrap_or_else(|| crate::services::document::open::bare_launch(&path));
            ctx.open.try_run(OpenRequest { launch, placement });
        }
    });
}
