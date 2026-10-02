//! A pane's context: the pane's OWN reader state, its handle (identity,
//! lifecycle gate, resource registry), the session's settings/UI slices and
//! the boundary to the Shell. This is what every reader function receives.
//! The type cannot name library state, the Shell's state, or another pane's
//! state: each pane builds its own (`crate::pane::document`), and the host
//! never holds one.

use crate::state::ReaderState;
use app_state::state::UiState;
use leptos::prelude::*;
use reader_core::settings::Settings;
use runtime_contract::boundary::{DocStatusReport, LaunchDocument, ReadPoint, ShellApi};

/// One pane's state bundle. Field names keep the reader-reachable paths
/// (`ctx.reader.*`, `ctx.settings`, `ctx.ui`) the reader's bodies read; what
/// does not EXIST on this type is every library path and the session's
/// lifecycle owner (a pane asks its own handle whether it may work).
#[derive(Clone, Copy)]
pub struct ReaderContext {
    /// THIS pane's document, viewer, search, AI-selection and gloss state —
    /// created with the pane, dropped with the pane.
    pub reader: ReaderState,
    /// The pane's handle: its id, its lifecycle gate, the resources its
    /// dispose owns.
    pub pane: crate::pane::handle::PaneHandle,
    /// The session's settings copy: seeded from storage at start, persisted
    /// through the boundary when the reader edits it. The shell owns the
    /// durable key; the session owns the live value (persist data ≠ retain
    /// live object).
    pub settings: RwSignal<Settings>,
    pub ui: UiState,
    /// The boundary. Commands only — never a state handle from the Shell.
    pub api: ApiHandle,
    /// The launch this pane's current document was opened with. Behind a
    /// signal only so the context stays Copy — every view closure captures
    /// it.
    pub launch: RwSignal<LaunchDocument>,
    /// The session id the manager knows this runtime by.
    pub id: u32,
    /// The shared-chrome handles the HOST built for this session: the title
    /// bar and appearance surfaces read these (chrome CODE is shared; chrome
    /// STATE is per-runtime — §9). Its reader surface follows the host's
    /// active pane.
    pub chrome: app_state::ChromeState,
    /// The HOST's workspace open command. A document this pane's user picks
    /// (Cmd/Ctrl+O, the frame's resolved open, a split) is handed to the
    /// host with where it goes relative to this pane — here, or in a new
    /// pane beside it — and the host places it: the pane never opens a
    /// document on its own authority.
    pub open: Callback<crate::host::contract::OpenRequest>,
    /// Whether the workspace would take another pane now (the view menu's
    /// split items follow it).
    pub can_split: Signal<bool>,
    /// Which ways the host's layout could move this pane now.
    pub moves: Signal<crate::host::tree::Moves>,
    /// The host's move command for this pane (the view menu's Move items).
    pub relocate: Callback<crate::host::tree::MoveDirection>,
}

/// Which ShellApi implementation backs this session: the hosted frame or the
/// standalone storage API. A Copy handle so the context stays cheaply clonable
/// and Send — the Rc it replaces could not cross a view closure.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ApiHandle {
    Standalone,
    /// The hosted frame: the boundary calls leave over the frame's port,
    /// stamped with the boot's generation (`crate::frame`).
    Frame,
    /// A pane frame: the calls leave over the pane's port to the workspace
    /// host, which answers them (`crate::pane_frame`).
    Pane,
}

impl ShellApi for ApiHandle {
    fn open_document(&self, launch: &LaunchDocument) {
        match self {
            ApiHandle::Standalone => StandaloneApi.open_document(launch),
            ApiHandle::Frame => {
                crate::frame::with_api(|api| api.open_document(launch));
            }
            ApiHandle::Pane => {
                crate::pane_frame::with_api(|api| api.open_document(launch));
            }
        }
    }
    fn navigate_library(&self) {
        match self {
            ApiHandle::Standalone => StandaloneApi.navigate_library(),
            ApiHandle::Frame => {
                crate::frame::with_api(|api| api.navigate_library());
            }
            ApiHandle::Pane => {
                crate::pane_frame::with_api(|api| api.navigate_library());
            }
        }
    }
    fn read_point(&self, point: &ReadPoint) {
        match self {
            ApiHandle::Standalone => StandaloneApi.read_point(point),
            ApiHandle::Frame => {
                crate::frame::with_api(|api| api.read_point(point));
            }
            ApiHandle::Pane => {
                crate::pane_frame::with_api(|api| api.read_point(point));
            }
        }
    }
    fn save_settings(&self, settings: &Settings) {
        match self {
            ApiHandle::Standalone => StandaloneApi.save_settings(settings),
            ApiHandle::Frame => {
                crate::frame::with_api(|api| api.save_settings(settings));
            }
            ApiHandle::Pane => {
                crate::pane_frame::with_api(|api| api.save_settings(settings));
            }
        }
    }
    fn save_cover(&self, path: &str, image: &runtime_contract::covers::CoverImage) {
        match self {
            ApiHandle::Standalone => StandaloneApi.save_cover(path, image),
            ApiHandle::Frame => {
                crate::frame::with_api(|api| api.save_cover(path, image));
            }
            ApiHandle::Pane => {
                crate::pane_frame::with_api(|api| api.save_cover(path, image));
            }
        }
    }
    fn save_gloss(&self, key: &str, marks: String) {
        match self {
            ApiHandle::Standalone => StandaloneApi.save_gloss(key, marks),
            ApiHandle::Frame => {
                crate::frame::with_api(|api| api.save_gloss(key, marks));
            }
            ApiHandle::Pane => {
                crate::pane_frame::with_api(|api| api.save_gloss(key, marks));
            }
        }
    }
    /// The reader never asks for a bake: it owns the engine, so a cover it
    /// wants is one it renders and files with `save_cover`. Bakes are the
    /// shelf's asks, and the Shell refuses one from any other frame kind —
    /// nothing is sent.
    fn bake_cover(&self, _path: &str) {}
    fn doc_status(&self, report: &DocStatusReport) {
        match self {
            ApiHandle::Standalone => StandaloneApi.doc_status(report),
            ApiHandle::Frame => {
                crate::frame::with_api(|api| api.doc_status(report));
            }
            ApiHandle::Pane => {
                crate::pane_frame::with_api(|api| api.doc_status(report));
            }
        }
    }
    fn publish_digest(&self, json: String) {
        match self {
            ApiHandle::Standalone => StandaloneApi.publish_digest(json),
            ApiHandle::Frame => {
                crate::frame::with_api(|api| api.publish_digest(json));
            }
            ApiHandle::Pane => {
                crate::pane_frame::with_api(|api| api.publish_digest(json));
            }
        }
    }
    fn reload(&self) {
        match self {
            ApiHandle::Standalone => StandaloneApi.reload(),
            ApiHandle::Frame => {
                crate::frame::with_api(|api| api.reload());
            }
            ApiHandle::Pane => {
                crate::pane_frame::with_api(|api| api.reload());
            }
        }
    }
    fn resolve_launch(&self, path: &str) -> Option<LaunchDocument> {
        match self {
            ApiHandle::Standalone => StandaloneApi.resolve_launch(path),
            ApiHandle::Frame => crate::frame::with_api(|api| api.resolve_launch(path)).flatten(),
            ApiHandle::Pane => None,
        }
    }
}

impl ReaderContext {
    /// Where the reader got to right now, as a boundary write — or `None`
    /// while the session's signals are gone.
    ///
    /// Clamped the way the flush always clamped; the fraction rides along for
    /// the continuous stream. Every read goes through `try_` because BOTH
    /// callers can outlive the reader they describe: the debounced progress
    /// timer fires up to a debounce window after the change it was armed for,
    /// and a close inside that window disposes this state. A disposed reader
    /// has no position to record — the dispose flush has already carried the
    /// durable one across the boundary (§15).
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

/// The unhosted substitute (no Shell, as in unit tests): durable writes
/// go straight to the browser store the Shell would have written, and
/// navigation commands are no-ops.
pub struct StandaloneApi;

impl StandaloneApi {
    pub fn new() -> Self {
        Self
    }
}

impl Default for StandaloneApi {
    fn default() -> Self {
        Self::new()
    }
}

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
    fn reload(&self) {
        app_chrome::window::api::reload_window();
    }
    fn resolve_launch(&self, path: &str) -> Option<LaunchDocument> {
        storage::resolve_launch(path)
    }
}
