//! The Shell's route/lifecycle authority. Library and Reader host are
//! disposable iframe/WASM artifacts; the Reader host owns independent
//! document realms, never document engines in the persistent Shell.
//!
//! A Reader is booted only for an actual open and ALWAYS retired on Library
//! return. There is no Reader intent prewarm, idle grace or realm recycling.
//! Only the lightweight Library may wait behind an active Reader. Both cold
//! and warm handoffs retain the outgoing pixels until the incoming frame
//! reports Painted; retirement runs behind the revealed route.

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

/// How long the shell waits after the reader is on screen before it boots
/// the shelf behind it. Long enough that the runtime the user is looking at
/// keeps the machine while it settles; short enough that the warm one is up
/// before the click that needs it.
const WARM_DELAY_MS: i32 = 700;

/// How long a runtime the user just left stays intact, hidden, before its
/// session is disposed and the frame recycled. Two jobs: the disposal lands
/// after the reveal has painted instead of competing with it, and a user who
/// goes straight back (shelf → book → shelf) gets the very session they left
/// — scroll, selection and all — with no disposal or remount at all.
const RECYCLE_DELAY_MS: i32 = 1_200;

/// The bound on a recycled frame's rearm (Rearm → Ready). A fresh session in
/// a loaded document is a mount, so this is generous; past it the frame is
/// removed and the slot boots a new one the ordinary way.
const REARM_TIMEOUT_MS: i32 = 10_000;

/// Where a recycle is. The recycling frame holds the warm lane from the
/// moment it is left, so nothing else boots a second runtime of its kind.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum RecyclePhase {
    /// Still intact behind the reveal; the timer will start the disposal. A
    /// promotion now simply takes the live session back.
    Pending,
    /// `Dispose` sent, `DisposeComplete` awaited.
    Disposing,
    /// `Rearm` sent, the fresh session's `Ready` awaited.
    Rearming,
}

#[derive(Clone, Copy, Debug)]
struct Recycle {
    lane: WarmLane,
    phase: RecyclePhase,
    timer: Option<i32>,
}

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
    /// Boundary traffic a WARM frame sent. Not "stale" — that frame is live
    /// and current — but a runtime the user is not looking at does not get to
    /// write durable state or publish the probe's digest.
    pub warm_traffic_seen: std::sync::atomic::AtomicU64,
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
    /// The frame being recycled, if one is (at most one: it holds the warm
    /// lane).
    recycle: Mutex<Option<Recycle>>,
}

thread_local! {
    /// The shell state, attached once the Shell component exists. NOT a
    /// manager field: a Leptos `RwSignal` shell must stay out of any type
    /// that crosses a `Sync` boundary (`provide_context` requires it), and
    /// there is exactly one thread this field ever runs on — the frame
    /// driver's closures arrive from the same event loop the manager awaits
    /// on — so the module thread-local is the honest wiring.
    static SHELL_STATE: RefCell<Option<ShellState>> = const { RefCell::new(None) };
    /// Promotions waiting for a recycle to finish (its rearm, or its
    /// failure). Resolved together whenever a recycle ends.
    static RECYCLE_WAITERS: RefCell<Vec<js_sys::Function>> = const { RefCell::new(Vec::new()) };
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
            incoming: Mutex::new(None),
            last_digest: Mutex::new(None),
            doc_status: Mutex::new("Idle".to_string()),
            doc_error: Mutex::new(None),
            stale_frames_seen: Default::default(),
            warm_traffic_seen: Default::default(),
            reader_launched: Mutex::new(None),
            reader_armed: Mutex::new(None),
            recycle: Mutex::new(None),
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

    /// How many reader frames are in the page, whatever their slot (the
    /// probe's `readerFramesResident`). This — not the active runtime, not
    /// the warm lane — is the memory question: a reader frame that exists
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
                self.note_launch(Some(driver.generation()));
            }
            return Ok(());
        }
        if self.active() == Some(ActiveRuntime::Library) && runtime == RuntimeName::Library {
            boot::clear_loading(&host);
            boot::set_active(&host, runtime);
            self.set_phase(BootPhase::Active(runtime));
            return Ok(());
        }

        // The warm path: the runtime the user asked for is already booted.
        if let Some(lane) = self.warm_lane_for(runtime)
            && self.promote_warm(&host, lane).await
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
                && self.promote_warm(&host, lane).await
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
    async fn promote_warm(&self, host: &web_sys::Element, lane: WarmLane) -> bool {
        let Some(driver) = crate::app::frame::lookup(lane.generation) else {
            self.clear_warm(lane.generation);
            return false;
        };
        // A recycling frame: still intact → take its session straight back;
        // mid-disposal or mid-rearm → wait for the fresh session, which is
        // still far cheaper than a boot.
        if !self.settle_recycle(lane).await {
            return false;
        }
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

        let _ = wasm_bindgen_futures::JsFuture::from(driver.wait_painted()).await;
        if driver.paint_outcome() != Some(Ok(())) {
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

        // Only Library can wait behind Reader. Refresh the persisted read
        // points and run the startup passes the hidden shelf held back.
        driver.send(&ShellFrame::Refresh);
        self.note_launch(None);

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
            // Claims the warm lane when it recycles, which makes the
            // schedule below a no-op: the counterpart IS the frame just left.
            self.retire_or_recycle(generation);
        }
        self.warm_counterpart_unasked(lane.kind);
        true
    }

    /// Only Library is eligible to wait behind Reader. No event on the
    /// Library route can allocate a Reader host without a real open.
    fn warm_counterpart_unasked(&self, revealed: RuntimeName) {
        let counterpart = revealed.counterpart();
        if counterpart == RuntimeName::Library {
            self.schedule_warm_library(WARM_DELAY_MS);
        }
    }

    /// A fresh realm without sacrificing the outgoing pixels. An incoming
    /// frame is laid out but hidden until BOTH Ready and Painted arrive.
    async fn cold_start(
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
        driver.promote();
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
            self.retire_or_recycle(generation);
        }
        self.warm_counterpart_unasked(runtime);
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
                FrameEvent::Contact => {}
                FrameEvent::Ready => {
                    // A first boot's Ready resolves its driver's gate (the
                    // warmer and cold start await that). A SECOND Ready is a
                    // recycled frame's fresh session — only the manager is
                    // waiting on that one.
                    manager.finish_rearm(generation);
                }
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
                    // Only an actual Painted report admits visible DOM.
                    // A timeout fails the boot; it never fabricates paint.
                    if let (Some(host), Some(active)) = (manager.host(), manager.active()) {
                        let runtime = match active {
                            ActiveRuntime::Library => RuntimeName::Library,
                            ActiveRuntime::Reader => RuntimeName::Reader,
                        };
                        boot::set_active(&host, runtime);
                        manager.set_phase(BootPhase::Active(runtime));
                    }
                    // The runtime the user is looking at is settled: now the
                    // counterpart may boot behind it — when the policy boots
                    // one unasked (Library behind Reader). Library never
                    // loads a Reader host without an actual open.
                    if let Some(active) = manager.active() {
                        let runtime = match active {
                            ActiveRuntime::Library => RuntimeName::Library,
                            ActiveRuntime::Reader => RuntimeName::Reader,
                        };
                        manager.warm_counterpart_unasked(runtime);
                    }
                }
                FrameEvent::Failed { stage, cause } => {
                    let Some(driver) = crate::app::frame::lookup(generation) else {
                        return;
                    };
                    if driver.slot() == FrameSlot::Incoming {
                        driver.teardown();
                        return;
                    }
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
    // The warm slot
    // -----------------------------------------------------------------

    /// Boot `kind` behind the runtime on screen after `delay_ms`. Idempotent,
    /// and a no-op while the slot already holds a runtime (promoted or not).
    fn schedule_warm_library(&self, delay_ms: i32) {
        let kind = RuntimeName::Library;
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
            delay_ms,
        ) {
            Ok(handle) => *claimed.warm_timer.lock().unwrap() = Some(handle),
            Err(_) => {
                claimed.clear_warm(lane.generation);
                return;
            }
        }
        timer.into_js_value();
    }

    /// Boot only Library into the warm slot. It holds startup passes until
    /// Refresh; no Reader or document code is loaded by this lane.
    async fn run_warm(&self, lane: WarmLane) {
        if !self.warm_holds(lane.generation) {
            return;
        }
        // A recycling lane already HAS its frame: booting a second one under
        // the same generation would put two frames behind one identity.
        if crate::app::frame::lookup(lane.generation).is_some() {
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
        // A lane that was a recycling frame stops being one with it (a warm
        // failure, a superseded recycle); the promotion path has already
        // settled its recycle by the time it clears the lane.
        let recycling = self
            .recycle
            .lock()
            .unwrap()
            .filter(|r| r.lane.generation == generation)
            .map(|r| r.lane);
        if let Some(lane) = recycling {
            self.end_recycle(lane);
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
        // A bake request is gate-free: a cold shelf asks for its backfill
        // from inside its own mount — before its Ready verdict admits it to
        // the frame registry, and before the manager has made it the live
        // slot — so both the registry lookup and the live check below would
        // drop the shelf's only request. A bake request writes nothing
        // durable: the baker keys it on the generation, starts it once that
        // frame is admitted, and prunes it if the frame is torn down first.
        // Only a shelf bakes; a frame already known as anything else is
        // refused.
        if let FrameVocabulary::BakeCover { path } = item {
            let known_not_a_shelf = crate::app::frame::lookup(generation)
                .is_some_and(|driver| driver.kind() != FrameKind::Library);
            if !known_not_a_shelf {
                crate::app::bake::request(generation, path);
            }
            return;
        }
        let Some(driver) = crate::app::frame::lookup(generation) else {
            return;
        };
        let item = match item {
            // A recycled frame's kept session is still the session that just
            // left: its tail (a prefetch that settles as dropped after the
            // close, the dispose beats) is the evidence the disposal
            // baseline reads, even though the frame already sits in the warm
            // slot. Only while that session lives — once the frame rearms,
            // the fresh session is warm traffic like any other.
            FrameVocabulary::PublishDigest(json)
                if matches!(
                    self.recycle_phase(generation),
                    Some(RecyclePhase::Pending | RecyclePhase::Disposing)
                ) =>
            {
                if let Ok(value) = serde_json::from_str::<serde_json::Value>(&json) {
                    *self.last_digest.lock().unwrap() = Some(value);
                }
                return;
            }
            other => other,
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
    // Recycling
    // -----------------------------------------------------------------

    /// Record which reader session was handed a document (`None` when the
    /// shelf takes over). The session is armed only once it answers.
    fn note_launch(&self, generation: Option<u64>) {
        *self.reader_launched.lock().unwrap() = generation;
        *self.reader_armed.lock().unwrap() = None;
    }

    /// Whether the runtime just left may keep its frame. The shelf always
    /// may (its heap is small and bounded); a reader only below the heap
    /// line, read from its last digest — the drained one it published on
    /// its way out. The HIGH-WATER mark is what is read: the live heap is
    /// low again after every close, but the frame's linear memory stays at
    /// the largest size the session ever reached, and that is the number a
    /// kept frame keeps paying for.
    fn may_recycle(&self, kind: RuntimeName) -> bool {
        kind == RuntimeName::Library
    }

    /// The handoff's outgoing half. The frame is already hidden. It becomes
    /// the warm counterpart in place — kept intact for a moment, then its
    /// session is disposed (the same §12 disposal, fully accounted) and a
    /// fresh warm Library session mounts in the same document. Reader
    /// hosts are always retired and removed, never claimed by the warm lane.
    fn retire_or_recycle(&self, generation: u64) {
        let Some(driver) = crate::app::frame::lookup(generation) else {
            return;
        };
        let kind: RuntimeName = driver.kind().into();
        let lane = WarmLane { kind, generation };
        let claimed = self.may_recycle(kind) && {
            let mut warm = self.warm.lock().unwrap();
            if warm.lane().is_some() {
                false
            } else {
                *warm = WarmState::Warming(lane);
                true
            }
        };
        if !claimed {
            self.retire(generation);
            return;
        }
        // A warm boot of this kind may be waiting on its delay: the frame
        // just left replaces it.
        self.abandon_warm_timer();
        let timer = self.handle().and_then(|manager| {
            let window = web_sys::window()?;
            let tick = wasm_bindgen::closure::Closure::<dyn FnMut()>::new(move || {
                let manager = manager.clone();
                wasm_bindgen_futures::spawn_local(async move {
                    manager.run_recycle(lane).await;
                });
            });
            let id = window
                .set_timeout_with_callback_and_timeout_and_arguments_0(
                    tick.as_ref().unchecked_ref(),
                    RECYCLE_DELAY_MS,
                )
                .ok()?;
            tick.into_js_value();
            Some(id)
        });
        *self.recycle.lock().unwrap() = Some(Recycle {
            lane,
            phase: RecyclePhase::Pending,
            timer,
        });
        if timer.is_none() {
            // No timer (no window): recycle on the spot rather than never.
            if let Some(manager) = self.handle() {
                wasm_bindgen_futures::spawn_local(async move {
                    manager.run_recycle(lane).await;
                });
            }
        }
    }

    fn recycle_phase(&self, generation: u64) -> Option<RecyclePhase> {
        self.recycle
            .lock()
            .unwrap()
            .filter(|r| r.lane.generation == generation)
            .map(|r| r.phase)
    }

    /// Advance a recycle from `from` to `to`, only if it is still THIS
    /// frame's recycle in the expected phase.
    fn advance_recycle(&self, lane: WarmLane, from: RecyclePhase, to: RecyclePhase) -> bool {
        let mut recycle = self.recycle.lock().unwrap();
        match recycle.as_mut() {
            Some(r) if r.lane == lane && r.phase == from => {
                r.phase = to;
                r.timer = None;
                true
            }
            _ => false,
        }
    }

    /// Dispose the kept session, then rearm the frame.
    async fn run_recycle(&self, lane: WarmLane) {
        if !self.advance_recycle(lane, RecyclePhase::Pending, RecyclePhase::Disposing) {
            return;
        }
        let Some(driver) = crate::app::frame::lookup(lane.generation) else {
            self.end_recycle(lane);
            self.clear_warm(lane.generation);
            return;
        };
        let promise = driver.grace_dispose();
        let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
        let outcome = driver.take_dispose_outcome();
        self.note_dispose_completed(lane.kind);
        if let Some(Err(_)) = outcome {
            web_sys::console::warn_1(&JsValue::from_str(&format!(
                "[mareader] forced frame removal for {:?} ({})",
                lane.kind,
                heard_summary(&driver)
            )));
            self.drop_recycled(&driver, lane);
            return;
        }
        if !self.warm_holds(lane.generation)
            || !self.advance_recycle(lane, RecyclePhase::Disposing, RecyclePhase::Rearming)
        {
            // Superseded while it disposed (a cold start cleared the slot):
            // the frame has no future, so it goes like any retired frame.
            self.end_recycle(lane);
            driver.teardown();
            return;
        }
        driver.rearm();
        self.arm_rearm_timeout(lane);
    }

    /// Bound the rearm: a fresh session that never answers is removed, and
    /// the slot warms a new frame the ordinary way.
    fn arm_rearm_timeout(&self, lane: WarmLane) {
        let (Some(manager), Some(window)) = (self.handle(), web_sys::window()) else {
            return;
        };
        let tick = wasm_bindgen::closure::Closure::<dyn FnMut()>::new(move || {
            if manager.recycle_phase(lane.generation) != Some(RecyclePhase::Rearming) {
                return;
            }
            if let Some(driver) = crate::app::frame::lookup(lane.generation) {
                web_sys::console::warn_1(&JsValue::from_str(&format!(
                    "[mareader] the recycled {} frame never rearmed ({})",
                    lane.kind.label(),
                    heard_summary(&driver)
                )));
                manager.drop_recycled(&driver, lane);
            } else {
                manager.end_recycle(lane);
                manager.clear_warm(lane.generation);
            }
        });
        if window
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                tick.as_ref().unchecked_ref(),
                REARM_TIMEOUT_MS,
            )
            .is_ok()
        {
            tick.into_js_value();
        }
    }

    /// A recycle that cannot finish: remove the frame, free the lane, and
    /// warm a fresh counterpart if the policy boots one unasked (the shelf
    /// behind Reader; never Reader behind Library).
    fn drop_recycled(&self, driver: &Rc<Driver>, lane: WarmLane) {
        driver.teardown();
        self.end_recycle(lane);
        self.clear_warm(lane.generation);
        let revealed = match self.active() {
            Some(ActiveRuntime::Library) => RuntimeName::Library,
            Some(ActiveRuntime::Reader) => RuntimeName::Reader,
            None => return,
        };
        if revealed.counterpart() == lane.kind {
            self.warm_counterpart_unasked(revealed);
        }
    }

    /// A recycled frame's fresh session answered `Ready`: it is the warm
    /// counterpart again, and counted as the new session it is.
    fn finish_rearm(&self, generation: u64) {
        let lane = {
            let mut recycle = self.recycle.lock().unwrap();
            let current = *recycle;
            match current {
                Some(r) if r.lane.generation == generation && r.phase == RecyclePhase::Rearming => {
                    *recycle = None;
                    r.lane
                }
                _ => return,
            }
        };
        self.note_session_created(lane.kind);
        self.set_warm_ready(lane);
        wake_recycle_waiters();
        web_sys::console::debug_1(&JsValue::from_str(&format!(
            "[mareader] {} recycled warm at generation {}",
            lane.kind.label(),
            lane.generation
        )));
    }

    fn end_recycle(&self, lane: WarmLane) {
        let timer = {
            let mut recycle = self.recycle.lock().unwrap();
            let current = *recycle;
            match current {
                Some(r) if r.lane == lane => {
                    *recycle = None;
                    r.timer
                }
                _ => None,
            }
        };
        if let (Some(id), Some(window)) = (timer, web_sys::window()) {
            window.clear_timeout_with_handle(id);
        }
        wake_recycle_waiters();
    }

    /// Make a recycling lane promotable. `Pending` → cancel: the session the
    /// user left is intact, so it is simply taken back. Mid-disposal or
    /// mid-rearm → wait for the fresh session. `false` means the frame
    /// cannot be revealed (the slot has already been emptied).
    async fn settle_recycle(&self, lane: WarmLane) -> bool {
        match self.recycle_phase(lane.generation) {
            None => true,
            Some(RecyclePhase::Pending) => {
                self.end_recycle(lane);
                true
            }
            Some(RecyclePhase::Disposing | RecyclePhase::Rearming) => {
                while self.recycle_phase(lane.generation).is_some() {
                    let wait = js_sys::Promise::new(&mut |resolve, _reject| {
                        RECYCLE_WAITERS.with(|w| w.borrow_mut().push(resolve));
                    });
                    let _ = wasm_bindgen_futures::JsFuture::from(wait).await;
                }
                matches!(
                    *self.warm.lock().unwrap(),
                    WarmState::Ready(held) if held.generation == lane.generation
                )
            }
        }
    }
}

fn wake_recycle_waiters() {
    let waiters = RECYCLE_WAITERS.with(|w| std::mem::take(&mut *w.borrow_mut()));
    for resolve in waiters {
        let _ = resolve.call0(&JsValue::NULL);
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

/// Clear only Shell boot markup. Frames have explicit driver ownership;
/// removing an error/loading card must never remove a warm or retiring realm.
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
    use super::{RuntimeManager, RuntimeName, history_launch};
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

    #[test]
    fn only_library_realms_may_recycle() {
        let manager = RuntimeManager::new();
        assert!(manager.may_recycle(RuntimeName::Library));
        assert!(!manager.may_recycle(RuntimeName::Reader));
        *manager.last_digest.lock().unwrap() = Some(serde_json::json!({
            "wasmHeapBytes": 0,
            "heapHighWaterBytes": 0,
            "host": { "panesCreated": 0 }
        }));
        assert!(!manager.may_recycle(RuntimeName::Reader));
    }
}
