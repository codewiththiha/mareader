//! Route-level glue: URL ⇄ document-state sync and the unmatched-path
//! fallback.

use leptos::prelude::*;
use leptos_router::hooks::{use_location, use_navigate};

use pdf_engine::types::DocStatus;
use crate::state::AppState;

/// URL follows document state: Ready ⇒ /reader, otherwise /.
///
/// The guard compares the current pathname before navigating, so a completed
/// navigation makes the effect a no-op and it can never loop.
#[component]
pub(crate) fn RouteSync(state: AppState) -> impl IntoView {
    let navigate = use_navigate();
    let loc = use_location();
    Effect::new(move |_| {
        let ready = state.reader.document.status.get() == DocStatus::Ready;
        let path = loc.pathname.get();
        if ready && path != "/reader" {
            navigate("/reader", Default::default());
        } else if !ready && path == "/reader" {
            navigate("/", Default::default());
        }
    });
}

/// Fallback for unmatched paths: bounce to the library.
#[component]
pub(crate) fn RedirectHome() -> impl IntoView {
    let navigate = use_navigate();
    Effect::new(move |_| navigate("/", Default::default()));
}
