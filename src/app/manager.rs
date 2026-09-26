//! The runtime manager: the one navigation authority (§10), the owner of
//! "which runtime is active", and the Shell minder of the frame lifecycle —
//! insertion, handshake, ready-timeout, two-phase disposal (§12). A runtime
//! is a frame ([`crate::app::frame::Driver`]), never a module the shell
//! executes; what the manager serializes is the sequence around the frames:
//! never two at once, never a start before the outgoing disposal resolved.
//!
//! Every step can still fail — the artifact page does not load, its wasm
//! rejects, the frame simply never says `Ready` — and a failed start is a
//! VISIBLE named state, never a window on its last painted frame. So each
//! stage has a bound (§6, §11), the host paints its loading state before any
//! await, and a failure paints the error state with runtime / stage / cause.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Mutex;

use runtime_contract::boundary::LaunchDocument;
use wasm_bindgen::JsValue;

use crate::app::boot::{self, BootError, BootPhase, BootStage, RuntimeName};
use crate::app::frame::{
    Driver, FrameEvent, FrameKind, FrameVocabulary, heard_summary, protocol_boot_stage,
    protocol_stage_label,
};
use crate::state::{ActiveRuntime, ShellState};

/// The active-runtime slot. `Starting` holds the in-flight disposal/load so a
/// second navigation cannot start a second runtime mid-transition. The slot
/// keeps the frame's GENERATION, not the driver itself: `Rc`/DOM plumbing can
/// never live in this `Arc`'d type (the shell state goes through Leptos
/// context, which demands `Send + Sync`), and the generation is all the
/// security property needs anyway — the whole frame security model IS the
/// generation (§8).
#[derive(Debug)]
pub enum Slot {
    None,
    Starting,
    Library { generation: u64 },
    Reader { generation: u64 },
}

impl RuntimeManager {
    /// The live driver, resolved from the slot's generation on the page
    /// thread (None while `Starting` or after teardown).
    fn live_driver(&self) -> Option<Rc<Driver>> {
        let generation = match &*self.slot.lock().unwrap() {
            Slot::Library { generation } | Slot::Reader { generation } => Some(*generation),
            Slot::None | Slot::Starting => None,
        }?;
        crate::app::frame::lookup(generation)
    }
}

pub struct RuntimeManager {
    slot: Mutex<Slot>,
    /// What the runtime host is showing (§6, §11), in the plain form the
    /// diagnostics probe reads. The pair (state + error) is always written
    /// together so a probe can never read one side stale.
    pub boot_state: Mutex<String>,
    pub boot_error: Mutex<Option<serde_json::Value>>,
    /// Create/dispose counts for the diagnostics identity (§21): a reader →
    /// library transition must leave active reader = none, library = one.
    pub reader_sessions_created: std::sync::atomic::AtomicU64,
    pub reader_disposes_completed: std::sync::atomic::AtomicU64,
    pub library_sessions_created: std::sync::atomic::AtomicU64,
    pub library_disposes_completed: std::sync::atomic::AtomicU64,
    host: Mutex<Option<web_sys::Element>>,
    /// Starts are serialized (§10): a navigation mid-start becomes the
    /// pending request, newest intent wins.
    starting: std::sync::atomic::AtomicBool,
    pending: Mutex<Option<(RuntimeName, Option<LaunchDocument>)>>,
    /// The last reader digest, cached for the probe.
    pub last_digest: Mutex<Option<serde_json::Value>>,
    pub doc_status: Mutex<String>,
    pub doc_error: Mutex<Option<String>>,
    /// The frame generations ever seen sending a message for a generation
    /// that is not the frame they belong to (§35): a ledger, not a gate —
    /// the generation check is the gate.
    pub stale_frames_seen: std::sync::atomic::AtomicU64,
}

thread_local! {
    /// The shell state, attached once the Shell component exists. NOT a
    /// manager field: a Leptos `RwSignal` shell must stay out of any type
    /// that crosses a `Sync` boundary (`provide_context` requires it), and
    /// there is exactly one thread this field ever runs on — the frame
    /// driver's closures arrive from the same event loop the manager awaits
    /// on — so the module thread-local is the honest wiring.
    static SHELL_STATE: RefCell<Option<ShellState>> = const { RefCell::new(None) };
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
            stale_frames_seen: Default::default(),
        }
    }

    /// Attach the shell state (called once from the Shell component, before
    /// the first boot). The manager stores it because frame events arrive
    /// from drivers the manager created — the state handle the bridge
    /// closures capture is exactly the handle the frame path needs.
    pub fn attach_state(&self, state: ShellState) {
        SHELL_STATE.with(|slot| *slot.borrow_mut() = Some(state));
    }

    /// Publish a boot phase: the diagnostics probe's copy, and the native
    /// host's boot report, written as one pair (state + error).
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

    /// Boot: whichever runtime the URL names (§11).
    pub fn boot(state: ShellState) {
        let path = web_sys::window()
            .map(|w| w.location().pathname().unwrap_or_default())
            .unwrap_or_default();
        let launch = crate::services::launch_from_url();
        if path == "/reader" && launch.path.is_empty() {
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

    async fn start(&self, runtime: RuntimeName, launch: Option<LaunchDocument>) {
        if let Err(error) = self.run_start(runtime, launch).await {
            self.fail(error);
        }
    }

    /// One start at a time — the queue discipline is unchanged from the
    /// module-loader era, and it means exactly what it meant then with a
    /// heavier boundary: two starts never interleave their awaits into the
    /// same host.
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
        let host = match self.host() {
            Some(host) => host,
            None => {
                let message = "the runtime host element is not mounted";
                return Err(BootError::new(runtime, BootStage::Start, message));
            }
        };
        // §11: covered BEFORE the first await.
        boot::paint_loading(&host, runtime);
        self.set_phase(BootPhase::Loading(runtime));
        // §5/§10: the outgoing frame is gone (and acknowledged) before the
        // replacement exists.
        self.dispose_active().await;
        *self.slot.lock().unwrap() = Slot::Starting;
        clear_host(&host);
        boot::paint_loading(&host, runtime);

        let generation = crate::app::frame::next_generation();
        let kind = match runtime {
            RuntimeName::Library => FrameKind::Library,
            RuntimeName::Reader => FrameKind::Reader,
        };
        let Some(driver) = Driver::new(kind, &host, generation) else {
            let message = format!("the {} frame element could not be created", runtime.label());
            return Err(BootError::new(runtime, BootStage::Start, message));
        };
        let manager_events = self.events_hook();
        driver.start(launch.clone(), manager_events);
        let _ = wasm_bindgen_futures::JsFuture::from(driver.wait_ready()).await;
        let outcome = driver.take_ready_outcome();
        match outcome {
            Some(Ok(())) => {}
            Some(Err(stage)) => {
                let cause = format!(
                    "{} — {}",
                    crate::app::frame::fatal_cause(&driver, stage),
                    heard_summary(&driver)
                );
                driver.teardown();
                return Err(BootError::new(runtime, stage.boot_stage(), cause));
            }
            None => {
                // The driver tore itself down before answering: impossible by
                // construction (its gates are never dropped without resolve),
                // but a blank outcome is never a boot.
                driver.teardown();
                let message = "the frame's boot gate closed without a verdict";
                return Err(BootError::new(runtime, BootStage::Start, message));
            }
        }
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
        crate::app::frame::register(driver.clone());
        *self.slot.lock().unwrap() = match runtime {
            RuntimeName::Reader => Slot::Reader {
                generation: driver.generation(),
            },
            RuntimeName::Library => Slot::Library {
                generation: driver.generation(),
            },
        };
        // Active is NOT published here. The frame answered `Ready` (the
        // runtime is mounted), but the loading cover is still down until
        // `Painted` — reporting Active now is what made the terminal say
        // `boot: library` while the user still stared at a loading screen.
        // The Painted handler below sets the host active and publishes the
        // phase, so the line lands exactly when the cover lifts.
        Ok(())
    }

    /// The dispatch closure every driver reports through. Everything here is
    /// generation-checked against the CURRENT slot: a stale frame cannot
    /// raise its own events into the live state (§35), and the stale ledger
    /// counts what was dropped. The hook runs on wasm's single thread, so
    /// the slot lock is never contended here.
    fn events_hook(&self) -> crate::app::frame::FrameEventHook {
        let state = SHELL_STATE.with(|slot| slot.borrow().clone());
        let Some(state) = state else {
            return Rc::new(|_generation, _event| {});
        };
        Rc::new(move |generation, event| {
            let manager = state.manager.clone();
            match event {
                FrameEvent::Contact | FrameEvent::Ready => {}
                FrameEvent::Stage(stage) => {
                    // Frame-side telemetry: every handshake stage the runtime
                    // reports lands in the console, so a boot that stalls in
                    // the field names where it stopped (§9's stage trail).
                    web_sys::console::debug_1(&JsValue::from_str(&format!(
                        "[frame {generation}] stage {stage:?}"
                    )));
                }
                FrameEvent::Painted => {
                    let current = manager.live_driver().map(|driver| driver.generation());
                    if current != Some(generation) {
                        return;
                    }
                    if let Some(host) = manager.host() {
                        boot::clear_loading(&host);
                    }
                    boot::uncover_page();
                    // The cover is up and the runtime's own DOM is on screen:
                    // only NOW is `boot: <runtime>` an honest line. (The
                    // driver's Painted grace reports the same event if the
                    // frame never announces it, so this always lands.)
                    if let (Some(host), Some(active)) = (manager.host(), manager.active()) {
                        let runtime = match active {
                            ActiveRuntime::Library => RuntimeName::Library,
                            ActiveRuntime::Reader => RuntimeName::Reader,
                        };
                        boot::set_active(&host, runtime);
                        manager.set_phase(BootPhase::Active(runtime));
                    }
                }
                FrameEvent::Failed { stage, cause } => {
                    // A live runtime turned on its own failure (§11): the
                    // error state is the outcome — the frame is taken down,
                    // nothing half-mounted survives, and the phase names it.
                    let Some(driver) = manager.live_driver() else {
                        return;
                    };
                    if driver.generation() != generation {
                        manager
                            .stale_frames_seen
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        return;
                    }
                    let runtime: RuntimeName = driver.kind().into();
                    driver.teardown();
                    *manager.slot.lock().unwrap() = Slot::None;
                    let label = protocol_stage_label(stage);
                    let message = format!("{cause} (protocol stage {label})");
                    let error = BootError::new(runtime, protocol_boot_stage(stage), message);
                    // The page placeholder still covers the window until the
                    // first paint: a failure before that paint must step it
                    // aside too, or the error card lands behind the one thing
                    // the user is still looking at.
                    boot::uncover_page();
                    if let Some(host) = manager.host() {
                        clear_host(&host);
                        boot::paint_error(&host, &error);
                    }
                    manager.set_phase(BootPhase::Failed(error));
                }
                FrameEvent::DisposeComplete => {
                    // The driver that awaited it resolves its own gate; the
                    // event is the frame's bookkeeping signal.
                }
                FrameEvent::Boundary(vocabulary) => {
                    let state = state.clone();
                    manager.dispatch_boundary(&state, generation, vocabulary);
                }
                FrameEvent::Stale => {
                    manager
                        .stale_frames_seen
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
            }
        })
    }

    /// A failed start: the host paints the error state and the console keeps
    /// the detail (§6).
    fn fail(&self, error: BootError) {
        boot::uncover_page();
        match self.host() {
            Some(host) => {
                clear_host(&host);
                boot::paint_error(&host, &error);
            }
            None => web_sys::console::error_1(&JsValue::from_str(&error.console_line())),
        }
        self.set_phase(BootPhase::Failed(error));
    }

    /// Dispose the live frame and AWAIT it (§5/§10, §12): graceful first
    /// (`DisposeComplete`), forced removal after the strict timeout — never
    /// silent. Both outcomes COMPLETE the exchange (the forced one simply
    /// names itself), so the caller has no error to fold back into a boot.
    async fn dispose_active(&self) {
        let runtime = match self.active() {
            Some(ActiveRuntime::Library) => RuntimeName::Library,
            Some(ActiveRuntime::Reader) => RuntimeName::Reader,
            None => return,
        };
        let Some(driver) = self.live_driver() else {
            *self.slot.lock().unwrap() = Slot::None;
            return;
        };
        let promise = driver.grace_dispose();
        let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
        match driver.take_dispose_outcome() {
            Some(Ok(())) | None => {
                driver.teardown();
            }
            Some(Err(_stage)) => {
                // §12's forced path: the removal proceeds, the fact lands in
                // the digest, and the shell keeps moving — a hung disposal
                // must not hold the next runtime hostage.
                let heard = heard_summary(&driver);
                web_sys::console::warn_1(&JsValue::from_str(&format!(
                    "[mareader] forced frame removal for {runtime:?} ({heard})"
                )));
                driver.teardown();
            }
        }
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
    }

    /// A library open command: navigate + start the reader (§13's sequence).
    pub fn open_document(&self, state: &ShellState, launch: LaunchDocument) {
        // A launch with no path would mount a reader that can never show a
        // document — the bare "No document" shell with a blank viewer. Refuse
        // it here, where the launch arrives, and say so in the console
        // instead of failing silently somewhere down the pipeline.
        if launch.path.is_empty() {
            web_sys::console::error_1(&JsValue::from_str(
                "[shell] open-document refused: the launch carries no path",
            ));
            return;
        }
        web_sys::console::log_1(&JsValue::from_str(&format!(
            "[shell] open-document: {}",
            launch.path
        )));
        navigate("/reader");
        self.start_reader(state, launch);
    }

    /// A reader handback: dispose the reader, then the library is active.
    pub fn navigate_library(&self, state: &ShellState) {
        navigate("/");
        self.start_library(state);
    }

    /// The frame-dispatched boundary vocabulary. Called by the driver's
    /// event hook; the hook itself is generation-gated at the port.
    pub fn dispatch_boundary(&self, state: &ShellState, generation: u64, item: FrameVocabulary) {
        let current = self.live_driver().map(|driver| driver.generation());
        if current != Some(generation) {
            self.stale_frames_seen
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return;
        }
        match item {
            FrameVocabulary::OpenDocument(launch) => {
                self.open_document(state, *launch);
            }
            FrameVocabulary::NavigateLibrary => {
                self.navigate_library(state);
            }
            FrameVocabulary::ReadPoint(point) => crate::services::apply_read_point(&point),
            FrameVocabulary::SaveSettings(settings) => {
                crate::services::save_settings(&settings);
            }
            FrameVocabulary::SaveLibrary(blob) => {
                crate::services::save_library(&blob);
            }
            FrameVocabulary::SaveCovers(covers) => {
                let _ = storage::save_covers(&covers);
            }
            FrameVocabulary::SaveCover { path, image } => {
                crate::services::save_cover(&path, image);
            }
            FrameVocabulary::BakeCover { path } => {
                self.bake_for_library(state, generation, path);
            }
            FrameVocabulary::DocStatus(report) => {
                if report.status != "Ready" {
                    let live = self.active() == Some(ActiveRuntime::Reader);
                    if live && report.status == "Idle" {
                        self.navigate_library(state);
                    }
                }
                // The document's truth, printed the moment it changes: the
                // terminal line `doc: Ready` means the file was read and the
                // viewer is up (and `doc: Error — …` carries the reason).
                // This is the line the native smoke waits for after handing
                // a file to the app — a booted Reader that never reads
                // cannot produce it.
                let previous = {
                    let mut status = self.doc_status.lock().unwrap();
                    let changed = *status != report.status;
                    *status = report.status.clone();
                    changed
                };
                let error_changed = *self.doc_error.lock().unwrap() != report.error;
                *self.doc_error.lock().unwrap() = report.error.clone();
                if previous || error_changed {
                    let detail = report.error.as_deref().unwrap_or("");
                    let line = if detail.is_empty() {
                        format!("doc: {}", report.status)
                    } else {
                        format!("doc: {} — {}", report.status, detail)
                    };
                    report_line(&line);
                }
            }
            FrameVocabulary::PublishDigest(json) => {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&json) {
                    *self.last_digest.lock().unwrap() = Some(value);
                }
            }
            FrameVocabulary::Reload => {
                crate::diagnostics::log_reload_heap();
                app_chrome::window::api::reload_window();
            }
            FrameVocabulary::ResolveLaunch { request, path } => {
                let document = crate::services::resolve_launch(&path).map(Box::new);
                if let Some(driver) = self.live_driver() {
                    driver.send(
                        &runtime_contract::protocol::ShellFrame::ResolveLaunchAnswer {
                            request,
                            document,
                        },
                    );
                }
            }
        }
    }

    /// A cover bake the library frame asked for: the Shell bakes with its own
    /// engine (the frame never gets one) and answers over the ASKING frame's
    /// lane — generation-stamped, so a bake that outlived its frame dies at
    /// the boundary (§35).
    fn bake_for_library(&self, state: &ShellState, generation: u64, path: String) {
        let manager = state.manager.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let width = runtime_contract::covers::COVER_WIDTH;
            let image = pdf_engine::api::cover_data_url(&path, width)
                .await
                .ok()
                .map(|cover| runtime_contract::covers::CoverImage {
                    data_url: cover.data_url,
                    width: cover.width,
                    height: cover.height,
                });
            let Some(driver) = manager.live_driver() else {
                return;
            };
            if driver.generation() != generation || driver.kind() != FrameKind::Library {
                return;
            }
            driver.send(&runtime_contract::protocol::ShellFrame::CoverBaked { path, image });
        });
    }
}

impl Default for RuntimeManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Tell the native host which runtime is live, or where the boot stopped.
fn report_boot(phase: &BootPhase) {
    let report = match phase {
        BootPhase::Failed(error) => {
            let runtime = error.runtime.artifact();
            format!("failed {runtime} {}", error.stage.slug())
        }
        other => other.as_str().to_string(),
    };
    report_line(&format!("boot: {report}"));
}

/// One honest line to the native host's terminal (`[mareader] <line>`).
/// Boot phases prefix themselves with `boot: `; document truth rides as
/// `doc: …`. Gated on the IPC being real — a plain browser has no host.
fn report_line(line: &str) {
    if !tauri_bridge::has_tauri() {
        return;
    }
    let args: JsValue = js_sys::Object::new().into();
    let key = JsValue::from_str("report");
    if js_sys::Reflect::set(&args, &key, &JsValue::from_str(line)).is_err() {
        return;
    }
    wasm_bindgen_futures::spawn_local(async move {
        let _ = tauri_bridge::invoke("boot_report", args).await;
    });
}

fn clear_host(host: &web_sys::Element) {
    // Remove the outgoing frame's element and the shell's previous boot
    // markup before the next frames (§11 — never mounted underneath). The
    // frame's element carries `data-mareader-runtime-frame`; the boot markup
    // carries its own attr, and both are handled additively here because a
    // disposed frame is already gone.
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
