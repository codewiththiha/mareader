//! The pane realm: one document pane in a frame of its own
//! (docs/pane-runtimes.md). The bins `pdf` and `reflow` call [`boot`]; the
//! workspace host's half of the conversation is `crate::frame_pane`.
//!
//! The frame runs the unchanged `DocumentPane` with a `PaneEnv` built from
//! the port: the host's settings, appearance, workspace flags, focus and
//! layout facts arrive as messages and land in local signals; the pane's
//! chrome-facing state leaves as a [`Mirror`] whenever it changes, and its
//! own shell calls leave as envelopes the host answers. Closing the pane is
//! the host removing this frame — everything here goes with the realm.

#[cfg(target_arch = "wasm32")]
mod realm;
#[cfg(target_arch = "wasm32")]
mod thumbs;

use crate::host::contract::Placement;
use crate::pane_wire::PaneKind;

/// The wire the pane's shell calls leave on: each envelope rides the port
/// inside a [`crate::pane_wire::PaneToHost::Api`].
#[derive(Clone)]
pub struct PaneApiWire;

impl frame_transport::Wire for PaneApiWire {
    fn post_json(&self, json: String) {
        #[cfg(target_arch = "wasm32")]
        realm::send(&crate::pane_wire::PaneToHost::Api { envelope: json });
        #[cfg(not(target_arch = "wasm32"))]
        let _ = json;
    }
}

/// Boot the pane artifact. Not hosted (no `?pane=` nonce in the URL), it
/// does nothing: a pane frame only ever runs for a host.
pub fn boot(kind: PaneKind) {
    #[cfg(target_arch = "wasm32")]
    realm::boot(kind);
    #[cfg(not(target_arch = "wasm32"))]
    let _ = kind;
}

/// Run `f` against the pane's shell api, while the port is live.
pub fn with_api<R>(f: impl FnOnce(&frame_transport::PortShellApi<PaneApiWire>) -> R) -> Option<R> {
    #[cfg(target_arch = "wasm32")]
    {
        realm::with_api(f)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = f;
        None
    }
}

/// The pane's open dialog picked `path`: the host resolves it into a launch
/// and places it.
pub fn open_path(path: String, placement: Placement) {
    #[cfg(target_arch = "wasm32")]
    realm::send(&crate::pane_wire::PaneToHost::OpenPath {
        path,
        placement: crate::pane_wire::WirePlacement::from(placement),
    });
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (path, placement);
}
