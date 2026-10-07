//! The disposable Reader workspace-host iframe; removing it retires
//! the realms.

use frame_transport::wasm::PortWire;
use leptos::prelude::Callable;
use runtime_contract::boundary::LaunchDocument;
use runtime_contract::protocol::{BootStage, RuntimeFrame, RuntimeKind, ShellFrame};

use crate::context::{ApiHandle, ReaderContext};
use crate::host::contract::{OpenRequest, Placement};

pub use frame_transport::artifact::{
    emit, frame_session, mount_root, report_painted, set_frame_session, take_frame_session,
    with_api,
};

/// Boot through the frame when the URL names one.
pub fn boot_if_hosted() -> bool {
    frame_transport::artifact::boot_if_hosted(adopt)
}

/// The Shell's channel offer, adopted; the boundary is live from
/// here.
fn adopt(wire: PortWire, generation: u64) {
    frame_transport::artifact::adopt(wire, generation, move |body| on_frame(body, generation));
}

/// One Shell envelope, already generation-guarded.
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
            // An in-session open targets an
            // independent document pane.
            app_ui::frame_theme::mark_frame_hidden(false);
            if let Some(id) = frame_session() {
                crate::command(id, *document);
            }
        }
        ShellFrame::Refresh => {
            // Refresh activates the Library,
            // never a Reader host.
        }
        ShellFrame::ResolveLaunchAnswer { request, document } => {
            with_api(|api| api.settle_launch(request, document.map(|document| *document)));
        }
        ShellFrame::Dispose => {
            with_api(|api| api.cancel_resolves());
            if let Some(id) = take_frame_session() {
                let promise = crate::dispose(id);
                crate::diagnostics::set_reader_live(false);
                wasm_bindgen_futures::spawn_local(async move {
                    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
                    emit(RuntimeFrame::DisposeComplete);
                });
            } else {
                // Nothing mounted: the answer is still owed.
                emit(RuntimeFrame::DisposeComplete);
            }
        }
        ShellFrame::CoverBaked { .. } | ShellFrame::ImportFiles { .. } => {
            // Cover answers and imports belong to Library.
        }
    }
}

/// The Shell's `init`: mounts the runtime root and starts the
/// session.
fn on_init(launch: Option<Box<LaunchDocument>>, generation: u64) {
    if frame_session().is_some() {
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
    set_frame_session(id);
    crate::diagnostics::set_reader_live(true);
    emit(RuntimeFrame::Status {
        stage: BootStage::Mounted,
    });
    emit(RuntimeFrame::Ready);
    report_painted();
}

/// The frame-side open flow: ask the Shell, await, then place.
pub fn open_path_in_frame(ctx: ReaderContext, path: String, placement: Placement) {
    if frame_session() != Some(ctx.id) || !ctx.pane.admits_work() {
        return;
    }
    let Some(ticket) = with_api(|api| api.resolve_launch(&path)) else {
        return;
    };
    wasm_bindgen_futures::spawn_local(async move {
        let document = ticket.await;
        if frame_session() == Some(ctx.id) && ctx.pane.admits_work() {
            let launch =
                document.unwrap_or_else(|| crate::services::document::open::bare_launch(&path));
            ctx.open.try_run(OpenRequest { launch, placement });
        }
    });
}
