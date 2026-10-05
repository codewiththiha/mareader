//! The Library artifact boots in its own disposable iframe. The URL
//! authenticates the Shell's MessageChannel offer; every message is stamped
//! with that frame's generation,
//! under the protocol vocabulary (`runtime-contract::protocol`).
//!
//! The boot every artifact frame shares — the URL marker, the channel offer,
//! the boundary, the runtime root, the paint report — is
//! [`frame_transport::artifact`]. What stays here is what the shelf's
//! envelopes mean.
//!
//! The session itself is the one `start_session`: the boot is a transport,
//! never a second implementation of the shelf.

use frame_transport::wasm::PortWire;
use runtime_contract::protocol::{BootStage, RuntimeFrame, RuntimeKind, ShellFrame};

use crate::context::ApiHandle;

pub use frame_transport::artifact::{
    emit, frame_session, mount_root, report_painted, set_frame_session, take_frame_session,
    with_api,
};

/// Boot through the frame when this artifact's URL names one.
///
/// Returns `true` as soon as the marker stands — before any offer arrives —
/// because §6 forbids the fallback: a hosted boot that never hears from its
/// Shell stays a claimless frame, never a standalone page.
pub fn boot_if_hosted() -> bool {
    frame_transport::artifact::boot_if_hosted(adopt)
}

/// The Shell's channel offer, adopted: from here the boundary is live, the
/// adoption's own status has gone out (that emission is how the Shell learns
/// which of its offered channels this boot took) and every envelope this
/// generation carries reaches [`on_frame`]. A re-offer for the same boot
/// adopts nothing — the first channel answered first, and a second listener
/// would only double the traffic.
fn adopt(wire: PortWire, generation: u64) {
    frame_transport::artifact::adopt(wire, generation, move |body| on_frame(body, generation));
}

/// One Shell envelope for this frame, already generation-guarded. The init's
/// visibility flag defers startup writes until reveal: no route may write
/// another route's store before its incoming paint.
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
            // Neither means anything to the shelf: a document launch is the
            // reader's command, and the library never asks the Shell to
            // resolve a launch. Dropped by the protocol, not by accident.
        }
    }
}

/// The Shell's `init`: mount the runtime root (§9) and start the session.
/// Hidden means an incoming route: paint first, start passes on reveal.
fn on_init(hidden: bool, generation: u64) {
    // A re-init for a boot that already mounted is not a second session: the
    // Shell mints one identity per frame and never reuses one.
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
