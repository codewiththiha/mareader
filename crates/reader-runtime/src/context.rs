//! A pane's context: its own reader state and handle, and the
//! boundary to the Shell.

use crate::state::ReaderState;
use app_state::state::UiState;
use leptos::prelude::*;
use reader_core::settings::Settings;
use runtime_contract::boundary::{DocStatusReport, LaunchDocument, ReadPoint, ShellApi};

/// One pane's state bundle: no library path, no lifecycle owner.
#[derive(Clone, Copy)]
pub struct ReaderContext {
    /// THIS pane's document, viewer, search and gloss state.
    pub reader: ReaderState,
    /// The pane's handle: its id, its lifecycle gate, the resources its
    /// dispose owns.
    pub pane: crate::pane::handle::PaneHandle,
    /// The session's settings copy, persisted through the boundary.
    pub settings: RwSignal<Settings>,
    pub ui: UiState,
    /// The boundary. Commands only — never a state handle from the Shell.
    pub api: ApiHandle,
    /// The launch this pane's document opened with; a signal keeps the
    /// context Copy.
    pub launch: RwSignal<LaunchDocument>,
    /// The session id the manager knows this runtime by.
    pub id: u32,
    /// The shared-chrome handles the host built for this session.
    pub chrome: app_state::ChromeState,
    /// The host's workspace open command: a pane never opens on its own
    /// authority.
    pub open: Callback<crate::host::contract::OpenRequest>,
    /// Whether the workspace would take another pane now (the view menu's
    /// split items follow it).
    pub can_split: Signal<bool>,
    /// Which ways the host's layout could move this pane now.
    pub moves: Signal<crate::host::tree::Moves>,
    /// The host's move command for this pane (the view menu's Move items).
    pub relocate: Callback<crate::host::tree::MoveDirection>,
}

/// Which ShellApi backs this session: hosted frame, standalone
/// storage, or a pane frame.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ApiHandle {
    Standalone,
    /// The hosted frame: calls leave over the frame's port.
    Frame,
    /// A pane frame: calls leave over the pane's port to the host.
    Pane,
}

impl ApiHandle {
    /// Route one Shell call to this session's transport. `None` while
    /// no port is adopted.
    fn route<R>(&self, call: impl FnOnce(&dyn ShellApi) -> R) -> Option<R> {
        match self {
            ApiHandle::Standalone => Some(call(&StandaloneApi)),
            ApiHandle::Frame => crate::frame::with_api(|api| call(api)),
            ApiHandle::Pane => crate::pane_frame::with_api(|api| call(api)),
        }
    }
}

impl ShellApi for ApiHandle {
    fn open_document(&self, launch: &LaunchDocument) {
        self.route(|api| api.open_document(launch));
    }
    fn navigate_library(&self) {
        self.route(|api| api.navigate_library());
    }
    fn read_point(&self, point: &ReadPoint) {
        self.route(|api| api.read_point(point));
    }
    fn save_settings(&self, settings: &Settings) {
        self.route(|api| api.save_settings(settings));
    }
    fn save_cover(&self, path: &str, image: &runtime_contract::covers::CoverImage) {
        self.route(|api| api.save_cover(path, image));
    }
    fn save_gloss(&self, key: &str, marks: String) {
        self.route(|api| api.save_gloss(key, marks));
    }
    /// The reader never asks for a bake: it owns the engine. Nothing sent.
    fn bake_cover(&self, _path: &str) {}
    fn doc_status(&self, report: &DocStatusReport) {
        self.route(|api| api.doc_status(report));
    }
    fn publish_digest(&self, json: String) {
        self.route(|api| api.publish_digest(json));
    }
}

impl ReaderContext {
    /// Where the reader got to, or `None` once the signals are gone.
    pub fn try_read_point(&self) -> Option<ReadPoint> {
        use reader_core::view::ViewMode;
        let num_pages = self.reader.document.num_pages.try_get_untracked()?;
        let page = self.reader.viewer.page.try_get_untracked()?;
        let page = page.clamp(1, num_pages.max(1));
        let format = self.reader.document.format.try_get_untracked()?;
        let mode = self.reader.viewer.mode.try_get_untracked()?;
        let streaming = format.is_reflowable() && mode == ViewMode::ScrollVertical;
        let fraction = if streaming {
            self.reader.stream_fraction()
        } else {
            None
        };
        let path = match self.reader.document.path.try_get_untracked()? {
            Some(path) => path,
            None => self.launch.try_with_untracked(|l| l.path.clone())?,
        };
        Some(ReadPoint {
            book_id: self.reader.document.book_id.try_get_untracked()?,
            path,
            page,
            num_pages,
            fraction,
            title: self.reader.document.title.try_get_untracked()?,
            author: self.reader.document.author.try_get_untracked()?,
        })
    }
}

/// The unhosted substitute: durable writes go to the browser store.
struct StandaloneApi;

impl ShellApi for StandaloneApi {
    fn open_document(&self, _launch: &LaunchDocument) {}
    fn navigate_library(&self) {}
    fn read_point(&self, point: &ReadPoint) {
        storage::apply_read_point(point);
    }
    fn save_settings(&self, settings: &Settings) {
        let _ = storage::save_settings(settings);
    }
    fn save_cover(&self, path: &str, image: &runtime_contract::covers::CoverImage) {
        let mut map = storage::load_covers();
        map.insert(path.to_string(), std::sync::Arc::new(image.clone()));
        let _ = storage::save_covers(&map);
    }
    fn save_gloss(&self, key: &str, marks: String) {
        storage::persist_encoded_gloss(key, &marks);
    }
    fn bake_cover(&self, _path: &str) {}
    fn doc_status(&self, _report: &runtime_contract::boundary::DocStatusReport) {}
    fn publish_digest(&self, _json: String) {}
}
