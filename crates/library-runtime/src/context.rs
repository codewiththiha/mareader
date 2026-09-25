//! The library session's context: the library state, the session's settings
//! copy, its UI chrome slice, and the boundary to the Shell. No field on
//! this type can name reader state — the reader is another artifact.

use app_state::boundary::ShellApi;
use app_state::state::{CoverMap, UiState};
use leptos::prelude::*;
use reader_core::settings::Settings;

/// Which ShellApi implementation backs this session: the hosted bridge or
/// the standalone storage API (`library.html` without a Shell). A Copy handle
/// so the context itself stays Copy — every library service passes it by value.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ApiHandle {
    Js,
    Standalone,
}

impl ShellApi for ApiHandle {
    fn open_document(&self, launch: &app_state::boundary::LaunchDocument) {
        match self {
            ApiHandle::Js => JsShellApi.open_document(launch),
            ApiHandle::Standalone => StandaloneApi::new().open_document(launch),
        }
    }
    fn navigate_library(&self) {
        match self {
            ApiHandle::Js => JsShellApi.navigate_library(),
            ApiHandle::Standalone => StandaloneApi::new().navigate_library(),
        }
    }
    fn read_point(&self, point: &app_state::boundary::ReadPoint) {
        match self {
            ApiHandle::Js => JsShellApi.read_point(point),
            ApiHandle::Standalone => StandaloneApi::new().read_point(point),
        }
    }
    fn save_settings(&self, settings: &Settings) {
        match self {
            ApiHandle::Js => JsShellApi.save_settings(settings),
            ApiHandle::Standalone => StandaloneApi::new().save_settings(settings),
        }
    }
    fn save_library(&self, blob: &library_core::blob::LibraryBlob) {
        match self {
            ApiHandle::Js => JsShellApi.save_library(blob),
            ApiHandle::Standalone => StandaloneApi::new().save_library(blob),
        }
    }
    fn save_covers(&self, covers: &app_state::state::covers::CoverMap) {
        match self {
            ApiHandle::Js => JsShellApi.save_covers(covers),
            ApiHandle::Standalone => StandaloneApi::new().save_covers(covers),
        }
    }
    fn save_cover(&self, path: &str, image: &app_state::state::covers::CoverImage) {
        match self {
            ApiHandle::Js => JsShellApi.save_cover(path, image),
            ApiHandle::Standalone => StandaloneApi::new().save_cover(path, image),
        }
    }
    fn doc_status(&self, report: &app_state::boundary::DocStatusReport) {
        match self {
            ApiHandle::Js => JsShellApi.doc_status(report),
            ApiHandle::Standalone => StandaloneApi::new().doc_status(report),
        }
    }
    fn publish_digest(&self, json: String) {
        match self {
            ApiHandle::Js => JsShellApi.publish_digest(json),
            ApiHandle::Standalone => StandaloneApi::new().publish_digest(json),
        }
    }
    fn reload(&self) {
        match self {
            ApiHandle::Js => JsShellApi.reload(),
            ApiHandle::Standalone => StandaloneApi::new().reload(),
        }
    }
    fn resolve_launch(&self, path: &str) -> Option<app_state::boundary::LaunchDocument> {
        match self {
            ApiHandle::Js => JsShellApi.resolve_launch(path),
            ApiHandle::Standalone => StandaloneApi::new().resolve_launch(path),
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
        let sidebar_slide = RwSignal::new(app_state::state::reader::viewer::Motion::default());
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

/// The hosted bridge: `window.__mareaderShell`, JSON on the wire.
/// Every target gets the type: off-wasm there is no bridge, `call` is a
/// deliberate no-op, and the same session code runs unhosted without
/// pretending a shell exists.
pub struct JsShellApi;

impl JsShellApi {
    /// The webview path: reflect the bridge off `window` and call it.
    #[cfg(target_arch = "wasm32")]
    fn call(&self, method: &str, json: Option<String>) {
        use wasm_bindgen::JsCast;
        let Some(window) = web_sys::window() else {
            return;
        };
        let target: js_sys::Object = window.unchecked_into();
        let Ok(bridge) =
            js_sys::Reflect::get(&target, &wasm_bindgen::JsValue::from_str("__mareaderShell"))
        else {
            return;
        };
        if bridge.is_undefined() {
            return;
        }
        let bridge: js_sys::Object = bridge.unchecked_into();
        let f: js_sys::Function =
            js_sys::Reflect::get(&bridge, &wasm_bindgen::JsValue::from_str(method))
                .expect("bridge method")
                .unchecked_into();
        if let Some(json) = json {
            _ = f.call1(&bridge, &wasm_bindgen::JsValue::from_str(&json));
        } else {
            _ = f.call0(&bridge);
        }
    }

    /// The host path: no webview, no bridge, nothing to deliver.
    #[cfg(not(target_arch = "wasm32"))]
    fn call(&self, _method: &str, _json: Option<String>) {}
}

impl ShellApi for JsShellApi {
    fn open_document(&self, launch: &app_state::boundary::LaunchDocument) {
        self.call(
            "openDocument",
            Some(serde_json::to_string(launch).unwrap_or_default()),
        );
    }
    fn navigate_library(&self) {}
    fn read_point(&self, point: &app_state::boundary::ReadPoint) {
        self.call(
            "readPoint",
            Some(serde_json::to_string(point).unwrap_or_default()),
        );
    }
    fn save_settings(&self, settings: &Settings) {
        self.call(
            "saveSettings",
            Some(serde_json::to_string(settings).unwrap_or_default()),
        );
    }
    fn save_library(&self, blob: &library_core::blob::LibraryBlob) {
        self.call(
            "saveLibrary",
            Some(serde_json::to_string(blob).unwrap_or_default()),
        );
    }
    fn save_covers(&self, covers: &app_state::state::covers::CoverMap) {
        self.call(
            "saveCovers",
            Some(serde_json::to_string(covers).unwrap_or_default()),
        );
    }
    fn save_cover(&self, path: &str, image: &app_state::state::covers::CoverImage) {
        #[derive(serde::Serialize)]
        #[serde(rename_all = "camelCase")]
        struct One<'a> {
            path: &'a str,
            image: &'a app_state::state::covers::CoverImage,
        }
        self.call(
            "saveCover",
            Some(serde_json::to_string(&One { path, image }).unwrap_or_default()),
        );
    }
    fn doc_status(&self, report: &app_state::boundary::DocStatusReport) {
        self.call(
            "docStatus",
            Some(serde_json::to_string(report).unwrap_or_default()),
        );
    }
    fn publish_digest(&self, json: String) {
        self.call("publishDigest", Some(json));
    }
    fn reload(&self) {
        self.call("reload", None);
    }
    fn resolve_launch(&self, path: &str) -> Option<app_state::boundary::LaunchDocument> {
        storage::resolve_launch(path)
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
    fn open_document(&self, _launch: &app_state::boundary::LaunchDocument) {}
    fn navigate_library(&self) {}
    fn read_point(&self, point: &app_state::boundary::ReadPoint) {
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
    fn save_cover(&self, path: &str, image: &app_state::state::covers::CoverImage) {
        let mut map = storage::load_covers();
        map.insert(path.to_string(), std::sync::Arc::new(image.clone()));
        let _ = storage::save_covers(&map);
    }
    fn doc_status(&self, _report: &app_state::boundary::DocStatusReport) {}
    fn publish_digest(&self, _json: String) {}
    fn reload(&self) {
        app_chrome::window::api::reload_window();
    }
    fn resolve_launch(&self, path: &str) -> Option<app_state::boundary::LaunchDocument> {
        storage::resolve_launch(path)
    }
}
