//! The reader session's context: the state slices this runtime owns, the
//! Phase 1 lifecycle owner, and the boundary to the Shell. This is what a
//! reader function receives instead of the old unified `ReaderContext` — the type
//! cannot name library state, which is the compile-level kill switch the
//! phase asks for.

use crate::state::ReaderState;
use app_state::state::UiState;
use leptos::prelude::*;
use reader_core::settings::Settings;
use runtime_contract::boundary::{DocStatusReport, LaunchDocument, ReadPoint, ShellApi};

/// The reader session's own state bundle. Field names match the old
/// `ReaderContext`'s reader-reachable paths (`ctx.reader.*`, `ctx.settings`,
/// `ctx.ui`) so the bodies moved out of the unified tree read the same; what
/// no longer EXISTS on this type is every library path.
#[derive(Clone, Copy)]
pub struct ReaderContext {
    pub reader: ReaderState,
    pub runtime: crate::runtime::ReaderRuntime,
    /// The session's settings copy: seeded from storage at start, persisted
    /// through the boundary when the reader edits it. The shell owns the
    /// durable key; the session owns the live value (persist data ≠ retain
    /// live object).
    pub settings: RwSignal<Settings>,
    pub ui: UiState,
    /// The boundary. Commands only — never a state handle from the Shell.
    pub api: ApiHandle,
    /// The launch descriptor the session was started with. Behind a signal
    /// only so the context stays Copy — every view closure captures it.
    pub launch: RwSignal<LaunchDocument>,
    /// The session id the manager knows this runtime by.
    pub id: u32,
    /// The shared-chrome handles this session provides: the title bar and
    /// appearance surfaces read these (chrome CODE is shared; chrome STATE is
    /// per-runtime — §9).
    pub chrome: app_state::ChromeState,
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
}

impl ShellApi for ApiHandle {
    fn open_document(&self, launch: &LaunchDocument) {
        match self {
            ApiHandle::Standalone => StandaloneApi.open_document(launch),
            ApiHandle::Frame => {
                crate::frame::with_api(|api| api.open_document(launch));
            }
        }
    }
    fn navigate_library(&self) {
        match self {
            ApiHandle::Standalone => StandaloneApi.navigate_library(),
            ApiHandle::Frame => {
                crate::frame::with_api(|api| api.navigate_library());
            }
        }
    }
    fn read_point(&self, point: &ReadPoint) {
        match self {
            ApiHandle::Standalone => StandaloneApi.read_point(point),
            ApiHandle::Frame => {
                crate::frame::with_api(|api| api.read_point(point));
            }
        }
    }
    fn save_settings(&self, settings: &Settings) {
        match self {
            ApiHandle::Standalone => StandaloneApi.save_settings(settings),
            ApiHandle::Frame => {
                crate::frame::with_api(|api| api.save_settings(settings));
            }
        }
    }
    fn save_library(&self, blob: &library_core::blob::LibraryBlob) {
        match self {
            ApiHandle::Standalone => StandaloneApi.save_library(blob),
            ApiHandle::Frame => {
                crate::frame::with_api(|api| api.save_library(blob));
            }
        }
    }
    fn save_covers(&self, covers: &runtime_contract::covers::CoverMap) {
        match self {
            ApiHandle::Standalone => StandaloneApi.save_covers(covers),
            ApiHandle::Frame => {
                crate::frame::with_api(|api| api.save_covers(covers));
            }
        }
    }
    fn save_cover(&self, path: &str, image: &runtime_contract::covers::CoverImage) {
        match self {
            ApiHandle::Standalone => StandaloneApi.save_cover(path, image),
            ApiHandle::Frame => {
                crate::frame::with_api(|api| api.save_cover(path, image));
            }
        }
    }
    fn bake_cover(&self, path: &str) {
        match self {
            ApiHandle::Standalone => StandaloneApi.bake_cover(path),
            ApiHandle::Frame => {
                crate::frame::with_api(|api| api.bake_cover(path));
            }
        }
    }
    fn doc_status(&self, report: &DocStatusReport) {
        match self {
            ApiHandle::Standalone => StandaloneApi.doc_status(report),
            ApiHandle::Frame => {
                crate::frame::with_api(|api| api.doc_status(report));
            }
        }
    }
    fn publish_digest(&self, json: String) {
        match self {
            ApiHandle::Standalone => StandaloneApi.publish_digest(json),
            ApiHandle::Frame => {
                crate::frame::with_api(|api| api.publish_digest(json));
            }
        }
    }
    fn reload(&self) {
        match self {
            ApiHandle::Standalone => StandaloneApi.reload(),
            ApiHandle::Frame => {
                crate::frame::with_api(|api| api.reload());
            }
        }
    }
    fn resolve_launch(&self, path: &str) -> Option<LaunchDocument> {
        match self {
            ApiHandle::Standalone => StandaloneApi.resolve_launch(path),
            ApiHandle::Frame => crate::frame::with_api(|api| api.resolve_launch(path)).flatten(),
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

/// The standalone substitute (`reader.html` with no Shell): durable writes
/// go straight to the browser store the Shell would have written, and
/// navigation commands are no-ops — the standalone page IS its own route.
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
    fn save_library(&self, blob: &library_core::blob::LibraryBlob) {
        let _ = storage::save_library(blob);
    }
    fn save_covers(&self, covers: &runtime_contract::covers::CoverMap) {
        let _ = storage::save_covers(covers);
    }
    fn save_cover(&self, path: &str, image: &runtime_contract::covers::CoverImage) {
        let mut map = storage::load_covers();
        map.insert(path.to_string(), std::sync::Arc::new(image.clone()));
        let _ = storage::save_covers(&map);
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
