//! The runtime manager: the one navigation authority (§10), the owner of
//! "which runtime is active", and the Shell minder of the frame lifecycle —
//! insertion, handshake, ready-timeout, two-phase disposal (§12). A runtime
//! is a frame ([`crate::app::frame::Driver`]), never a module the shell
//! executes; what the manager decides is the POLICY around the frames.
//!
//! That policy is a warm slot (see `docs/runtime-split.md`).
//!
//! ```text
//! cold boot   ACTIVE painted ──▶ WARM counterpart boots (hidden, no work)
//!             click          ──▶ reveal WARM in place ──▶ ACTIVE
//!                                                    └─▶ old ACTIVE retires
//! ```
//!
//! The ordering is the whole change. Disposal used to be awaited BEFORE the
//! replacement existed, which made every transition a cold boot of the
//! incoming artifact: the user paid the artifact's module load, wasm
//! instantiation and Leptos mount on every click. Now the outgoing runtime is
//! retired AFTER the handoff, when nobody is looking at it — so the click
//! costs a reveal, and the memory is still reclaimed as a unit (the retired
//! frame is removed identically, just not on the critical path).
//!
//! The trade this makes, stated plainly: two runtimes are briefly resident
//! where the old policy held one. The second one is boot-only — a warm reader
//! holds no document, and a warm shelf runs none of its startup passes — so
//! what is resident is a module and a mount, not a workload.
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
use runtime_contract::protocol::ShellFrame;
use wasm_bindgen::{JsCast, JsValue};

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

/// A runtime booted behind the active one and waiting to be revealed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct WarmLane {
    kind: RuntimeName,
    generation: u64,
}

/// Where the warm slot is: empty, booting, or booted and ready to reveal.
#[derive(Clone, Copy, Debug)]
enum WarmState {
    None,
    /// Booting. The generation is minted up front so a promotion that arrives
    /// mid-boot can await THIS frame instead of building a second one.
    Warming(WarmLane),
    Ready(WarmLane),
}

impl WarmState {
    fn lane(self) -> Option<WarmLane> {
        match self {
            WarmState::None => None,
            WarmState::Warming(lane) | WarmState::Ready(lane) => Some(lane),
        }
    }
}

/// How long the shell waits after a runtime is on screen before it boots the
/// counterpart behind it. Long enough that the runtime the user is looking at
/// keeps the machine while it settles; short enough that the warm one is up
/// before the click that needs it.
const WARM_DELAY_MS: i32 = 700;

pub struct RuntimeManager {
    slot: Mutex<Slot>,
    /// The runtime booted behind the active one (one at most, and never the
    /// same kind as the active one).
    warm: Mutex<WarmState>,
    /// The timer that will boot the warming lane. Kept so a click that
    /// outran the delay can cancel it and boot the lane on the spot.
    warm_timer: Mutex<Option<i32>>,
    /// What the runtime host is showing (§6, §11), in the plain form the
    /// diagnostics probe reads. The pair (state + error) is always written
    /// together so a probe can never read one side stale.
    pub boot_state: Mutex<String>,
    pub boot_error: Mutex<Option<serde_json::Value>>,
    /// Create/dispose counts for the diagnostics identity (§21): a reader →
    /// library transition must leave active reader = none, library = one.
    /// A warm runtime counts as created the moment it answers `Ready`, so the
    /// live total is `created - disposed == active + warm`.
    pub reader_sessions_created: std::sync::atomic::AtomicU64,
    pub reader_disposes_completed: std::sync::atomic::AtomicU64,
    pub library_sessions_created: std::sync::atomic::AtomicU64,
    pub library_disposes_completed: std::sync::atomic::AtomicU64,
    host: Mutex<Option<web_sys::Element>>,
    /// Starts are serialized (§10): a navigation mid-start becomes the
    /// pending request, newest intent wins. Warming is NOT serialized — it
    /// runs beside the active runtime on purpose.
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
    /// Boundary traffic a WARM frame sent. Not "stale" — that frame is live
    /// and current — but a runtime the user is not looking at does not get to
    /// write durable state or publish the probe's digest.
    pub warm_traffic_seen: std::sync::atomic::AtomicU64,
    /// The reader session the Shell has handed a document to. An Idle
    /// document status only means "the book is gone, go back to the shelf"
    /// for a session that ever had one — a warm reader starts Idle.
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
            warm: Mutex::new(WarmState::None),
            warm_timer: Mutex::new(None),
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
            warm_traffic_seen: Default::default(),
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

    /// A handle on the shared manager. Retirement and warming outlive the
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

    /// Which runtime is waiting behind the active one (the diagnostics
    /// probe's `warmRuntime`).
    ///
    /// These three read the warm slot for `crate::diagnostics`, whose
    /// reporter is web-only — on native there is no probe to feed.
    #[cfg(target_arch = "wasm32")]
    pub fn warm_runtime(&self) -> Option<ActiveRuntime> {
        self.warm
            .lock()
            .unwrap()
            .lane()
            .map(|lane| match lane.kind {
                RuntimeName::Reader => ActiveRuntime::Reader,
                RuntimeName::Library => ActiveRuntime::Library,
            })
    }

    /// Whether the warm runtime has finished booting: only a ready frame can
    /// be revealed, so a promotion for a warming one waits out its remainder.
    #[cfg(target_arch = "wasm32")]
    pub fn warm_ready(&self) -> bool {
        matches!(&*self.warm.lock().unwrap(), WarmState::Ready(_))
    }

    /// The warm frame's generation, so the suites can prove the frame that
    /// was revealed is the one that was booted — not a fresh one.
    #[cfg(target_arch = "wasm32")]
    pub fn warm_generation(&self) -> Option<u64> {
        self.warm.lock().unwrap().lane().map(|lane| lane.generation)
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

    /// One navigation request, resolved the cheapest way available:
    /// in-session, then by revealing a warm frame, then by booting.
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
                *self.reader_armed.lock().unwrap() = Some(driver.generation());
            }
            return Ok(());
        }
        if self.active() == Some(ActiveRuntime::Library) && runtime == RuntimeName::Library {
            return Ok(());
        }

        // The warm path: the runtime the user asked for is already booted.
        if let Some(lane) = self.warm_lane_for(runtime)
            && self.promote_warm(&host, lane, launch.clone()).await
        {
            return Ok(());
        }
        // Falling through means there was no warm frame, or the warm frame
        // died on its way up. One more case is worth a boot before a cold
        // start: the lane is CLAIMED but its delay has not elapsed, so there
        // is no frame to promote yet. Booting it now is the same work a cold
        // start would do, with one difference the user can see — the runtime
        // they are looking at stays on screen for it instead of going behind
        // a loading cover, and the arrival is still a reveal.
        if let Some(lane) = self.warming_lane().filter(|lane| lane.kind == runtime) {
            self.abandon_warm_timer();
            self.run_warm(lane).await;
            if let Some(lane) = self.warm_lane_for(runtime)
                && self.promote_warm(&host, lane, launch.clone()).await
            {
                return Ok(());
            }
        }
        self.cold_start(&host, runtime, launch).await
    }

    /// The warm lane holding `runtime`, whether it has finished booting yet.
    fn warm_lane_for(&self, runtime: RuntimeName) -> Option<WarmLane> {
        self.warm
            .lock()
            .unwrap()
            .lane()
            .filter(|lane| lane.kind == runtime)
    }

    /// Reveal a warm runtime and retire the one the user just left.
    ///
    /// Returns `false` only when the warm frame could not be promoted, in
    /// which case it has already been torn down and the warm slot emptied.
    async fn promote_warm(
        &self,
        host: &web_sys::Element,
        lane: WarmLane,
        launch: Option<LaunchDocument>,
    ) -> bool {
        let Some(driver) = crate::app::frame::lookup(lane.generation) else {
            self.clear_warm(lane.generation);
            return false;
        };
        // A click that beat the warm boot waits for its REMAINDER rather than
        // starting a second frame — still strictly less work than a boot, and
        // never two boots of the same artifact.
        if !driver.is_ready() {
            let _ = wasm_bindgen_futures::JsFuture::from(driver.wait_verdict()).await;
        }
        let failed = match driver.ready_outcome() {
            Some(Ok(())) => None,
            Some(Err(stage)) => Some(stage),
            None => Some(FrameFatalStage::ReadyTimeout),
        };
        if let Some(stage) = failed {
            let heard = heard_summary(&driver);
            web_sys::console::warn_1(&JsValue::from_str(&format!(
                "[mareader] the warm {} frame could not be revealed ({}) — {}",
                lane.kind.label(),
                heard,
                crate::app::frame::fatal_cause(&driver, stage)
            )));
            driver.teardown();
            self.clear_warm(lane.generation);
            return false;
        }

        // The reveal. Nothing is rebuilt: same element, same document, same
        // realm, same wasm instance — this is the line the whole warm slot
        // exists to make cheap.
        driver.promote();
        self.clear_warm(lane.generation);
        let outgoing = self.replace_active(lane.kind, driver.generation());

        // What a runtime cannot know while it waits behind another one.
        match lane.kind {
            RuntimeName::Reader => {
                if let Some(document) = launch {
                    driver.send(&ShellFrame::Launch {
                        document: Box::new(document),
                    });
                    *self.reader_armed.lock().unwrap() = Some(driver.generation());
                }
            }
            RuntimeName::Library => {
                // The shelf it seeded at boot predates the reading session:
                // the row the reader was in has moved since. Re-read the
                // store and run the startup passes it held back.
                driver.send(&ShellFrame::Refresh);
                *self.reader_armed.lock().unwrap() = None;
            }
        }

        // The revealed frame painted while it was waiting, so there is no
        // loading cover to hold and nothing to wait for: the active stamp
        // lands with the reveal.
        boot::clear_loading(host);
        boot::set_active(host, lane.kind);
        boot::uncover_page();
        self.set_phase(BootPhase::Active(lane.kind));

        // And only now does the outgoing runtime go. It is off screen, so its
        // disposal is not on the critical path — but it is the same disposal:
        // graceful first (DisposeComplete), forced after the strict timeout,
        // the frame removed either way.
        if let Some(generation) = outgoing {
            // Hide it in the same breath as the reveal, never one task
            // later: two frames may differ, but two VISIBLE frames may not.
            if let Some(leaving) = crate::app::frame::lookup(generation) {
                leaving.begin_retiring();
            }
            self.retire(generation);
        }
        self.schedule_warm(lane.kind.counterpart());
        true
    }

    /// Boot a runtime the slow way: no warm frame to reveal, so the host
    /// covers and the incoming artifact pays its own boot.
    async fn cold_start(
        &self,
        host: &web_sys::Element,
        runtime: RuntimeName,
        launch: Option<LaunchDocument>,
    ) -> Result<(), BootError> {
        // §11: covered BEFORE the first await.
        boot::paint_loading(host, runtime);
        self.set_phase(BootPhase::Loading(runtime));
        // The outgoing frame is gone (and acknowledged) before the
        // replacement exists. Only the cold path serializes this way: it is
        // the one case where there is nothing to look at yet.
        self.dispose_active().await;
        *self.slot.lock().unwrap() = Slot::Starting;
        clear_host(host);
        boot::paint_loading(host, runtime);

        let generation = crate::app::frame::next_generation();
        let Some(driver) = Driver::new(runtime.frame_kind(), host, generation, FrameSlot::Active)
        else {
            let message = format!("the {} frame element could not be created", runtime.label());
            return Err(BootError::new(runtime, BootStage::Start, message));
        };
        let manager_events = self.events_hook();
        driver.start(launch.clone(), manager_events);
        let _ = wasm_bindgen_futures::JsFuture::from(driver.wait_verdict()).await;
        match driver.ready_outcome() {
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
        self.note_session_created(runtime);
        crate::app::frame::register(driver.clone());
        *self.slot.lock().unwrap() = match runtime {
            RuntimeName::Reader => Slot::Reader {
                generation: driver.generation(),
            },
            RuntimeName::Library => Slot::Library {
                generation: driver.generation(),
            },
        };
        *self.reader_armed.lock().unwrap() = match runtime {
            RuntimeName::Reader if launch.is_some() => Some(driver.generation()),
            _ => None,
        };
        // Active is NOT published here. The frame answered `Ready` (the
        // runtime is mounted), but the loading cover is still down until
        // `Painted` — reporting Active now is what made the terminal say
        // `boot: library` while the user still stared at a loading screen.
        // The Painted handler below sets the host active and publishes the
        // phase, so the line lands exactly when the cover lifts.
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
            // A warm frame's lifecycle is the manager's business, not the
            // state's: it is not on screen, so its stages never become the
            // shell's visible state.
            let warm = crate::app::frame::lookup(generation)
                .is_some_and(|driver| driver.slot() == FrameSlot::Warm);
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
                    if warm {
                        return;
                    }
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
                    // The runtime the user is looking at is settled: now the
                    // counterpart may boot behind it.
                    if let Some(active) = manager.active() {
                        let runtime = match active {
                            ActiveRuntime::Library => RuntimeName::Library,
                            ActiveRuntime::Reader => RuntimeName::Reader,
                        };
                        manager.schedule_warm(runtime.counterpart());
                    }
                }
                FrameEvent::Failed { stage, cause } => {
                    let Some(driver) = crate::app::frame::lookup(generation) else {
                        return;
                    };
                    if driver.slot() == FrameSlot::Warm {
                        // A warm boot failing is not a visible failure. The
                        // runtime on screen is untouched; all that is lost is
                        // the next transition's head start.
                        web_sys::console::warn_1(&JsValue::from_str(&format!(
                            "[mareader] the warm {} frame failed ({cause})",
                            driver.kind().label()
                        )));
                        driver.teardown();
                        manager.clear_warm(generation);
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

    // -----------------------------------------------------------------
    // The warm slot
    // -----------------------------------------------------------------

    /// Boot the counterpart behind the runtime on screen, once the host has
    /// something to look at. Idempotent, and a no-op while the slot already
    /// holds a runtime (promoted or not).
    fn schedule_warm(&self, kind: RuntimeName) {
        let Some(manager) = self.handle() else {
            return;
        };
        let lane = {
            let mut warm = manager.warm.lock().unwrap();
            if warm.lane().is_some() {
                // Something is already waiting. A warm runtime of the wrong
                // kind means the active runtime changed underneath us; the
                // next transition will retire or reveal this one first.
                return;
            }
            let lane = WarmLane {
                kind,
                generation: crate::app::frame::next_generation(),
            };
            *warm = WarmState::Warming(lane);
            lane
        };
        let Some(window) = web_sys::window() else {
            manager.clear_warm(lane.generation);
            return;
        };
        // A handle the error path can still reach: the closure takes the
        // other one, and a timer the window refused has to unwind the claim
        // it just made on the slot.
        let claimed = manager.clone();
        let timer = wasm_bindgen::closure::Closure::<dyn FnMut()>::new(move || {
            let manager = manager.clone();
            wasm_bindgen_futures::spawn_local(async move {
                manager.run_warm(lane).await;
            });
        });
        match window.set_timeout_with_callback_and_timeout_and_arguments_0(
            timer.as_ref().unchecked_ref(),
            WARM_DELAY_MS,
        ) {
            Ok(handle) => *claimed.warm_timer.lock().unwrap() = Some(handle),
            Err(_) => {
                claimed.clear_warm(lane.generation);
                return;
            }
        }
        timer.into_js_value();
    }

    /// Boot one runtime into the warm slot. It renders and stops: a warm
    /// reader is given no document (warming a PDF session would retain the
    /// memory warming exists to avoid), and a warm shelf is told it is warm
    /// so it holds its startup passes.
    async fn run_warm(&self, lane: WarmLane) {
        if !self.warm_holds(lane.generation) {
            return;
        }
        let Some(host) = self.host() else {
            self.clear_warm(lane.generation);
            return;
        };
        let Some(driver) = Driver::new(
            lane.kind.frame_kind(),
            &host,
            lane.generation,
            FrameSlot::Warm,
        ) else {
            self.clear_warm(lane.generation);
            return;
        };
        driver.start(None, self.events_hook());
        // Registered BEFORE the wait, not after: a promotion that arrives
        // mid-boot resolves from the same verdict this one is waiting on,
        // and the two continuations resume in registration order — so the
        // frame has to be findable by the time the promotion looks for it.
        crate::app::frame::register(driver.clone());
        let _ = wasm_bindgen_futures::JsFuture::from(driver.wait_verdict()).await;
        if !matches!(driver.ready_outcome(), Some(Ok(()))) {
            // A warm boot that fails is invisible and cheap: the next
            // transition simply pays the boot it would have paid anyway.
            driver.teardown();
            self.clear_warm(lane.generation);
            return;
        }
        // The runtime exists from the moment it answers: the warm slot holds
        // a session, and the accounting has to be able to see it.
        self.note_session_created(lane.kind);
        self.set_warm_ready(lane);
        web_sys::console::debug_1(&JsValue::from_str(&format!(
            "[mareader] {} warm at generation {}",
            lane.kind.label(),
            lane.generation
        )));
    }

    fn warm_holds(&self, generation: u64) -> bool {
        matches!(
            *self.warm.lock().unwrap(),
            WarmState::Warming(lane) | WarmState::Ready(lane) if lane.generation == generation
        )
    }

    /// Mark a warm boot finished. Generation-guarded: a promotion that beat
    /// the boot has already emptied the slot, and must not be resurrected.
    fn set_warm_ready(&self, lane: WarmLane) {
        let mut warm = self.warm.lock().unwrap();
        if warm
            .lane()
            .is_some_and(|held| held.generation == lane.generation)
        {
            *warm = WarmState::Ready(lane);
        }
    }

    /// Empty the warm slot, but only if it still holds THIS generation — a
    /// promotion and a failed warm boot can both reach here for one frame.
    fn clear_warm(&self, generation: u64) {
        let mut warm = self.warm.lock().unwrap();
        if warm
            .lane()
            .is_some_and(|held| held.generation == generation)
        {
            *warm = WarmState::None;
            drop(warm);
            self.abandon_warm_timer();
        }
    }

    /// Cancel the pending warm boot, if one is pending. A lane whose delay
    /// never elapsed has no frame to cancel — only the timer that would have
    /// made one.
    fn abandon_warm_timer(&self) {
        let handle = self.warm_timer.lock().unwrap().take();
        if let (Some(handle), Some(window)) = (handle, web_sys::window()) {
            window.clear_timeout_with_handle(handle);
        }
    }

    /// The lane that is claimed but not yet booted — the state a click can
    /// outrun.
    fn warming_lane(&self) -> Option<WarmLane> {
        match *self.warm.lock().unwrap() {
            WarmState::Warming(lane) => Some(lane),
            WarmState::Ready(_) | WarmState::None => None,
        }
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
    }

    /// Dispose the live frame and AWAIT it. Only the cold path needs this: it
    /// is the one case with nothing on screen to preserve.
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
                let heard = heard_summary(&driver);
                web_sys::console::warn_1(&JsValue::from_str(&format!(
                    "[mareader] forced frame removal for {runtime:?} ({heard})"
                )));
                driver.teardown();
            }
        }
        self.note_dispose_completed(runtime);
        *self.slot.lock().unwrap() = Slot::None;
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
        navigate("/reader");
        self.start_reader(state, launch);
    }

    /// A reader handback: reveal the library, retire the reader.
    pub fn navigate_library(&self, state: &ShellState) {
        navigate("/");
        self.start_library(state);
    }

    /// The frame-dispatched boundary vocabulary. Called by the driver's
    /// event hook; the hook itself is generation-gated at the port.
    pub fn dispatch_boundary(&self, state: &ShellState, generation: u64, item: FrameVocabulary) {
        let Some(driver) = crate::app::frame::lookup(generation) else {
            return;
        };
        if driver.slot() == FrameSlot::Warm {
            // A warm runtime is live and current, so this is not stale-frame
            // traffic — but it is not the one on screen either. Letting it
            // write durable state or publish the probe's digest would let a
            // runtime the user cannot see describe the application.
            self.warm_traffic_seen
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return;
        }
        // Not the live frame — but a RETIRING one still gets its terminal
        // word in: its last digest is the evidence the disposal baseline
        // reads, and dropping it would leave the probe describing a runtime
        // that is already gone.
        let live = self.live_driver().as_ref().map(|d| d.generation()) == Some(generation);
        if !live && driver.slot() != FrameSlot::Retiring {
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
                self.bake_for_library(generation, path);
            }
            FrameVocabulary::DocStatus(report) => {
                if report.status != "Ready" {
                    let live = self.active() == Some(ActiveRuntime::Reader);
                    let armed = *self.reader_armed.lock().unwrap() == Some(generation);
                    // Idle only means "the book is gone" for a reader the
                    // Shell actually handed a book to; a warm reader boots
                    // with nothing open and must not bounce the user back.
                    if live && armed && report.status == "Idle" {
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
    /// the boundary (§35). The asking frame is resolved by generation, not
    /// from the active slot: a warm shelf's bakes are answered to the warm
    /// shelf.
    fn bake_for_library(&self, generation: u64, path: String) {
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
            let Some(driver) = crate::app::frame::lookup(generation) else {
                return;
            };
            if driver.generation() != generation || driver.kind() != FrameKind::Library {
                return;
            }
            driver.send(&runtime_contract::protocol::ShellFrame::CoverBaked { path, image });
        });
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
    /// The runtime that waits behind this one.
    pub const fn counterpart(self) -> RuntimeName {
        match self {
            RuntimeName::Library => RuntimeName::Reader,
            RuntimeName::Reader => RuntimeName::Library,
        }
    }

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

/// Empty one half of the host: the outgoing runtime's frame and the shell's
/// boot markup before the next frames (§11 — never mounted underneath).
/// Scoped to a slot so clearing the way for a cold boot cannot take a warm
/// runtime's frame out from under it.
fn clear_host(host: &web_sys::Element) {
    // Single-quoted: CSS takes either, and it keeps the selector out of the
    // escaping business entirely.
    let selector = format!(
        "iframe[data-mareader-slot='{}'], iframe[data-mareader-slot='{}']",
        FrameSlot::Active.attr(),
        FrameSlot::Retiring.attr()
    );
    if let Ok(nodes) = host.query_selector_all(&selector) {
        for index in 0..nodes.length() {
            let Some(node) = nodes.item(index) else {
                continue;
            };
            let _ = host.remove_child(&node);
        }
    }
    boot::clear_boot(host);
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
