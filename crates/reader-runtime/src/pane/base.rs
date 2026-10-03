//! The slice every pane implementation starts from.
//!
//! A pane — the in-process [`crate::pane::document::DocumentPane`] or the
//! host-side [`crate::frame_pane::FramePane`] over a pane realm — builds the
//! same three things before it renders anything: the pane handle's reader
//! state, the [`ReaderContext`] the reader functions receive, and the
//! neutral [`PaneSurface`] projection of that state the host publishes. The
//! two implementations differ in where the document runs, never in this
//! base, so it has one owner here.

use leptos::prelude::*;
use runtime_contract::boundary::LaunchDocument;

use crate::context::ReaderContext;
use crate::host::contract::{PaneDocStatus, PaneEnv, PaneSurface};
use crate::pane::handle::PaneHandle;
use crate::state::ReaderState;
use reader_core::document::DocStatus;

/// A launch for a pane with nothing to open: a documentless host slot, or a
/// pane whose first open has not been named yet.
pub(crate) fn empty_launch() -> LaunchDocument {
    LaunchDocument {
        book_id: None,
        path: String::new(),
        resume_page: 1,
        saved_fraction: None,
        blend_override: false,
        cover_data_url: None,
        display_name: None,
    }
}

/// The engine's status in the host's words.
pub(crate) fn status_word(status: DocStatus) -> PaneDocStatus {
    match status {
        DocStatus::Idle => PaneDocStatus::Idle,
        DocStatus::Opening => PaneDocStatus::Opening,
        DocStatus::Ready => PaneDocStatus::Ready,
        DocStatus::Error => PaneDocStatus::Error,
    }
}

/// Build the pane's state, context and surface under the caller's reactive
/// owner: `handle` is provided as a context (the components a pane mounts
/// register what they create with it), the reader state is created on it,
/// and the surface derives from that state so the host publishes live
/// facts rather than a snapshot.
pub(crate) fn contexts(
    env: PaneEnv,
    handle: PaneHandle,
    launch: LaunchDocument,
) -> (ReaderContext, PaneSurface) {
    provide_context(handle);
    let reader = ReaderState::new(handle);
    let ctx = ReaderContext {
        reader,
        pane: handle,
        settings: env.settings,
        ui: env.ui,
        api: env.api,
        launch: RwSignal::new(launch),
        id: env.session_id,
        chrome: env.chrome,
        open: env.open,
        can_split: env.can_split,
        moves: env.moves,
        relocate: env.relocate,
    };
    let status = reader.document.status;
    let error = reader.document.error;
    let surface = PaneSurface {
        status: Signal::derive(move || status_word(status.get())),
        error: Signal::derive(move || error.get()),
        page: reader.viewer.page.into(),
        reflowable: Signal::derive(move || reader.reflowable()),
        search_visible: reader.search.visible.into(),
        name: Signal::derive(move || reader.document.display_name()),
    };
    (ctx, surface)
}
