//! The runtime manager: the one navigation authority (§10) and the owner of
//! "which runtime is active". Its type makes two primary runtimes
//! unspeakable, and it never starts a runtime before the previous one's
//! dispose promise resolved (§5).
//!
//! Every step of a start can fail — the artifact is missing, its wasm rejects,
//! the start export is gone — and a failed start is a VISIBLE state, never a
//! panic. A panic inside the shell wasm takes the whole shell with it, leaving
//! the window on its last painted frame: that is how a missing `/library.js`
//! became a blank window with a silent terminal. So the loader, the init and
//! the start export each report the stage they failed in (§6), the host paints
//! a loading state before the first await and an error state after a failure
//! (§11), and the page's own placeholder steps aside once the host paints.

use std::cell::RefCell;
use std::sync::Mutex;

use runtime_contract::boundary::LaunchDocument;
use wasm_bindgen::{JsCast, JsValue};

use crate::app::boot::{self, BootError, BootPhase, BootStage, RuntimeName};
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
    /// What the runtime host is showing (§6, §11), in the plain form the
    /// diagnostics probe reads: the probe runs from JS, outside any reactive
    /// context, so this is a value and not a signal — the same shape as
    /// `doc_status`/`doc_error` beside it. The host's own DOM is the other half
    /// of the same fact, and the coverage watch in `boot.rs` reads THAT rather
    /// than any shell-side copy of it.
    pub boot_state: Mutex<String>,
    pub boot_error: Mutex<Option<serde_json::Value>>,
    /// Create/dispose counts for the diagnostics identity (§21): a reader →
    /// library transition must leave active reader = none, library = one.
    pub reader_sessions_created: std::sync::atomic::AtomicU64,
    pub reader_disposes_completed: std::sync::atomic::AtomicU64,
    pub library_sessions_created: std::sync::atomic::AtomicU64,
    /// Library disposals that COMPLETED (the dispose promise resolved). The
    /// pair with `reader_disposes_completed` is what proves the handoff order
    /// in either direction: a replacement becomes active only after the
    /// outgoing runtime's count has moved.
    pub library_disposes_completed: std::sync::atomic::AtomicU64,
    host: Mutex<Option<web_sys::Element>>,
    /// Starts are serialized. A navigation that lands while a start is still
    /// loading becomes the PENDING request instead of a second start running
    /// beside the first: two starts interleaving their awaits could both mount
    /// into the host, and one runtime at a time is the invariant the host is
    /// built around (§10, §11). Newest request wins — the user's last intent.
    starting: std::sync::atomic::AtomicBool,
    pending: Mutex<Option<(RuntimeName, Option<LaunchDocument>)>>,
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
            boot_state: Mutex::new(BootPhase::Booting.as_str().to_string()),
            boot_error: Mutex::new(None),
            reader_sessions_created: Default::default(),
            reader_disposes_completed: Default::default(),
            library_sessions_created: Default::default(),
            library_disposes_completed: Default::default(),
            host: Mutex::new(None),
            starting: std::sync::atomic::AtomicBool::new(false),
            pending: Mutex::new(None),
            last_digest: Mutex::new(None),
            doc_status: Mutex::new("Idle".to_string()),
            doc_error: Mutex::new(None),
        }
    }

    /// Publish a boot phase: the diagnostics probe's copy, and the native
    /// host's boot report. The phase is written as one pair (state + error) so
    /// a probe can never read a failure's state beside the previous runtime's
    /// error, or the reverse.
    fn set_phase(&self, phase: BootPhase) {
        *self.boot_error.lock().unwrap() = phase.error().map(BootError::to_json);
        *self.boot_state.lock().unwrap() = phase.as_str().to_string();
        report_boot(&phase);
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

    /// The mount target, if the shell has mounted its view.
    fn host(&self) -> Option<web_sys::Element> {
        self.host.lock().unwrap().clone()
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
        let manager = state.manager.clone();
        wasm_bindgen_futures::spawn_local(async move {
            manager
                .start_serialized(RuntimeName::Reader, Some(launch))
                .await;
        });
    }

    /// Start (or replace with) the library runtime.
    pub fn start_library(&self, state: &ShellState) {
        let manager = state.manager.clone();
        wasm_bindgen_futures::spawn_local(async move {
            manager.start_serialized(RuntimeName::Library, None).await;
        });
    }

    /// The one start sequence. Its failure path is closed: `run_start` either
    /// leaves a live session in the slot or an error state in the host.
    async fn start(&self, runtime: RuntimeName, launch: Option<LaunchDocument>) {
        if let Err(error) = self.run_start(runtime, launch).await {
            self.fail(error);
        }
    }

    /// One start at a time. A second request arriving mid-start is queued and
    /// run when the current one settles, so two starts cannot interleave their
    /// awaits into the same host. The flag and the queue are checked without
    /// an await between them, which on wasm's single-threaded executor is what
    /// makes the window between "queue is empty" and "flag cleared" empty too.
    async fn start_serialized(&self, runtime: RuntimeName, launch: Option<LaunchDocument>) {
        if self
            .starting
            .swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            *self.pending.lock().unwrap() = Some((runtime, launch));
            return;
        }
        self.start(runtime, launch).await;
        loop {
            let queued = self.pending.lock().unwrap().take();
            match queued {
                Some((runtime, launch)) => self.start(runtime, launch).await,
                None => {
                    self.starting
                        .store(false, std::sync::atomic::Ordering::SeqCst);
                    // A request that landed after the take but before the flag
                    // cleared saw the flag SET, so it queued instead of opening
                    // its own loop: pick it up here rather than stranding it.
                    if self.pending.lock().unwrap().is_none() {
                        return;
                    }
                    self.starting
                        .store(true, std::sync::atomic::Ordering::SeqCst);
                }
            }
        }
    }

    async fn run_start(
        &self,
        runtime: RuntimeName,
        launch: Option<LaunchDocument>,
    ) -> Result<(), BootError> {
        // A watch from the runtime being replaced is stale now: it must not
        // take the loading state away from this start's host (§11).
        boot::stop_watch();
        let host = match self.host() {
            Some(host) => host,
            None => {
                let message = "the runtime host element is not mounted";
                return Err(BootError::new(runtime, BootStage::Start, message));
            }
        };
        // §11: covered BEFORE the first await. The card is additive (it
        // removes the shell's own boot nodes and leaves a mounted runtime's
        // DOM alone), so the outgoing session stays visible underneath it until
        // its disposal takes it away — and the host is never bare if that
        // disposal is the only thing on screen.
        boot::paint_loading(&host, runtime);
        self.set_phase(BootPhase::Loading(runtime));
        // §5/§10: the outgoing session is gone before the replacement exists.
        self.dispose_active().await?;
        *self.slot.lock().unwrap() = Slot::Starting;
        // The disposal may have removed the last thing in the host (its own
        // DOM): re-paint in the same synchronous step, so nothing is ever
        // uncovered between the teardown and the load.
        clear_host(&host);
        boot::paint_loading(&host, runtime);
        let module = self.load_module(runtime).await?;
        let start = start_export(&module, runtime)?;
        let value = call_start(&start, &module, &host, runtime, launch)?;
        let Some(id) = value.as_f64() else {
            let name = runtime.artifact();
            let message = format!("the {name} start export returned no session id");
            return Err(BootError::new(runtime, BootStage::Start, message));
        };
        let id = id as u32;
        match runtime {
            RuntimeName::Reader => {
                self.reader_sessions_created
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            RuntimeName::Library => {
                self.library_sessions_created
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
        *self.slot.lock().unwrap() = match runtime {
            RuntimeName::Reader => Slot::Reader { id, module },
            RuntimeName::Library => Slot::Library { id, module },
        };
        // The runtime's session exists; its DOM may not be there yet (the
        // reader's first render is a suspense anchor), so the loading state
        // stays until it has painted, and the page placeholder goes with it.
        boot::mark_active(&host, runtime);
        self.set_phase(BootPhase::Active(runtime));
        Ok(())
    }

    /// A failed start: the host paints the error state and the console keeps
    /// the detail (§6). Nothing half-mounted survives it, and the slot stays
    /// `Starting` — there is no live session to dispose later.
    fn fail(&self, error: BootError) {
        // The error state IS the outcome: nothing may cover it, and no coverage
        // watch from the failed start may remove it.
        boot::stop_watch();
        boot::uncover_page();
        match self.host() {
            Some(host) => {
                clear_host(&host);
                boot::paint_error(&host, &error);
            }
            // Without a host there is nothing to paint into; the console line
            // is then the only place the failure can show (the phase signal
            // still carries it for the diagnostics probe).
            None => web_sys::console::error_1(&JsValue::from_str(&error.console_line())),
        }
        self.set_phase(BootPhase::Failed(error));
    }

    /// Dispose the live runtime and AWAIT it (§5/§10): the replacement must not
    /// become active while the outgoing session is still tearing down. A
    /// dispose that cannot run is a failure, not something to continue past —
    /// continuing would put two live sessions in one host (§10).
    async fn dispose_active(&self) -> Result<(), BootError> {
        let live = match &*self.slot.lock().unwrap() {
            Slot::Library { id, module } => Some((RuntimeName::Library, *id, module.clone())),
            Slot::Reader { id, module } => Some((RuntimeName::Reader, *id, module.clone())),
            _ => None,
        };
        let Some((runtime, id, module)) = live else {
            return Ok(());
        };
        let export = match runtime {
            RuntimeName::Library => "mareaderLibraryDispose",
            RuntimeName::Reader => "mareaderReaderDispose",
        };
        let key = JsValue::from_str(export);
        let dispose = match js_sys::Reflect::get(&module, &key) {
            Ok(dispose) => dispose,
            Err(_) => return Err(missing_export(runtime, export)),
        };
        if !dispose.is_function() {
            return Err(missing_export(runtime, export));
        }
        let dispose: js_sys::Function = dispose.unchecked_into();
        let promise = dispose
            .call1(&module, &JsValue::from_f64(id as f64))
            .map_err(|err| BootError::new(runtime, BootStage::Dispose, js_message(&err)))?;
        let promise: js_sys::Promise = promise.unchecked_into();
        wasm_bindgen_futures::JsFuture::from(promise)
            .await
            .map_err(|err| BootError::new(runtime, BootStage::Dispose, js_message(&err)))?;
        match runtime {
            RuntimeName::Reader => {
                self.reader_disposes_completed
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            RuntimeName::Library => {
                self.library_disposes_completed
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }
        *self.slot.lock().unwrap() = Slot::None;
        Ok(())
    }

    /// One dynamic import of a runtime artifact, initialized. The loader caches
    /// the module namespace per artifact (compiled code may be cached); the
    /// LIVE session is what each start call creates — the module's wasm
    /// INSTANCE is initialized here, once per import, and the sessions above it
    /// come and go.
    async fn load_module(&self, runtime: RuntimeName) -> Result<js_sys::Object, BootError> {
        if let Some(module) = cached_module(runtime) {
            return Ok(module);
        }
        let path = format!("/{}.js", runtime.artifact());
        let promise = crate::app::loader::dyn_import(&path);
        let value = wasm_bindgen_futures::JsFuture::from(promise)
            .await
            .map_err(|err| BootError::new(runtime, BootStage::ModuleLoad, js_message(&err)))?;
        let module: js_sys::Object = value.unchecked_into();
        // A dynamically imported artifact does not initialize itself (§12): its
        // `default` export is the wasm-bindgen init, and the `*Start` exports
        // below are only callable once it has resolved. A module without it is
        // not a runtime artifact, which is a failure worth naming rather than
        // skipping past.
        let key = JsValue::from_str("default");
        let init = match js_sys::Reflect::get(&module, &key) {
            Ok(init) => init,
            Err(_) => return Err(no_init_export(runtime)),
        };
        if !init.is_function() {
            return Err(no_init_export(runtime));
        }
        let init: js_sys::Function = init.unchecked_into();
        let promise = init
            .call0(&module)
            .map_err(|err| BootError::new(runtime, BootStage::Init, js_message(&err)))?;
        let promise: js_sys::Promise = promise.unchecked_into();
        wasm_bindgen_futures::JsFuture::from(promise)
            .await
            .map_err(|err| BootError::new(runtime, BootStage::Init, js_message(&err)))?;
        cache_module(runtime, &module);
        Ok(module)
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

    /// Deliver one command JSON to the LIVE library session's command export.
    /// A delivery with no live library session is dropped: the request that
    /// produced it outlived its generation, and a replacement session never
    /// inherits a predecessor's queue traffic (§5's ordering in miniature).
    pub fn deliver_library_command(&self, json: &str) {
        let guard = self.slot.lock().unwrap_or_else(|e| e.into_inner());
        let Slot::Library { id, module } = &*guard else {
            return;
        };
        let key = JsValue::from_str("mareaderLibraryCommand");
        let Ok(cmd) = js_sys::Reflect::get(module, &key) else {
            return;
        };
        if !cmd.is_function() {
            return;
        }
        let cmd: js_sys::Function = cmd.unchecked_into();
        let _ = cmd.call2(
            module,
            &JsValue::from_f64(*id as f64),
            &JsValue::from_str(json),
        );
    }
}

impl Default for RuntimeManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Tell the native host which runtime is live, or where the boot stopped.
///
/// One line per phase transition — a boot paints three or four in a session,
/// never one per frame — and nothing at all in the web build (`has_tauri` is
/// false there). Without it, a native window that never booted its frontend
/// is indistinguishable from one that did: the exact blind spot the packaged
/// app had. tools/tauri-smoke.mjs reads these lines as its assertion.
fn report_boot(phase: &BootPhase) {
    if !tauri_bridge::has_tauri() {
        return;
    }
    let report = match phase {
        BootPhase::Failed(error) => {
            let runtime = error.runtime.artifact();
            format!("failed {runtime} {}", error.stage.slug())
        }
        other => other.as_str().to_string(),
    };
    let args: JsValue = js_sys::Object::new().into();
    let key = JsValue::from_str("report");
    if js_sys::Reflect::set(&args, &key, &JsValue::from_str(&report)).is_err() {
        return;
    }
    wasm_bindgen_futures::spawn_local(async move {
        let _ = tauri_bridge::invoke("boot_report", args).await;
    });
}

fn missing_export(runtime: RuntimeName, export: &str) -> BootError {
    let name = runtime.artifact();
    let message = format!("the {name} module does not export {export}()");
    BootError::new(runtime, BootStage::Dispose, message)
}

fn no_init_export(runtime: RuntimeName) -> BootError {
    let name = runtime.artifact();
    let message = format!("the {name} module exports no default wasm init");
    BootError::new(runtime, BootStage::Init, message)
}

/// The `*Start` export, or the failure that says which one is missing.
fn start_export(
    module: &js_sys::Object,
    runtime: RuntimeName,
) -> Result<js_sys::Function, BootError> {
    let export = match runtime {
        RuntimeName::Library => "mareaderLibraryStart",
        RuntimeName::Reader => "mareaderReaderStart",
    };
    let key = JsValue::from_str(export);
    let start = match js_sys::Reflect::get(module, &key) {
        Ok(start) => start,
        Err(_) => return Err(missing_start(runtime, export)),
    };
    if !start.is_function() {
        return Err(missing_start(runtime, export));
    }
    Ok(start.unchecked_into())
}

fn missing_start(runtime: RuntimeName, export: &str) -> BootError {
    let name = runtime.artifact();
    let message = format!("the {name} module does not export {export}()");
    BootError::new(runtime, BootStage::Start, message)
}

/// Call the runtime's start export. The reader takes the launch payload as a
/// second argument; the library takes the host alone.
fn call_start(
    start: &js_sys::Function,
    module: &js_sys::Object,
    host: &web_sys::Element,
    runtime: RuntimeName,
    launch: Option<LaunchDocument>,
) -> Result<JsValue, BootError> {
    let host: JsValue = host.clone().into();
    let result = match launch {
        Some(launch) => {
            let json = serde_json::to_string(&launch)
                .map_err(|err| BootError::new(runtime, BootStage::Start, err.to_string()))?;
            start.call2(module, &host, &JsValue::from_str(&json))
        }
        None => start.call1(module, &host),
    };
    result.map_err(|err| BootError::new(runtime, BootStage::Start, js_message(&err)))
}

/// The browser's reason, in words. A rejected `import()` is an Error whose
/// `message` names the URL; a thrown panic value is usually a string.
fn js_message(value: &JsValue) -> String {
    let message = js_sys::Reflect::get(value, &JsValue::from_str("message"))
        .ok()
        .and_then(|message| message.as_string());
    if let Some(message) = message {
        return message;
    }
    if let Some(text) = value.as_string() {
        return text;
    }
    js_sys::JSON::stringify(value)
        .ok()
        .and_then(|text| text.as_string())
        .unwrap_or_else(|| format!("{value:?}"))
}

fn cached_module(runtime: RuntimeName) -> Option<js_sys::Object> {
    match runtime {
        RuntimeName::Library => LIBRARY_MODULE.with(|module| module.borrow().clone()),
        RuntimeName::Reader => None,
    }
}

fn cache_module(runtime: RuntimeName, module: &js_sys::Object) {
    if runtime == RuntimeName::Library {
        LIBRARY_MODULE.with(|cache| *cache.borrow_mut() = Some(module.clone()));
    }
}

fn clear_host(host: &web_sys::Element) {
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
