//! The Shell's route/lifecycle authority. Library and Reader host are
//! disposable iframe/WASM artifacts; the Reader host owns independent
//! document realms, never document engines in the persistent Shell.
//!
//! Both route realms are disposable. Navigation creates a fresh incoming
//! frame, retains the outgoing pixels until Ready and Painted, then retires
//! and removes the outgoing realm. Neither route prewarms or recycles the
//! other behind it; only Shell and the current route survive the handoff.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Mutex;

use runtime_contract::boundary::LaunchDocument;
use runtime_contract::protocol::ShellFrame;
use wasm_bindgen::JsValue;

use crate::app::boot::{self, BootError, BootPhase, BootStage, RuntimeName};
use crate::app::frame::{
    Driver, FrameEvent, FrameFatalStage, FrameKind, FrameSlot, FrameVocabulary, heard_summary,
    protocol_boot_stage, protocol_stage_label,
};
use crate::state::{ActiveRuntime, ShellState};

/// The active-runtime slot. `Starting` holds the in-flight load so a second
/// navigation cannot start a second runtime into the same host. The slot
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

impl Slot {
    /// The generation of the runtime on screen, if there is one.
    fn generation(&self) -> Option<u64> {
        match self {
            Slot::Library { generation } | Slot::Reader { generation } => Some(*generation),
            Slot::None | Slot::Starting => None,
        }
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
    /// A frame counts as created when it answers Ready. After retirement,
    /// `created - disposed == active`; there is no retained counterpart.
    pub reader_sessions_created: std::sync::atomic::AtomicU64,
    pub reader_disposes_completed: std::sync::atomic::AtomicU64,
    pub library_sessions_created: std::sync::atomic::AtomicU64,
    pub library_disposes_completed: std::sync::atomic::AtomicU64,
    host: Mutex<Option<web_sys::Element>>,
    /// Starts are serialized (§10): a navigation mid-start becomes the
    /// pending request, newest intent wins. An incoming realm is cancelled
    /// before its successor starts; outgoing retirement runs independently.
    starting: std::sync::atomic::AtomicBool,
    pending: Mutex<Option<(RuntimeName, Option<LaunchDocument>)>>,
    /// The cold incoming frame, cancellable before Ready/Painted so a return
    /// to Library cannot leave a late Reader boot behind it.
    incoming: Mutex<Option<u64>>,
    /// The last reader digest, cached for the probe.
    pub last_digest: Mutex<Option<serde_json::Value>>,
    pub doc_status: Mutex<String>,
    pub doc_error: Mutex<Option<String>>,
    /// The frame generations ever seen sending a message for a generation
    /// that is not the frame they belong to (§35): a ledger, not a gate —
    /// the generation check is the gate.
    pub stale_frames_seen: std::sync::atomic::AtomicU64,
    /// The reader session the Shell has handed a document to. An Idle
    /// document status only means "the book is gone, go back to the shelf"
    /// for a session that actually showed one. Initial Idle may trail Opening.
    ///
    /// Two facts, deliberately separate. `reader_launched` is the session the
    /// Shell SENT a launch to; `reader_armed` is set only once that session
    /// showed the book (Ready): a boot-time Idle arriving after admission
    /// must not bounce a newly opened document back to Library.
    reader_launched: Mutex<Option<u64>>,
    reader_armed: Mutex<Option<u64>>,
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
            incoming: Mutex::new(None),
            last_digest: Mutex::new(None),
            doc_status: Mutex::new("Idle".to_string()),
            doc_error: Mutex::new(None),
            stale_frames_seen: Default::default(),
            reader_launched: Mutex::new(None),
            reader_armed: Mutex::new(None),
        }
    }

    /// Attach the shell state (called once from the Shell component, before
    /// the first boot). The manager stores it because frame events arrive
    /// from drivers the manager created — the state handle the bridge
    /// closures capture is exactly the handle the frame path needs.
    pub fn attach_state(&self, state: ShellState) {
        SHELL_STATE.with(|slot| *slot.borrow_mut() = Some(state));
    }

    /// A handle on the shared manager. Retirement outlives the
    /// call that started them, so they need an owned handle; the shell state
    /// owns the one `Arc` there is.
    fn handle(&self) -> Option<std::sync::Arc<RuntimeManager>> {
        SHELL_STATE.with(|slot| slot.borrow().as_ref().map(|state| state.manager.clone()))
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

    /// How many reader frames are in the page, whatever their slot (the
    /// probe's `readerFramesResident`). This — not the active runtime, not
    /// a lifecycle label — is the memory question: a reader frame that exists
    /// holds its realm, its wasm instance and its heap high-water mark.
    #[cfg(target_arch = "wasm32")]
    pub fn reader_frames_resident(&self) -> usize {
        crate::app::frame::resident(FrameKind::Reader)
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
                navigate_reader(&launch);
            }
            state.manager.start_reader(&state, launch);
        } else {
            state.manager.start_library(&state);
        }
    }

    /// Start (or replace with) the reader runtime.
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
            let incoming = self.incoming.lock().unwrap().take();
            if let Some(driver) = incoming.and_then(crate::app::frame::lookup) {
                driver.teardown();
            }
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

    /// An open inside Reader keeps its workspace and replaces a document
    /// realm. A route change always boots a fresh independent route frame.
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

        // Already there. A reader that is on screen takes a new document over
        // its own port: rebooting it to change books is the slowest possible
        // answer to a drop or an "open another".
        if self.active() == Some(ActiveRuntime::Reader) && runtime == RuntimeName::Reader {
            if let (Some(document), Some(driver)) = (launch, self.live_driver()) {
                driver.send(&ShellFrame::Launch {
                    document: Box::new(document),
                });
                self.note_launch(Some(driver.generation()));
            }
            // Cancelling an incoming Library can return to the Reader that
            // never stopped being visible. Restore its route phase as well.
            boot::clear_loading(&host);
            boot::set_active(&host, runtime);
            self.set_phase(BootPhase::Active(runtime));
            return Ok(());
        }
        if self.active() == Some(ActiveRuntime::Library) && runtime == RuntimeName::Library {
            boot::clear_loading(&host);
            boot::set_active(&host, runtime);
            self.set_phase(BootPhase::Active(runtime));
            return Ok(());
        }

        self.start_fresh(&host, runtime, launch).await
    }

    /// A fresh realm without sacrificing the outgoing pixels. An incoming
    /// frame is laid out but hidden until BOTH Ready and Painted arrive.
    async fn start_fresh(
        &self,
        host: &web_sys::Element,
        runtime: RuntimeName,
        launch: Option<LaunchDocument>,
    ) -> Result<(), BootError> {
        let outgoing = self.live_driver().map(|driver| driver.generation());
        self.set_phase(BootPhase::Loading(runtime));
        if outgoing.is_none() {
            *self.slot.lock().unwrap() = Slot::Starting;
            boot::paint_loading(host, runtime);
        }
        let generation = crate::app::frame::next_generation();
        let Some(driver) = Driver::new(runtime.frame_kind(), host, generation, FrameSlot::Incoming)
        else {
            let message = format!("the {} frame element could not be created", runtime.label());
            return Err(BootError::new(runtime, BootStage::Start, message));
        };
        *self.incoming.lock().unwrap() = Some(generation);
        crate::app::frame::register(driver.clone());
        driver.start(launch.clone(), self.events_hook());
        let _ = wasm_bindgen_futures::JsFuture::from(driver.wait_verdict()).await;
        if *self.incoming.lock().unwrap() != Some(generation) {
            // A newer route already removed this realm and woke its gates.
            return Ok(());
        }
        let ready = driver.ready_outcome();
        if ready == Some(Ok(())) {
            self.note_session_created(runtime);
            let _ = wasm_bindgen_futures::JsFuture::from(driver.wait_painted()).await;
        }
        if *self.incoming.lock().unwrap() != Some(generation) {
            if ready == Some(Ok(())) {
                self.note_dispose_completed(runtime);
            }
            return Ok(());
        }
        *self.incoming.lock().unwrap() = None;
        let failure = match ready {
            Some(Err(stage)) => Some(stage),
            None => Some(FrameFatalStage::ReadyTimeout),
            Some(Ok(())) => driver.paint_outcome().and_then(Result::err),
        };
        if let Some(stage) = failure {
            let cause = crate::app::frame::fatal_cause(&driver, stage).into_owned();
            driver.teardown();
            if ready == Some(Ok(())) {
                self.note_dispose_completed(runtime);
            }
            return Err(BootError::new(runtime, stage.boot_stage(), cause));
        }
        driver.reveal();
        self.replace_active(runtime, generation);
        self.note_launch(if runtime == RuntimeName::Reader {
            Some(generation)
        } else {
            None
        });
        if runtime == RuntimeName::Library {
            driver.send(&ShellFrame::Refresh);
        }
        driver.publish_boot_metadata();
        boot::clear_loading(host);
        boot::uncover_page();
        boot::set_active(host, runtime);
        self.set_phase(BootPhase::Active(runtime));
        if let Some(generation) = outgoing {
            if let Some(leaving) = crate::app::frame::lookup(generation) {
                leaving.begin_retiring();
            }
            self.retire(generation);
        }
        Ok(())
    }

    /// Put `generation` on screen and hand back the one it displaces.
    fn replace_active(&self, kind: RuntimeName, generation: u64) -> Option<u64> {
        let mut slot = self.slot.lock().unwrap();
        let outgoing = slot.generation();
        *slot = match kind {
            RuntimeName::Reader => Slot::Reader { generation },
            RuntimeName::Library => Slot::Library { generation },
        };
        outgoing
    }

    /// The dispatch closure every driver reports through. Everything here is
    /// generation-checked against the CURRENT slot: a stale frame cannot
    /// raise its own events into the live state (§35), and the stale ledger
    /// counts what was dropped. The hook runs on wasm's single thread, so the
    /// slot lock is never contended here.
    fn events_hook(&self) -> crate::app::frame::FrameEventHook {
        let state = SHELL_STATE.with(|slot| slot.borrow().clone());
        let Some(state) = state else {
            return Rc::new(|_generation, _event| {});
        };
        Rc::new(move |generation, event| {
            let manager = state.manager.clone();
            match event {
                FrameEvent::Stage(stage) => {
                    // Frame-side telemetry: every handshake stage the runtime
                    // reports lands in the console, so a boot that stalls in
                    // the field names where it stopped (§9's stage trail).
                    web_sys::console::debug_1(&JsValue::from_str(&format!(
                        "[frame {generation}] stage {stage:?}"
                    )));
                }
                FrameEvent::Failed { stage, cause } => {
                    let Some(driver) = crate::app::frame::lookup(generation) else {
                        return;
                    };
                    if driver.slot() == FrameSlot::Incoming {
                        driver.teardown();
                        return;
                    }
                    // §11: the error state is the outcome — the frame is taken
                    // down, nothing half-mounted survives, and the phase names
                    // it.
                    if manager.live_driver().as_ref().map(|d| d.generation()) != Some(generation) {
                        manager
                            .stale_frames_seen
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        return;
                    }
                    let runtime: RuntimeName = driver.kind().into();
                    driver.teardown();
                    manager.note_dispose_completed(runtime);
                    manager.note_launch(None);
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
        if let Some(driver) = self.live_driver() {
            driver.begin_retiring();
            self.retire(driver.generation());
            *self.slot.lock().unwrap() = Slot::None;
        }
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

    // -----------------------------------------------------------------
    // Retirement
    // -----------------------------------------------------------------

    /// Take the runtime the user just left off the critical path: it is
    /// already invisible, so its teardown can finish whenever it finishes.
    fn retire(&self, generation: u64) {
        let Some(manager) = self.handle() else {
            return;
        };
        wasm_bindgen_futures::spawn_local(async move {
            manager.run_retire(generation).await;
        });
    }

    /// §12, unchanged in substance — graceful first (`DisposeComplete`),
    /// forced removal after the strict timeout, never silent. Both outcomes
    /// COMPLETE the exchange (the forced one simply names itself), so there
    /// is no error to fold back into a boot: nothing is waiting on this.
    async fn run_retire(&self, generation: u64) {
        let Some(driver) = crate::app::frame::lookup(generation) else {
            return;
        };
        // Nothing is disposed while it is still on screen. A retirement
        // started by the reveal has already been hidden there; any other
        // path into here gets hidden on arrival.
        if driver.slot().is_visible() {
            driver.begin_retiring();
        }
        let runtime: RuntimeName = driver.kind().into();
        let promise = driver.grace_dispose();
        let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
        match driver.take_dispose_outcome() {
            Some(Ok(())) | None => {
                driver.teardown();
            }
            Some(Err(_stage)) => {
                let heard = heard_summary(&driver);
                web_sys::console::warn_1(&JsValue::from_str(&format!(
                    "[mareader] forced frame removal for {runtime:?} ({heard})"
                )));
                driver.teardown();
            }
        }
        self.note_dispose_completed(runtime);
        if runtime == RuntimeName::Reader
            && let Some(active) = self.live_driver()
            && active.kind() == FrameKind::Library
        {
            active.send(&ShellFrame::Refresh);
        }
    }

    fn note_session_created(&self, runtime: RuntimeName) {
        let counter = match runtime {
            RuntimeName::Reader => &self.reader_sessions_created,
            RuntimeName::Library => &self.library_sessions_created,
        };
        counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }

    fn note_dispose_completed(&self, runtime: RuntimeName) {
        let counter = match runtime {
            RuntimeName::Reader => &self.reader_disposes_completed,
            RuntimeName::Library => &self.library_disposes_completed,
        };
        counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
        navigate_reader(&launch);
        self.start_reader(state, launch);
    }

    /// Files dropped from the OS: imported by the LIBRARY when it is the
    /// runtime on screen, and nothing otherwise — a drop over the reader
    /// neither imports nor opens. Returns whether the shelf was asked.
    #[cfg(target_arch = "wasm32")]
    pub fn import_dropped(&self, paths: Vec<String>) -> bool {
        let generation = match &*self.slot.lock().unwrap() {
            Slot::Library { generation } => *generation,
            _ => return false,
        };
        let Some(driver) = crate::app::frame::lookup(generation) else {
            return false;
        };
        web_sys::console::log_1(&JsValue::from_str(&format!(
            "[shell] import-drop: {} file(s) to the library",
            paths.len()
        )));
        driver.send(&ShellFrame::ImportFiles { paths });
        true
    }

    /// Whether the library is the runtime on screen (the drop listener's
    /// admission: only then is an OS drag worth showing).
    #[cfg(target_arch = "wasm32")]
    pub fn library_on_screen(&self) -> bool {
        matches!(&*self.slot.lock().unwrap(), Slot::Library { .. })
    }

    /// A reader handback: reveal the library, retire the reader.
    pub fn navigate_library(&self, state: &ShellState) {
        navigate("/");
        self.start_library(state);
    }

    /// The frame-dispatched boundary vocabulary. Called by the driver's
    /// event hook; the hook itself is generation-gated at the port.
    pub fn dispatch_boundary(&self, state: &ShellState, generation: u64, item: FrameVocabulary) {
        // Only an actual incoming/active Library owns cover work. Retiring
        // shelves cannot queue new bakes after cancellation at the handoff.
        if let FrameVocabulary::BakeCover { path } = item {
            if crate::app::frame::lookup(generation).is_some_and(|driver| {
                driver.kind() == FrameKind::Library && driver.slot() != FrameSlot::Retiring
            }) {
                crate::app::bake::request(generation, path);
            }
            return;
        }
        let Some(driver) = crate::app::frame::lookup(generation) else {
            return;
        };
        // Not the live frame — but a RETIRING one still gets its terminal
        // word in: its last digest is the evidence the disposal baseline
        // reads, and dropping it would leave the probe describing a runtime
        // that is already gone.
        let live = self.live_driver().as_ref().map(|d| d.generation()) == Some(generation);
        let terminal = matches!(
            &item,
            FrameVocabulary::ReadPoint(_)
                | FrameVocabulary::SaveSettings(_)
                | FrameVocabulary::SaveCover { .. }
                | FrameVocabulary::SaveGloss { .. }
                | FrameVocabulary::PublishDigest(_)
                | FrameVocabulary::DocStatus(_)
        );
        if !live && (driver.slot() != FrameSlot::Retiring || !terminal) {
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
                // The Shell paints its own document too (the backdrop between
                // frames, the boot and error covers): follow the runtime's
                // edit instead of keeping the look it booted with.
                let _ = leptos::prelude::Set::try_set(&state.settings, *settings);
            }
            FrameVocabulary::SaveCover { path, image } => {
                crate::services::save_cover(&path, image);
            }
            // Only a reader glosses — the shelf's own gloss upkeep is row data
            // it writes in its frame — so a list from any other kind is not a
            // gloss the user made, and is refused.
            FrameVocabulary::SaveGloss { key, marks } => {
                if driver.kind() == FrameKind::Reader {
                    crate::services::save_gloss(&key, &marks);
                }
            }
            // Routed before the gates above.
            FrameVocabulary::BakeCover { .. } => {}
            FrameVocabulary::DocStatus(report) => {
                let live = self.live_driver().map(|d| d.generation()) == Some(generation)
                    && self.active() == Some(ActiveRuntime::Reader);
                let launched = *self.reader_launched.lock().unwrap() == Some(generation);
                if live && launched && report.status == "Ready" {
                    // The session showed the book it was handed: from here
                    // on, Idle is the book going away. Not on Opening — the
                    // initial host's Idle can still be in flight behind it.
                    *self.reader_armed.lock().unwrap() = Some(generation);
                }
                let armed = *self.reader_armed.lock().unwrap() == Some(generation);
                // Idle only means "the book is gone" for a reader that
                // actually opened the book it was handed. Boot-time Idle
                // trailing admission must not bounce a new open to Library.
                if live && armed && report.status == "Idle" {
                    self.note_launch(None);
                    self.navigate_library(state);
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

    // -----------------------------------------------------------------
    // Reader launch identity
    // -----------------------------------------------------------------

    /// Record which reader session was handed a document (`None` when the
    /// shelf takes over). The session is armed only once it answers.
    fn note_launch(&self, generation: Option<u64>) {
        *self.reader_launched.lock().unwrap() = generation;
        *self.reader_armed.lock().unwrap() = None;
    }
}

impl RuntimeManager {
    /// The live driver, resolved from the slot's generation on the page
    /// thread (None while `Starting` or after teardown).
    fn live_driver(&self) -> Option<Rc<Driver>> {
        let generation = self.slot.lock().unwrap().generation()?;
        crate::app::frame::lookup(generation)
    }
}

impl Default for RuntimeManager {
    fn default() -> Self {
        Self::new()
    }
}

impl RuntimeName {
    /// The frame kind that boots this runtime's artifact.
    pub const fn frame_kind(self) -> FrameKind {
        match self {
            RuntimeName::Library => FrameKind::Library,
            RuntimeName::Reader => FrameKind::Reader,
        }
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

/// Clear only Shell boot markup. Frames have explicit driver ownership;
/// removing an error/loading card must never remove an incoming or retiring realm.
fn clear_host(host: &web_sys::Element) {
    boot::clear_boot(host);
}

/// History carries bounded navigation metadata, never the cover raster or
/// a live runtime. The forward handler resolves the latest persisted read point.
fn history_launch(launch: &LaunchDocument) -> LaunchDocument {
    LaunchDocument {
        book_id: launch.book_id.clone(),
        path: launch.path.clone(),
        resume_page: launch.resume_page,
        saved_fraction: launch.saved_fraction,
        blend_override: launch.blend_override,
        cover_data_url: None,
        display_name: launch.display_name.clone(),
    }
}

/// A forward-history Reader entry keeps a plain launch descriptor. The
/// Reader runtime itself is always fresh after a Library return.
fn navigate_reader(launch: &LaunchDocument) {
    let Ok(value) = serde_wasm_bindgen::to_value(&history_launch(launch)) else {
        return;
    };
    if let Some(window) = web_sys::window()
        && let Ok(history) = window.history()
    {
        let _ = history.push_state_with_url(&value, "", Some("/reader"));
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

#[cfg(test)]
mod tests {
    use super::history_launch;
    use runtime_contract::boundary::LaunchDocument;

    #[test]
    fn history_keeps_no_cover_raster_or_runtime_state() {
        let launch = LaunchDocument {
            book_id: Some("book".to_string()),
            path: "/samples/book.pdf".to_string(),
            resume_page: 7,
            saved_fraction: Some(0.5),
            blend_override: true,
            cover_data_url: Some("data:image/jpeg;base64,not-history-data".to_string()),
            display_name: Some("Book".to_string()),
        };
        let history = history_launch(&launch);
        assert!(history.cover_data_url.is_none());
        assert_eq!(history.path, launch.path);
        assert_eq!(history.resume_page, 7);
        assert_eq!(history.saved_fraction, Some(0.5));
        assert!(history.blend_override);
        assert_eq!(history.display_name, launch.display_name);
    }
}
