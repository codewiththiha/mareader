//! The pane realm: one document pane in its own frame.

#[cfg(target_arch = "wasm32")]
mod realm;
#[cfg(all(target_arch = "wasm32", feature = "pdf"))]
mod thumbs;

use crate::host::contract::Placement;
use crate::pane_wire::PaneKind;

/// The look was re-baked: the host's rail must re-render.
pub fn pictures_stale() {
    #[cfg(all(target_arch = "wasm32", feature = "pdf"))]
    thumbs::pictures_stale();
}

/// The wire the pane's shell calls leave on.
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

/// Boot the pane artifact; unhosted, does nothing.
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

/// The pane's open dialog picked `path`.
pub fn open_path(path: String, placement: Placement) {
    #[cfg(target_arch = "wasm32")]
    realm::send(&crate::pane_wire::PaneToHost::OpenPath {
        path,
        placement: crate::pane_wire::WirePlacement::from(placement),
    });
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (path, placement);
}
