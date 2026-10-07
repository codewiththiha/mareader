//! The Library artifact's frame boot: the URL marker and the
//! shelf's envelope meanings.

use frame_transport::wasm::PortWire;
use runtime_contract::protocol::{BootStage, RuntimeFrame, RuntimeKind, ShellFrame};

use crate::context::ApiHandle;

pub use frame_transport::artifact::{
    emit, frame_session, mount_root, report_painted, set_frame_session, take_frame_session,
    with_api,
};

/// Boot through the frame when the URL names one; no fallback (§6).
pub fn boot_if_hosted() -> bool {
    frame_transport::artifact::boot_if_hosted(adopt)
}

/// The Shell's channel offer, adopted: the boundary goes live and
/// envelopes reach [`on_frame`].
fn adopt(wire: PortWire, generation: u64) {
    frame_transport::artifact::adopt(wire, generation, move |body| on_frame(body, generation));
}

/// One generation-guarded Shell envelope. A hidden init defers startup
/// writes until reveal.
fn on_frame(body: ShellFrame, generation: u64) {
    match body {
        ShellFrame::Init {
            runtime,
            launch: _,
            hidden,
        } => {
            debug_assert_eq!(runtime, RuntimeKind::Library);
            on_init(hidden, generation);
        }
        ShellFrame::CoverBaked { path, image } => {
            if let Some(id) = frame_session() {
                crate::command(
                    id,
                    crate::LibraryCommand::CoverBaked {
                        path,
                        image: image.map(Box::new),
                    },
                );
            }
        }
        ShellFrame::Refresh => {
            // Fresh Library has painted. Reconcile late durable writes and
            // run its previously deferred startup once.
            app_ui::frame_theme::mark_frame_hidden(false);
            if let Some(id) = frame_session() {
                crate::command(id, crate::LibraryCommand::Refresh);
            }
        }
        ShellFrame::ImportFiles { paths } => {
            if let Some(id) = frame_session() {
                crate::command(id, crate::LibraryCommand::ImportFiles { paths });
            }
        }
        ShellFrame::Dispose => {
            if let Some(id) = take_frame_session() {
                let promise = crate::dispose(id);
                wasm_bindgen_futures::spawn_local(async move {
                    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
                    emit(RuntimeFrame::DisposeComplete);
                });
            } else {
                emit(RuntimeFrame::DisposeComplete);
            }
        }
        ShellFrame::Launch { .. } | ShellFrame::ResolveLaunchAnswer { .. } => {
            // A document launch and its answer are the reader's: dropped here.
        }
    }
}

/// The Shell's `init`: mount the runtime root and start the session.
fn on_init(hidden: bool, generation: u64) {
    // A re-init is never a second session.
    if frame_session().is_some() {
        return;
    }
    let Some(root) = mount_root("library", generation) else {
        return;
    };
    app_ui::frame_theme::mark_frame_hidden(hidden);
    let id = crate::start_session(&root, ApiHandle::Frame, hidden);
    set_frame_session(id);
    emit(RuntimeFrame::Status {
        stage: BootStage::Mounted,
    });
    emit(RuntimeFrame::Ready);
    report_painted();
}
