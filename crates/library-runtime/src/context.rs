//! The library session's context: the library state, the session's settings
//! copy, its UI chrome slice, and the boundary to the Shell. No field on
//! this type can name reader state — the reader is another artifact.

use app_state::state::UiState;
use leptos::prelude::*;
use reader_core::settings::Settings;
use runtime_contract::boundary::ShellApi;
use runtime_contract::covers::CoverMap;

/// Which ShellApi implementation backs this session: the hosted frame or
/// the standalone storage API (`library.html` without a Shell). A Copy handle
/// so the context itself stays Copy — every library service passes it by value.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ApiHandle {
    Standalone,
    /// The hosted frame: the boundary calls leave over the frame's port,
    /// stamped with the boot's generation (`crate::frame`).
    Frame,
}

impl ShellApi for ApiHandle {
    fn open_document(&self, launch: &runtime_contract::boundary::LaunchDocument) {
        match self {
            ApiHandle::Standalone => StandaloneApi::new().open_document(launch),
            ApiHandle::Frame => {
                crate::frame::with_api(|api| api.open_document(launch));
            }
        }
    }
    fn navigate_library(&self) {
        match self {
            ApiHandle::Standalone => StandaloneApi::new().navigate_library(),
            ApiHandle::Frame => {
                crate::frame::with_api(|api| api.navigate_library());
            }
        }
    }
    fn read_point(&self, point: &runtime_contract::boundary::ReadPoint) {
        match self {
            ApiHandle::Standalone => StandaloneApi::new().read_point(point),
            ApiHandle::Frame => {
                crate::frame::with_api(|api| api.read_point(point));
            }
        }
    }
    fn save_settings(&self, settings: &Settings) {
        match self {
            ApiHandle::Standalone => StandaloneApi::new().save_settings(settings),
            ApiHandle::Frame => {
                crate::frame::with_api(|api| api.save_settings(settings));
            }
        }
    }
    fn save_library(&self, blob: &library_core::blob::LibraryBlob) {
        match self {
            ApiHandle::Standalone => StandaloneApi::new().save_library(blob),
            ApiHandle::Frame => {
                crate::frame::with_api(|api| api.save_library(blob));
            }
        }
    }
    fn save_covers(&self, covers: &runtime_contract::covers::CoverMap) {
        match self {
            ApiHandle::Standalone => StandaloneApi::new().save_covers(covers),
            ApiHandle::Frame => {
                crate::frame::with_api(|api| api.save_covers(covers));
            }
        }
    }
    fn save_cover(&self, path: &str, image: &runtime_contract::covers::CoverImage) {
        match self {
            ApiHandle::Standalone => StandaloneApi::new().save_cover(path, image),
            ApiHandle::Frame => {
                crate::frame::with_api(|api| api.save_cover(path, image));
            }
        }
    }
    fn bake_cover(&self, path: &str) {
        match self {
            ApiHandle::Standalone => StandaloneApi::new().bake_cover(path),
            ApiHandle::Frame => {
                crate::frame::with_api(|api| api.bake_cover(path));
            }
        }
    }
    fn doc_status(&self, report: &runtime_contract::boundary::DocStatusReport) {
        match self {
            ApiHandle::Standalone => StandaloneApi::new().doc_status(report),
            ApiHandle::Frame => {
                crate::frame::with_api(|api| api.doc_status(report));
            }
        }
    }
    fn publish_digest(&self, json: String) {
        match self {
            ApiHandle::Standalone => StandaloneApi::new().publish_digest(json),
            ApiHandle::Frame => {
                crate::frame::with_api(|api| api.publish_digest(json));
            }
        }
    }
    fn reload(&self) {
        match self {
            ApiHandle::Standalone => StandaloneApi::new().reload(),
            ApiHandle::Frame => {
                crate::frame::with_api(|api| api.reload());
            }
        }
    }
    fn resolve_launch(&self, path: &str) -> Option<runtime_contract::boundary::LaunchDocument> {
        match self {
            ApiHandle::Standalone => StandaloneApi::new().resolve_launch(path),
            ApiHandle::Frame => crate::frame::with_api(|api| api.resolve_launch(path)).flatten(),
        }
    }
}

#[derive(Clone, Copy)]
pub struct LibraryContext {
    pub library: crate::state::LibraryState,
    /// The session's settings copy: seeded from storage at start, persisted
    /// through the boundary when the library edits it (the appearance menu).
    pub settings: RwSignal<Settings>,
    pub ui: UiState,
    pub api: ApiHandle,
    pub id: u32,
    /// The shared-chrome handles this session provides: its settings + ui
    /// slices with a dormant reader surface (the reader is another runtime).
    pub chrome: app_state::ChromeState,
}

impl LibraryContext {
    pub fn new(api: ApiHandle) -> Self {
        let library_blob = storage::load_library();
        let settings = RwSignal::new(storage::load_settings());
        let sidebar = RwSignal::new(app_state::SidebarMode::None);
        let toast = RwSignal::new(None);
        let window_maximized = RwSignal::new(false);
        let ui = UiState {
            sidebar,
            toast,
            window_maximized,
        };
        let search_visible = RwSignal::new(false);
        let sidebar_slide = RwSignal::new(app_state::state::Motion::default());
        Self {
            library: crate::state::LibraryState {
                books: RwSignal::new(library_blob.books),
                shelves: RwSignal::new(library_blob.shelves),
                folders: RwSignal::new(library_blob.folders),
                view: RwSignal::new(library_blob.view),
                covers: RwSignal::new(storage::load_covers()),
                ..crate::state::LibraryState::default()
            },
            settings,
            ui,
            api,
            id: 0,
            chrome: app_state::ChromeState {
                settings,
                ui,
                reader: app_state::ReaderSurface {
                    reflowable: leptos::prelude::Signal::derive(|| false),
                    search_visible,
                    sidebar_slide,
                },
            },
        }
    }
}

#[cfg(test)]
impl Default for LibraryContext {
    /// The unhosted session context unit tests build on: no shell bridge, so
    /// every boundary call is the deliberate no-op — the same shape the
    /// standalone artifact boots with.
    fn default() -> Self {
        Self::new(ApiHandle::Standalone)
    }
}

/// The standalone substitute (`library.html` with no Shell): durable writes
/// go straight to the browser store.
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
    fn open_document(&self, _launch: &runtime_contract::boundary::LaunchDocument) {}
    fn navigate_library(&self) {}
    fn read_point(&self, point: &runtime_contract::boundary::ReadPoint) {
        storage::apply_read_point(point);
    }
    fn save_settings(&self, settings: &Settings) {
        let _ = storage::save_settings(settings);
    }
    fn save_library(&self, blob: &library_core::blob::LibraryBlob) {
        let _ = storage::save_library(blob);
    }
    fn save_covers(&self, covers: &CoverMap) {
        let _ = storage::save_covers(covers);
    }
    fn save_cover(&self, path: &str, image: &runtime_contract::covers::CoverImage) {
        let mut map = storage::load_covers();
        map.insert(path.to_string(), std::sync::Arc::new(image.clone()));
        let _ = storage::save_covers(&map);
    }
    /// Never reached: the standalone bake runs from `covers.rs`' drain
    /// directly, because it needs this session's context to file the answer
    /// — see [`crate::services::cover_engine`].
    fn bake_cover(&self, _path: &str) {}
    fn doc_status(&self, _report: &runtime_contract::boundary::DocStatusReport) {}
    fn publish_digest(&self, _json: String) {}
    fn reload(&self) {
        app_chrome::window::api::reload_window();
    }
    fn resolve_launch(&self, path: &str) -> Option<runtime_contract::boundary::LaunchDocument> {
        storage::resolve_launch(path)
    }
}
