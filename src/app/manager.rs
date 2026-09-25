//! The runtime manager: the one navigation authority (§10) and the owner of
//! "which runtime is active". Its type makes two primary runtimes
//! unspeakable, and it never starts a runtime before the previous one's
//! dispose promise resolved (§5).

use std::cell::RefCell;
use std::sync::Mutex;

use app_state::boundary::LaunchDocument;
use wasm_bindgen::JsCast;

use crate::state::{ActiveRuntime, ShellState};

/// The active-runtime slot. `Starting` holds the in-flight disposal/load so a
/// second navigation cannot start a second runtime mid-transition.
pub enum Slot {
    None,
    Starting,
    Library { id: u32, module: js_sys::Object },
    Reader { id: u32, module: js_sys::Object },
}

pub struct RuntimeManager {
    slot: Mutex<Slot>,
    /// Create/dispose counts for the diagnostics identity (§21): a reader →
    /// library transition must leave active reader = none, library = one.
    pub reader_sessions_created: std::sync::atomic::AtomicU64,
    pub reader_disposes_completed: std::sync::atomic::AtomicU64,
    pub library_sessions_created: std::sync::atomic::AtomicU64,
    host: Mutex<Option<web_sys::Element>>,
    /// The last reader digest, cached for the probe (the reader pushes on
    /// every change that matters).
    pub last_digest: Mutex<Option<serde_json::Value>>,
    pub doc_status: Mutex<String>,
    pub doc_error: Mutex<Option<String>>,
}

thread_local! {
    /// The library module namespace, cached after the first load (compiled
    /// code, not live state).
    static LIBRARY_MODULE: RefCell<Option<js_sys::Object>> = const { RefCell::new(None) };
}

impl RuntimeManager {
    pub fn new() -> Self {
        Self {
            slot: Mutex::new(Slot::None),
            reader_sessions_created: Default::default(),
            reader_disposes_completed: Default::default(),
            library_sessions_created: Default::default(),
            host: Mutex::new(None),
            last_digest: Mutex::new(None),
            doc_status: Mutex::new("Idle".to_string()),
            doc_error: Mutex::new(None),
        }
    }

    pub fn set_host(&self, host: web_sys::Element) {
        *self.host.lock().unwrap() = Some(host);
    }

    pub fn active(&self) -> Option<ActiveRuntime> {
        match &*self.slot.lock().unwrap() {
            Slot::Library { .. } => Some(ActiveRuntime::Library),
            Slot::Reader { .. } => Some(ActiveRuntime::Reader),
            _ => None,
        }
    }

    fn host(&self) -> web_sys::Element {
        self.host
            .lock()
            .unwrap()
            .clone()
            .expect("runtime host element")
    }

    /// Boot: whichever runtime the URL names (§11 — the route tells the
    /// shell which runtime should be active; the manager starts it).
    pub fn boot(state: ShellState) {
        let path = web_sys::window()
            .map(|w| w.location().pathname().unwrap_or_default())
            .unwrap_or_default();
        let launch = crate::services::launch_from_url();
        if path == "/reader" && launch.path.is_empty() {
            // A /reader URL with no launch data cannot open a document: the
            // same bounce the unified app's RouteSync had.
            navigate("/");
            state.manager.start_library(&state);
            return;
        }
        if path == "/reader" || !launch.path.is_empty() {
            if !launch.path.is_empty() {
                navigate("/reader");
            }
            state.manager.start_reader(&state, launch);
        } else {
            state.manager.start_library(&state);
        }
    }

    /// Start (or replace with) the reader runtime. Any active runtime is
    /// disposed and AWAITED first.
    pub fn start_reader(&self, state: &ShellState, launch: LaunchDocument) {
        let state = state.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let manager = &state.manager;
            manager.dispose_active().await;
            *manager.slot.lock().unwrap() = Slot::Starting;
            clear_host(manager.host());
            let module = load_module("reader").await;
            let start = js_sys::Reflect::get(
                &module,
                &wasm_bindgen::JsValue::from_str("mareaderReaderStart"),
            )
            .expect("reader start export");
            let start: js_sys::Function = start.unchecked_into();
            let launch_json = serde_json::to_string(&launch).expect("launch json");
            let id = start
                .call2(
                    &module,
                    &manager.host().into(),
                    &wasm_bindgen::JsValue::from_str(&launch_json),
                )
                .expect("reader start")
                .as_f64()
                .expect("session id") as u32;
            manager
                .reader_sessions_created
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            *manager.slot.lock().unwrap() = Slot::Reader { id, module };
        });
    }

    /// Start (or replace with) the library runtime.
    pub fn start_library(&self, state: &ShellState) {
        let state = state.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let manager = &state.manager;
            manager.dispose_active().await;
            *manager.slot.lock().unwrap() = Slot::Starting;
            clear_host(manager.host());
            let module = load_module("library").await;
            let start = js_sys::Reflect::get(
                &module,
                &wasm_bindgen::JsValue::from_str("mareaderLibraryStart"),
            )
            .expect("library start export");
            let start: js_sys::Function = start.unchecked_into();
            let id = start
                .call1(&module, &manager.host().into())
                .expect("library start")
                .as_f64()
                .expect("session id") as u32;
            manager
                .library_sessions_created
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            *manager.slot.lock().unwrap() = Slot::Library { id, module };
        });
    }

    async fn dispose_active(&self) {
        let live = match &*self.slot.lock().unwrap() {
            Slot::Library { id, module } => Some(("mareaderLibraryDispose", *id, module.clone())),
            Slot::Reader { id, module } => Some(("mareaderReaderDispose", *id, module.clone())),
            _ => None,
        };
        if let Some((dispose_name, id, module)) = live {
            let dispose =
                js_sys::Reflect::get(&module, &wasm_bindgen::JsValue::from_str(dispose_name))
                    .expect("dispose export");
            let dispose: js_sys::Function = dispose.unchecked_into();
            let promise = dispose
                .call1(&module, &wasm_bindgen::JsValue::from_f64(id as f64))
                .expect("dispose call");
            let promise: js_sys::Promise = promise.unchecked_into();
            let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
            if dispose_name == "mareaderReaderDispose" {
                self.reader_disposes_completed
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            *self.slot.lock().unwrap() = Slot::None;
        }
    }

    /// A library open command: navigate + start the reader (§13's sequence —
    /// the shell captures the minimal launch data and starts the runtime).
    pub fn open_document(&self, state: &ShellState, launch: LaunchDocument) {
        navigate("/reader");
        self.start_reader(state, launch);
    }

    /// A reader handback: dispose the reader, then the library is active.
    pub fn navigate_library(&self, state: &ShellState) {
        navigate("/");
        self.start_library(state);
    }
}

impl Default for RuntimeManager {
    fn default() -> Self {
        Self::new()
    }
}

fn clear_host(host: web_sys::Element) {
    // Remove the outgoing runtime's DOM before the next mounts (§11 — never
    // mounted underneath).
    while let Some(child) = host.first_child() {
        let _ = host.remove_child(&child);
    }
}

/// History-API navigation: two paths, `/` and `/reader` (§11). popstate is
/// wired in the shell root once.
pub fn navigate(path: &str) {
    if let Some(window) = web_sys::window() {
        let _ = window.history().map(|h| {
            let _ = h.push_state_with_url(&wasm_bindgen::JsValue::NULL, "", Some(path));
        });
    }
}

/// One dynamic import of a runtime artifact, initialized. The loader caches
/// the module namespace per artifact (compiled code may be cached); the LIVE
/// session is what each start call creates — the module's wasm INSTANCE is
/// initialized here, once per import, and the sessions above it come and go.
async fn load_module(name: &str) -> js_sys::Object {
    let cached = match name {
        "library" => LIBRARY_MODULE.with(|m| m.borrow().clone()),
        _ => None,
    };
    if let Some(module) = cached {
        return module;
    }
    let module = crate::app::loader::dyn_import(&format!("/{name}.js"));
    let module = wasm_bindgen_futures::JsFuture::from(module)
        .await
        .expect("runtime module loads");
    let module: js_sys::Object = module.unchecked_into();
    // A dynamically imported artifact does not initialize itself: its
    // `default` export is the wasm-bindgen init (the same call the artifact's
    // own page makes), and the `*Start` exports below are only callable once
    // it has resolved.
    let key = wasm_bindgen::JsValue::from_str("default");
    if let Ok(init) = js_sys::Reflect::get(&module, &key)
        && init.is_function()
    {
        let init: js_sys::Function = init.unchecked_into();
        let promise = init.call0(&module).expect("runtime module init");
        let promise: js_sys::Promise = promise.unchecked_into();
        let done = wasm_bindgen_futures::JsFuture::from(promise);
        done.await.expect("runtime wasm instance initializes");
    }
    if name == "library" {
        LIBRARY_MODULE.with(|m| *m.borrow_mut() = Some(module.clone()));
    }
    module
}
