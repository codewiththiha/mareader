//! The library session's context: the library state, the session's settings
//! copy, its UI chrome slice, and the boundary to the Shell. No field on
//! this type can name reader state — the reader is another artifact.

use app_state::state::UiState;
use leptos::prelude::*;
use reader_core::settings::Settings;
use runtime_contract::boundary::ShellApi;

/// Which ShellApi implementation backs this session: the hosted frame or
/// the unhosted storage API (the test lane, no Shell). A Copy handle
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
    fn read_point(&self, point: &runtime_contract::boundary::ReadPoint) {
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
    fn save_cover(&self, path: &str, image: &runtime_contract::covers::CoverImage) {
        match self {
            ApiHandle::Standalone => StandaloneApi.save_cover(path, image),
            ApiHandle::Frame => {
                crate::frame::with_api(|api| api.save_cover(path, image));
            }
        }
    }
    /// The shelf never glosses: a mark is made in a reader, and the shelf's
    /// own gloss upkeep (a removed or duplicated row's marks) is row data it
    /// writes in its own frame, like the library blob. Nothing is sent.
    fn save_gloss(&self, _key: &str, _marks: String) {}
    fn bake_cover(&self, path: &str) {
        match self {
            ApiHandle::Standalone => StandaloneApi.bake_cover(path),
            ApiHandle::Frame => {
                crate::frame::with_api(|api| api.bake_cover(path));
            }
        }
    }
    fn doc_status(&self, report: &runtime_contract::boundary::DocStatusReport) {
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
}

#[derive(Clone, Copy)]
pub struct LibraryContext {
    pub library: crate::state::LibraryState,
    /// The session's settings copy: seeded from storage at start, persisted
    /// through the boundary when the library edits it (the appearance menu).
    pub settings: RwSignal<Settings>,
    pub ui: UiState,
    pub api: ApiHandle,
    /// The shared-chrome handles this session provides: its settings + ui
    /// slices with a dormant reader surface (the reader is another runtime).
    pub chrome: app_state::ChromeState,
}

impl LibraryContext {
    pub fn new(api: ApiHandle) -> Self {
        let library_blob = storage::load_library();
        // One-time, before any pane can ask for a row's marks: carry the
        // address-keyed highlights a build before the row-id scheme wrote
        // onto the rows that were reading them. The migration is guarded by
        // its own durable flag, so every later session is a single read.
        storage::migrate_gloss_keys(&library_blob.books);
        let settings = RwSignal::new(storage::load_settings());
        let sidebar = RwSignal::new(app_state::SidebarMode::None);
        let toast = RwSignal::new(None);
        let window_maximized = RwSignal::new(false);
        let ui = UiState {
            sidebar,
            toast,
            window_maximized,
        };
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
            chrome: app_state::ChromeState {
                settings,
                ui,
                reader: app_state::ReaderSurface {
                    reflowable: leptos::prelude::Signal::derive(|| false),
                    search_visible: leptos::prelude::Signal::derive(|| false),
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
    /// unhosted api has.
    fn default() -> Self {
        Self::new(ApiHandle::Standalone)
    }
}

/// The unhosted substitute (no Shell, as in unit tests): durable writes
/// go straight to the browser store.
struct StandaloneApi;

impl ShellApi for StandaloneApi {
    fn open_document(&self, _launch: &runtime_contract::boundary::LaunchDocument) {}
    fn navigate_library(&self) {}
    fn read_point(&self, point: &runtime_contract::boundary::ReadPoint) {
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
    /// No Shell, no baker: an unhosted session has nobody to ask, so the
    /// ask goes nowhere and the shelf shows the covers it already holds
    /// (`covers.rs` never starts an unhosted drain — this is only reached
    /// by the host tests' queue policy).
    fn bake_cover(&self, _path: &str) {}
    fn doc_status(&self, _report: &runtime_contract::boundary::DocStatusReport) {}
    fn publish_digest(&self, _json: String) {}
}
