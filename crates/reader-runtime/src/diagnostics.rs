//! The lifecycle/memory diagnostics surface: one home for the Phase 0
//! baseline counters instead of scattered debug prints.
//!
//! Two halves: Rust-owned counters (reader runtime lifecycle, reader pane,
//! live virtualizers, the disposal epoch, the wasm heap high-water mark) and
//! engine-owned counters (PDF session, worker, render lane, thumbnails),
//! read through `pdf_engine::api::engine_stats` at snapshot time because
//! those resources are created and released inside the engine — counting
//! them anywhere else would count a secondhand story.
//!
//! Counters are cheap atomics, always on. Console narration is opt-in
//! (`window.__mareaderDiagnostics()` in the app webview turns it on and
//! returns the JSON snapshot), so ordinary operation logs nothing. The
//! pairing rules the snapshot exposes — every session/worker/pane dies once,
//! every started render resolves — are what the teardown baseline asserts;
//! see `docs/memory-baseline.md`.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use serde::Serialize;

use app_state::memory::wasm_heap_bytes;

static READER_RUNTIMES_CREATED: AtomicU64 = AtomicU64::new(0);
static READER_DISPOSES_COMPLETED: AtomicU64 = AtomicU64::new(0);
static PANES_CREATED: AtomicU64 = AtomicU64::new(0);
static PANES_DISPOSED: AtomicU64 = AtomicU64::new(0);
static VIRTUALIZERS_CREATED: AtomicU64 = AtomicU64::new(0);
static VIRTUALIZERS_DISPOSED: AtomicU64 = AtomicU64::new(0);

/// The largest heap sample ever observed. The wasm heap only ever grows
/// (`Memory.grow` is monotonic), so the high water mark is mostly the current
/// size — but it makes a heap that stepped up during a workload visible even
/// when a later reading of the same process caught it lower, and it is the
/// number the baseline workloads chart.
static HEAP_HIGH_WATER: AtomicU64 = AtomicU64::new(0);
static READER_PAGE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static READER_LIVE: AtomicBool = AtomicBool::new(true);

/// Whether lifecycle events are narrated to the console. Off in normal
/// operation; the dev surface flips it on.
static EVENT_LOG: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// The live virtualizers, so a snapshot can read their window and
    /// zombie counts from the handles that own them. Entries are added where
    /// a virtualizer is created and removed in the SAME owner's cleanup, so
    /// the registry only ever holds handles their owner still holds too —
    /// it never extends a lifetime past disposal.
    static LIVE_VIRTUALIZERS: RefCell<Vec<virtual_list_leptos::Virtualizer>> =
        const { RefCell::new(Vec::new()) };
    /// The reader runtime's self-reported lifecycle view (Phase 1 §12): the
    /// runtime itself publishes every state transition with its generation
    /// and its live resource count, and the snapshot relays it. `None` off
    /// the app (host tests) — reported, not guessed.
    static RUNTIME_VIEW: RefCell<Option<crate::runtime::RuntimeView>> =
        const { RefCell::new(None) };
}

/// The runtime publishes its lifecycle here on every transition; this is
/// what makes disposal completion observable BY the runtime, not inferred
/// from its surroundings.
pub fn publish_runtime_view(
    lifecycle: crate::runtime::RuntimeLifecycle,
    generation: u64,
    virtualizer_count: usize,
) {
    RUNTIME_VIEW.with(|cell| {
        *cell.borrow_mut() = Some(crate::runtime::RuntimeView {
            lifecycle,
            generation,
            virtualizer_count,
        });
    });
}

/// Narrate one lifecycle event when the dev surface opted in. Counters tick
/// regardless; this is only the narration half.
fn event(name: &str) {
    if !EVENT_LOG.load(Ordering::Relaxed) {
        return;
    }
    narrate(name, "");
}

/// Narrate one lifecycle event with detail, when opted in.
fn narrate(name: &str, detail: &str) {
    if !EVENT_LOG.load(Ordering::Relaxed) {
        return;
    }
    #[cfg(target_arch = "wasm32")]
    web_sys::console::log_1(&format!("[lifecycle] {name}{detail}").into());
    #[cfg(not(target_arch = "wasm32"))]
    let _ = (name, detail);
}

/// Fold the current heap size into the high-water mark.
fn observe_heap() {
    if let Some(bytes) = wasm_heap_bytes() {
        note_heap_sample(bytes);
    }
}

/// Record one heap sample (bytes) into the high-water mark. The probe in
/// [`app_state::memory`] logs through here so the chart and the snapshot see the
/// same number.
pub(crate) fn note_heap_sample(bytes: u64) {
    HEAP_HIGH_WATER.fetch_max(bytes, Ordering::Relaxed);
}

/// The ordinal for the reader session about to start: one per session this
/// artifact hosts, monotonic for the artifact's life.
///
/// This is the runtime's IDENTITY (§21), not a per-instance counter. Two
/// sessions inside one artifact — the Shell loads the reader once and mounts
/// it per open — must never both answer "generation 1": a fresh runtime is a
/// NEW number, so a revived one would be visible as a repeated number rather
/// than as a plausible first mount. Distinct from
/// [`note_reader_runtime_create`], which counts document opens.
pub fn next_session_ordinal() -> u64 {
    READER_SESSIONS_STARTED.fetch_add(1, Ordering::Relaxed) + 1
}

/// Sessions this artifact has started (the identity counter above).
static READER_SESSIONS_STARTED: AtomicU64 = AtomicU64::new(0);

/// An open flow CLAIMED the document state — the boundary hook today's
/// architecture has for "a reader runtime began". Counted once per attempt,
/// failed opens included, because the claim is what the hook observes; a
/// failed attempt is a create whose dispose never needs to run, so the
/// pairing these counters prove is not liveness (that is
/// `reader_runtime_live` / the engine's `hasDocument`) but the close path's
/// completion count. Phase 1's explicit runtime object replaces this hook
/// with a real lifetime.
pub(crate) fn note_reader_runtime_create() {
    READER_RUNTIMES_CREATED.fetch_add(1, Ordering::Relaxed);
    event("reader_runtime:create");
}

/// The reader runtime's dispose began: `close_document` started tearing the
/// engine document down and resetting the reader slice. The caller passes
/// its claim stamp so the completion assertion can tell its own moment from
/// a later open's.
pub(crate) fn note_reader_runtime_dispose_begin(_stamp: u64) {
    event("reader_runtime:dispose_begin");
}

/// The reader runtime's dispose completed: the engine destroy+sweep tail
/// resolved. This is the moment the baseline asserts on — unless a newer
/// open or close claimed the state meanwhile, in which case this dispose's
/// evidence is stale and the assertion belongs to whoever holds the state
/// now (a fast close → reopen must not read as a broken baseline).
pub(crate) fn note_reader_runtime_dispose_complete(stamp: u64) {
    READER_DISPOSES_COMPLETED.fetch_add(1, Ordering::Relaxed);
    observe_heap();
    if crate::services::document::session::current_epoch() == stamp {
        assert_dispose_baseline();
    }
    event("reader_runtime:dispose_complete");
    // The Shell's manager awaits the dispose export's promise before it starts
    // the next runtime (§5): the tail that just drained is what resolves it.
    // A document close inside a live session reaches this with nothing
    // pending, which resolves to a no-op.
    crate::resolve_dispose();
}

/// The dispose tail's assertion (Phase 0's gate): after the sweeps, nothing
/// reader-owned is reachable and the engine half is drained. A broken
/// baseline is an actionable lifecycle failure — the one report this
/// surface emits uninvited, with the full snapshot as evidence. A passing
/// baseline is only narrated when the dev surface opted in.
fn assert_dispose_baseline() {
    // The reader slice was reset synchronously before the dispose tail ran,
    // so the runtime is no longer live by construction; the snapshot's
    // engine half and the live gauges are what the assertion really reads.
    let snap = snapshot();
    if snap.at_baseline() {
        if EVENT_LOG.load(Ordering::Relaxed) {
            narrate("reader_runtime:baseline_ok ", &snapshot_json());
        }
        return;
    }
    let json = snapshot_json();
    #[cfg(target_arch = "wasm32")]
    web_sys::console::error_1(
        &format!("[lifecycle] reader dispose left resources behind:\n{json}").into(),
    );
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("[lifecycle] reader dispose left resources behind:\n{json}");
}

/// A reader pane mounted (today: the `/reader` surface; the workspace pane
/// tree of later phases reports the same hook).
pub(crate) fn note_pane_create() {
    PANES_CREATED.fetch_add(1, Ordering::Relaxed);
    event("pane:create");
}

/// A reader pane's owner disposed it.
pub(crate) fn note_pane_dispose() {
    PANES_DISPOSED.fetch_add(1, Ordering::Relaxed);
    event("pane:dispose");
}

/// Register a live virtualizer with the diagnostics registry. Called right
/// where `use_virtualizer` returns; pair with [`untrack_virtualizer`] in the
/// same owner's cleanup.
pub(crate) fn track_virtualizer(v: &virtual_list_leptos::Virtualizer) {
    LIVE_VIRTUALIZERS.with(|live| live.borrow_mut().push(v.clone()));
    VIRTUALIZERS_CREATED.fetch_add(1, Ordering::Relaxed);
}

/// Drop a virtualizer from the diagnostics registry. Handles compare by
/// identity, so exactly the disposed entry goes.
pub(crate) fn untrack_virtualizer(v: &virtual_list_leptos::Virtualizer) {
    let removed = LIVE_VIRTUALIZERS.with(|live| {
        let mut list = live.borrow_mut();
        let at = list.iter().position(|candidate| candidate == v);
        at.map(|at| list.remove(at)).is_some()
    });
    if removed {
        VIRTUALIZERS_DISPOSED.fetch_add(1, Ordering::Relaxed);
    }
}

/// The runtime's self-reported ownership view (Phase 1 §12), folded into
/// every snapshot: the lifecycle state, the generation stamp, and the
/// resource counts the runtime itself owns or reads from the engine. The
/// important field is `state` — the runtime REPORTS its own disposal
/// completion instead of the surroundings inferring it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct RuntimeSnapshot {
    state: crate::runtime::RuntimeLifecycle,
    generation: u64,
    active_document: bool,
    active_render_tasks: u32,
    active_prefetch: u32,
    registered_pages: u32,
    virtualizer_count: usize,
    listener_count: usize,
    timer_count: usize,
    worker_count: u64,
}

/// One point-in-time reading of every resource the reader owns, plus the
/// engine's half where an engine is attached.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Snapshot {
    /// Reader runtimes that ever started (open flows claimed the state).
    reader_runtimes_created: u64,
    /// Reader runtimes whose dispose tail completed.
    reader_disposes_completed: u64,
    /// Whether a reader runtime is live right now (a document is open or
    /// opening) — supplied by the caller, which reads it from the document
    /// status.
    reader_runtime_live: bool,
    /// The document session's claim stamp: it moves on every open and close,
    /// so a snapshot can be attributed to a moment in the lifecycle.
    disposal_epoch: u64,
    /// The reader's current page (the viewer's own counter). The fast-jump
    /// workload reads it to name the destination window it asserts the
    /// render trace against — the destination comes from the app, not from
    /// a scroll-position guess.
    reader_page: u32,
    /// False when a create/dispose pair went impossible (disposed > created:
    /// a double dispose). `pane_live` derives from saturating subtraction,
    /// so accounting corruption would otherwise read as a quiet zero — the
    /// baseline must fail on it instead.
    accounting_consistent: bool,
    /// Live panes, and the panes that ever mounted/disposed.
    pane_live: u64,
    panes_created: u64,
    panes_disposed: u64,
    /// Live virtualizers, their currently-mounted window items, and their
    /// zombie-retained items — the virtualizer state that must all read
    /// zero once the reader is gone.
    virtualizer_live: usize,
    virtualizers_created: u64,
    virtualizers_disposed: u64,
    live_window_items: usize,
    retained_virtual_items: usize,
    /// The virtualizers' live DOM/event bookkeeping: event listener
    /// bindings, `ResizeObserver` bindings, and armed timers (scroll-end
    /// debounce + retention expiry). The ownership document lists these as
    /// held resources; a dispose that leaked one now shows here instead of
    /// being inferred from the handle count.
    virtualizer_listeners: usize,
    virtualizer_observers: usize,
    virtualizer_timers: usize,
    /// The reader's mounted-window ceiling ([`crate::features::virtualizers::RENDER_BUDGET`]
    /// max items). The browser baseline asserts its observed peaks against
    /// this number, so the test enforces the live policy rather than a
    /// copy of it.
    render_budget_max_items: u32,
    /// Look-ahead (paper colour) samples in flight — the prefetch work the
    /// baseline must see and see drained.
    lookahead_samples_active: usize,
    /// The engine's half (PDF session, worker, render lane, thumbnails).
    /// `None` without an engine — reported, not guessed.
    engine: Option<pdf_engine::api::EngineStats>,
    /// The runtime's self-reported view, when one has published (the app
    /// runtime publishes from birth; host tests without a runtime report
    /// `None` rather than inventing a state).
    #[serde(skip_serializing_if = "Option::is_none")]
    runtime: Option<RuntimeSnapshot>,
    wasm_heap_bytes: Option<u64>,
    heap_high_water_bytes: u64,
}

impl Snapshot {
    /// The post-close baseline this phase's acceptance is written against:
    /// nothing reader-owned is live and the engine half is drained. The
    /// monotonic create/dispose counters are deliberately NOT a liveness
    /// test — a failed open consumes a create without ever needing a
    /// dispose — the liveness they could assert is already carried by the
    /// engine's own `hasDocument`. The wasm heap's level is likewise not
    /// part of this: the arena never shrinks; what matters is that
    /// ownership does not survive.
    pub(crate) fn at_baseline(&self) -> bool {
        !self.reader_runtime_live
            && self.pane_live == 0
            && self.virtualizer_live == 0
            && self.retained_virtual_items == 0
            && self.lookahead_samples_active == 0
            // FAIL CLOSED: a create/dispose pair that went impossible is
            // bookkeeping corruption, not a drained reader.
            && self.accounting_consistent
            // FAIL CLOSED: an engine the diagnostics bridge cannot read is
            // not a drained engine. A missing/broken engine surface must
            // never launder itself into "at baseline" — unverifiable is its
            // own failure mode, and exactly the one this gate exists to
            // catch.
            && matches!(&self.engine, Some(engine) if engine.drained())
    }
}

/// The viewer's page, pushed by the reading-progress sync so the digest
/// carries it without the probe needing a signal handle.
pub fn set_reader_page(page: u32) {
    READER_PAGE.store(page, Ordering::Relaxed);
}

/// The digest is built only while a session owns this artifact; the flag is
/// set at session start and cleared at session end.
pub fn set_reader_live(live: bool) {
    READER_LIVE.store(live, Ordering::Relaxed);
}

/// Take a snapshot. `reader_runtime_live` comes from the caller because the
/// authoritative bit (document status) is reactive state, not a global.
pub(crate) fn snapshot() -> Snapshot {
    observe_heap();
    let runtime_view = RUNTIME_VIEW.with(|cell| *cell.borrow());
    let engine = engine_probe();
    let (
        live_window_items,
        retained_virtual_items,
        virtualizer_listeners,
        virtualizer_observers,
        virtualizer_timers,
    ) = LIVE_VIRTUALIZERS.with(|live| {
        let list = live.borrow();
        (
            list.iter()
                .map(virtual_list_leptos::Virtualizer::live_window_items)
                .sum(),
            list.iter()
                .map(virtual_list_leptos::Virtualizer::retained_items)
                .sum(),
            list.iter()
                .map(virtual_list_leptos::Virtualizer::listener_bindings)
                .sum(),
            list.iter()
                .map(virtual_list_leptos::Virtualizer::observer_bindings)
                .sum(),
            list.iter()
                .map(virtual_list_leptos::Virtualizer::armed_timers)
                .sum(),
        )
    });
    Snapshot {
        reader_runtimes_created: READER_RUNTIMES_CREATED.load(Ordering::Relaxed),
        reader_disposes_completed: READER_DISPOSES_COMPLETED.load(Ordering::Relaxed),
        reader_runtime_live: READER_LIVE.load(Ordering::Relaxed),
        disposal_epoch: crate::services::document::session::current_epoch(),
        reader_page: READER_PAGE.load(Ordering::Relaxed),
        pane_live: PANES_CREATED
            .load(Ordering::Relaxed)
            .saturating_sub(PANES_DISPOSED.load(Ordering::Relaxed)),
        panes_created: PANES_CREATED.load(Ordering::Relaxed),
        panes_disposed: PANES_DISPOSED.load(Ordering::Relaxed),
        accounting_consistent: PANES_DISPOSED.load(Ordering::Relaxed)
            <= PANES_CREATED.load(Ordering::Relaxed)
            && VIRTUALIZERS_DISPOSED.load(Ordering::Relaxed)
                <= VIRTUALIZERS_CREATED.load(Ordering::Relaxed),
        virtualizer_live: LIVE_VIRTUALIZERS.with(|live| live.borrow().len()),
        lookahead_samples_active: pdf_engine::backdrop::pending_samples(),
        virtualizers_created: VIRTUALIZERS_CREATED.load(Ordering::Relaxed),
        virtualizers_disposed: VIRTUALIZERS_DISPOSED.load(Ordering::Relaxed),
        live_window_items,
        retained_virtual_items,
        virtualizer_listeners,
        virtualizer_observers,
        virtualizer_timers,
        render_budget_max_items: crate::features::virtualizers::RENDER_BUDGET.max_items as u32,
        runtime: runtime_view.map(|view| {
            let engine_stats: pdf_engine::api::EngineStats = engine.unwrap_or_default();
            RuntimeSnapshot {
                state: view.lifecycle,
                generation: view.generation,
                active_document: engine_stats.has_document,
                active_render_tasks: engine_stats.active_renders,
                active_prefetch: engine_stats.active_prefetches,
                registered_pages: engine_stats.pages,
                virtualizer_count: view.virtualizer_count,
                listener_count: virtualizer_listeners,
                timer_count: virtualizer_timers,
                worker_count: engine_stats
                    .workers_created
                    .saturating_sub(engine_stats.workers_terminated),
            }
        }),
        engine,
        wasm_heap_bytes: wasm_heap_bytes(),
        heap_high_water_bytes: HEAP_HIGH_WATER.load(Ordering::Relaxed),
    }
}

/// The engine's live counters, or `None` where there is no engine to read:
/// the probe talks only on wasm, and a host test must not walk into the
/// wasm-bindgen stubs.
fn engine_probe() -> Option<pdf_engine::api::EngineStats> {
    #[cfg(target_arch = "wasm32")]
    {
        pdf_engine::api::engine_stats()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        None
    }
}

/// The digest as the Shell's probe receives it: the SAME JSON field set the
/// unified app's snapshot carried, built here (the reader owns every field
/// the reader measures) and pushed through the boundary on changes. The
/// Shell layers its own manager facts on top.
pub(crate) fn snapshot_json() -> String {
    let snap = snapshot();
    let mut value = match serde_json::to_value(&snap) {
        Ok(value) => value,
        Err(_) => return "{}".to_string(),
    };
    // The verdict rides the snapshot so an automated baseline check asserts
    // one field instead of re-deriving the rule on the consumer side. The
    // consumer (the Shell) ANDs this with its own manager facts: a drained
    // reader digest means nothing while the manager still holds a session.
    value["atBaseline"] = serde_json::Value::Bool(snap.at_baseline());
    serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string())
}

/// Push the digest across the boundary NOW. Called on the moments the
/// consumer waits on: lifecycle transitions, document status changes, page
/// turns, virtualizer registration, and the dispose beats. Bookkeeping, not
/// logging: no output unless someone is listening.
pub fn publish_digest(api: &dyn runtime_contract::boundary::ShellApi) {
    api.publish_digest(snapshot_json());
}

/// The artifact's own `__mareaderDiagnostics()` probe, installed in its own
/// window (the frame's in the hosted case). It answers a FRESH snapshot —
/// the Shell-side global merges this runtime's last pushed digest with the
/// manager's facts, which is one digest beat stale; a probe that has to race
/// in-flight engine work (the deep CI's close-during-render race) reads this
/// window's probe so "active" means active at the moment of the read, not at
/// the moment of the last beat. The probe carries the reader's own fields
/// only — the session create/dispose accounting and the AND-ed baseline
/// verdict belong to the Shell's global (§21).
#[cfg(target_arch = "wasm32")]
pub fn install() {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::prelude::Closure;

    let Some(window) = web_sys::window() else {
        return;
    };
    let probe = Closure::wrap(Box::new(|| snapshot_json()) as Box<dyn Fn() -> String>);
    let probe: wasm_bindgen::JsValue = probe.into_js_value();
    let name = wasm_bindgen::JsValue::from_str("__mareaderDiagnostics");
    let target: js_sys::Object = window.unchecked_into();
    _ = js_sys::Reflect::set(&target, &name, &probe);
}

thread_local! {
    /// Session facts the wasm exports report: creations, dispose requests.
    /// The shell's own manager keeps the authoritative counts; these mirror
    /// them inside the artifact for the digest (§21).
    static SESSION_FACTS: std::cell::RefCell<SessionFacts> =
        std::cell::RefCell::new(SessionFacts::default());
}

#[derive(Default)]
struct SessionFacts {
    created: u64,
    dispose_requests: u64,
}

/// A session was created (the wasm start export ran).
pub fn note_session_create(id: u32) {
    let _ = id;
    SESSION_FACTS.with(|f| f.borrow_mut().created += 1);
}

/// The shell asked this runtime to dispose.
pub fn note_dispose_request(id: u32) {
    let _ = id;
    SESSION_FACTS.with(|f| f.borrow_mut().dispose_requests += 1);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_dispose_notes_move_their_counters() {
        let created_before = READER_RUNTIMES_CREATED.load(Ordering::Relaxed);
        let disposed_before = READER_DISPOSES_COMPLETED.load(Ordering::Relaxed);
        note_reader_runtime_create();
        // A stamp no epoch can match: the completion assertion is the close
        // tail's business, not this counter test's.
        note_reader_runtime_dispose_complete(u64::MAX);
        assert_eq!(
            READER_RUNTIMES_CREATED.load(Ordering::Relaxed),
            created_before + 1
        );
        assert_eq!(
            READER_DISPOSES_COMPLETED.load(Ordering::Relaxed),
            disposed_before + 1
        );
    }

    #[test]
    fn pane_notes_pair() {
        let created_before = PANES_CREATED.load(Ordering::Relaxed);
        let disposed_before = PANES_DISPOSED.load(Ordering::Relaxed);
        note_pane_create();
        note_pane_dispose();
        assert_eq!(
            PANES_CREATED.load(Ordering::Relaxed) - PANES_DISPOSED.load(Ordering::Relaxed),
            created_before - disposed_before
        );
    }

    #[test]
    fn the_runtime_reports_its_own_lifecycle_in_the_snapshot() {
        publish_runtime_view(crate::runtime::RuntimeLifecycle::Ready, 3, 2);
        let value: serde_json::Value =
            serde_json::from_str(&snapshot_json()).expect("snapshot is JSON");
        let runtime = value
            .get("runtime")
            .expect("the runtime view rides the snapshot");
        assert_eq!(runtime["state"], "ready");
        assert_eq!(runtime["generation"], 3);
        assert_eq!(runtime["virtualizerCount"], 2);
        // The snapshot's runtime half reports the resource counts the
        // baseline gates on (Phase 1 §12) — present even with no engine.
        for field in [
            "activeDocument",
            "activeRenderTasks",
            "activePrefetch",
            "registeredPages",
            "listenerCount",
            "timerCount",
            "workerCount",
        ] {
            assert!(runtime.get(field).is_some(), "runtime view lacks {field}");
        }
        RUNTIME_VIEW.with(|cell| *cell.borrow_mut() = None);
    }

    #[test]
    fn snapshot_serializes_with_the_documented_shape() {
        let json = snapshot_json();
        let value: serde_json::Value = serde_json::from_str(&json).expect("snapshot is JSON");
        for field in [
            "readerRuntimesCreated",
            "readerDisposesCompleted",
            "readerRuntimeLive",
            "disposalEpoch",
            "readerPage",
            "accountingConsistent",
            "panesCreated",
            "panesDisposed",
            "virtualizersCreated",
            "virtualizersDisposed",
            "paneLive",
            "virtualizerLive",
            "retainedVirtualItems",
            "engine",
            "wasmHeapBytes",
            "heapHighWaterBytes",
        ] {
            assert!(value.get(field).is_some(), "snapshot lacks {field}");
        }
    }

    /// A synthetic drained snapshot. Built by hand rather than read from the
    /// global counters: sibling tests tick those concurrently, and the
    /// baseline question is about the SHAPE, not about this process's
    /// moment.
    fn drained_engine() -> pdf_engine::api::EngineStats {
        pdf_engine::api::EngineStats::default()
    }

    fn drained_snapshot() -> Snapshot {
        Snapshot {
            reader_runtimes_created: 7,
            reader_disposes_completed: 7,
            reader_runtime_live: false,
            disposal_epoch: 9,
            reader_page: 0,
            accounting_consistent: true,
            pane_live: 0,
            panes_created: 7,
            panes_disposed: 7,
            virtualizer_live: 0,
            virtualizers_created: 28,
            virtualizers_disposed: 28,
            live_window_items: 0,
            retained_virtual_items: 0,
            virtualizer_listeners: 0,
            virtualizer_observers: 0,
            virtualizer_timers: 0,
            render_budget_max_items: 3,
            lookahead_samples_active: 0,
            runtime: None,
            engine: Some(drained_engine()),
            wasm_heap_bytes: None,
            heap_high_water_bytes: 0,
        }
    }

    #[test]
    fn a_drained_snapshot_is_at_baseline() {
        assert!(drained_snapshot().at_baseline());
    }

    #[test]
    fn an_unreadable_engine_fails_closed() {
        // "Cannot inspect the engine" is not "the engine is drained": a
        // broken diagnostics bridge must never pass the gate it guards.
        let mut snap = drained_snapshot();
        snap.engine = None;
        assert!(!snap.at_baseline());
    }

    #[test]
    fn any_live_ownership_breaks_the_baseline() {
        let mut snap = drained_snapshot();
        snap.reader_runtime_live = true;
        assert!(!snap.at_baseline());
        let mut snap = drained_snapshot();
        snap.pane_live = 1;
        assert!(!snap.at_baseline());
        let mut snap = drained_snapshot();
        snap.virtualizer_live = 2;
        assert!(!snap.at_baseline());
        let mut snap = drained_snapshot();
        snap.retained_virtual_items = 3;
        assert!(!snap.at_baseline());
        let mut snap = drained_snapshot();
        snap.lookahead_samples_active = 1;
        assert!(!snap.at_baseline());
        // A create whose dispose never ran is deliberately NOT a baseline
        // break: a failed open consumes a create without needing a dispose,
        // and the liveness that inequality tried to assert is carried by
        // the engine's own hasDocument instead.
        let mut snap = drained_snapshot();
        snap.reader_disposes_completed = 6;
        assert!(snap.at_baseline());
    }

    #[test]
    fn an_undrained_engine_breaks_the_baseline() {
        let mut snap = drained_snapshot();
        snap.engine = Some(pdf_engine::api::EngineStats {
            pages: 1,
            ..pdf_engine::api::EngineStats::default()
        });
        assert!(!snap.at_baseline());
        snap.engine = Some(pdf_engine::api::EngineStats {
            prefetches_started: 2,
            prefetches_completed: 1,
            ..pdf_engine::api::EngineStats::default()
        });
        assert!(!snap.at_baseline());
        // And a fully balanced engine half keeps it: the pairing rules hold.
        let mut snap = drained_snapshot();
        snap.engine = Some(pdf_engine::api::EngineStats {
            sessions_opened: 4,
            sessions_destroyed: 4,
            workers_created: 5,
            workers_terminated: 5,
            renders_started: 11,
            renders_completed: 9,
            renders_cancelled: 2,
            renders_queued: 9,
            renders_dropped: 2,
            prefetches_started: 6,
            prefetches_completed: 4,
            prefetches_dropped: 2,
            ..pdf_engine::api::EngineStats::default()
        });
        assert!(snap.at_baseline(), "{snap:?}");
    }
}
