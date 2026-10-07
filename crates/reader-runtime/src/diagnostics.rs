//! Lifecycle/memory diagnostics: always-on counters for reader runtime,
//! panes, virtualizers, heap, plus the engine's own.

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

/// The largest heap sample ever observed; the wasm heap only grows.
static HEAP_HIGH_WATER: AtomicU64 = AtomicU64::new(0);
static READER_PAGE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
static READER_LIVE: AtomicBool = AtomicBool::new(true);
/// Whether this realm runs the PDF engine and must report it drained.
static ENGINE_EXPECTED: AtomicBool =
    AtomicBool::new(cfg!(all(feature = "engine", feature = "pdf")));

/// Whether lifecycle events are narrated to the console.
static EVENT_LOG: AtomicBool = AtomicBool::new(false);

thread_local! {
    /// The live virtualizers: a snapshot reads their window and zombie counts.
    static LIVE_VIRTUALIZERS: RefCell<Vec<virtual_list_leptos::Virtualizer>> =
        const { RefCell::new(Vec::new()) };
    /// The runtime's own lifecycle view, published on each transition.
    static RUNTIME_VIEW: RefCell<Option<crate::runtime::RuntimeView>> =
        const { RefCell::new(None) };
    /// The live reader host's workspace probe (see [`install_host_probe`]).
    static HOST_PROBE: RefCell<Option<HostProbe>> = const { RefCell::new(None) };
}

/// What a snapshot asks the workspace: the live probe, then the settled
/// answer.
enum HostProbe {
    /// Reads the manager's state through a WEAK reference, never extending the
    /// host's lifetime.
    Live(Box<dyn Fn() -> Option<crate::host::HostSnapshot>>),
    /// The workspace as its teardown left it (`disposed`, no panes).
    Settled(crate::host::HostSnapshot),
}

/// The runtime publishes its lifecycle here, making disposal observable.
pub fn publish_runtime_view(lifecycle: crate::runtime::RuntimeLifecycle, generation: u64) {
    RUNTIME_VIEW.with(|cell| {
        *cell.borrow_mut() = Some(crate::runtime::RuntimeView {
            lifecycle,
            generation,
        });
    });
}

/// The host installs its probe here for the session; it reads plain state,
/// weakly.
pub(crate) fn install_host_probe(probe: impl Fn() -> Option<crate::host::HostSnapshot> + 'static) {
    HOST_PROBE.with(|cell| *cell.borrow_mut() = Some(HostProbe::Live(Box::new(probe))));
}

/// The host's teardown finished: the probe becomes the final answer.
pub(crate) fn settle_host_probe(last: crate::host::HostSnapshot) {
    HOST_PROBE.with(|cell| *cell.borrow_mut() = Some(HostProbe::Settled(last)));
}

fn host_probe() -> Option<crate::host::HostSnapshot> {
    HOST_PROBE.with(|cell| match cell.borrow().as_ref()? {
        HostProbe::Live(probe) => probe(),
        HostProbe::Settled(last) => Some(last.clone()),
    })
}

/// Narrate one lifecycle event when the dev surface opted in.
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

/// Record one heap sample (bytes) into the high-water mark.
fn note_heap_sample(bytes: u64) {
    HEAP_HIGH_WATER.fetch_max(bytes, Ordering::Relaxed);
}

/// The ordinal for the session about to start: monotonic per frame.
pub fn next_session_ordinal() -> u64 {
    READER_SESSIONS_STARTED.fetch_add(1, Ordering::Relaxed) + 1
}

/// Sessions this artifact has started (the identity counter above).
static READER_SESSIONS_STARTED: AtomicU64 = AtomicU64::new(0);

/// An open flow CLAIMED the document state; counted per attempt, failed
/// opens included.
pub(crate) fn note_reader_runtime_create() {
    READER_RUNTIMES_CREATED.fetch_add(1, Ordering::Relaxed);
    event("reader_runtime:create");
}

/// A pane's dispose began: its session started tearing the document down.
pub(crate) fn note_reader_runtime_dispose_begin() {
    event("reader_runtime:dispose_begin");
}

/// The runtime's dispose completed: the engine tail resolved, and the
/// baseline is asserted.
pub(crate) fn note_reader_runtime_dispose_complete(stamp: u64) {
    READER_DISPOSES_COMPLETED.fetch_add(1, Ordering::Relaxed);
    observe_heap();
    let still_reading = host_probe().is_some_and(|host| host.still_reading());
    if crate::services::document::session::current_epoch() == stamp && !still_reading {
        assert_dispose_baseline();
    }
    event("reader_runtime:dispose_complete");
}

/// The dispose tail's assertion: nothing reader-owned reachable, engine
/// drained.
fn assert_dispose_baseline() {
    // The reader slice was reset before the tail ran; the gauges are what
    // is read.
    let snap = snapshot();
    if snap.realm_at_baseline() {
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

/// The host's manager created a pane; the dispose pair is the manager's.
pub(crate) fn note_pane_create() {
    PANES_CREATED.fetch_add(1, Ordering::Relaxed);
    event("pane:create");
}

/// The pane manager ran a pane's dispose (its sync teardown completed).
pub(crate) fn note_pane_dispose() {
    PANES_DISPOSED.fetch_add(1, Ordering::Relaxed);
    event("pane:dispose");
}

/// Register a live virtualizer; pair with [`untrack_virtualizer`] in the
/// same owner.
pub(crate) fn track_virtualizer(v: &virtual_list_leptos::Virtualizer) {
    LIVE_VIRTUALIZERS.with(|live| live.borrow_mut().push(v.clone()));
    VIRTUALIZERS_CREATED.fetch_add(1, Ordering::Relaxed);
}

/// Drop a virtualizer from the registry; handles compare by identity.
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

/// The runtime's own ownership view, folded into the snapshot.
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

/// One reading of every resource the reader owns, plus the engine's.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Snapshot {
    /// Reader runtimes that ever started (open flows claimed the state).
    reader_runtimes_created: u64,
    /// Reader runtimes whose dispose tail completed.
    reader_disposes_completed: u64,
    /// Whether a runtime is live now (a document is open or opening).
    reader_runtime_live: bool,
    /// The claim stamp: moves on every open and close.
    disposal_epoch: u64,
    /// The reader's current page, for the fast-jump workload's assertions.
    reader_page: u32,
    /// False when a create/dispose pair went impossible; the baseline must
    /// fail on it.
    accounting_consistent: bool,
    /// Live panes, and the panes that ever mounted/disposed.
    pane_live: u64,
    panes_created: u64,
    panes_disposed: u64,
    /// Live virtualizers, their mounted window items and retained items.
    virtualizer_live: usize,
    virtualizers_created: u64,
    virtualizers_disposed: u64,
    live_window_items: usize,
    retained_virtual_items: usize,
    /// The virtualizers' live bookkeeping: listeners, observers and timers.
    virtualizer_listeners: usize,
    virtualizer_observers: usize,
    virtualizer_timers: usize,
    /// The mounted-window ceiling (`RENDER_BUDGET` max items), which the
    /// baseline asserts peaks against.
    render_budget_max_items: u32,
    /// Look-ahead (paper colour) samples in flight, which must drain.
    lookahead_samples_active: usize,
    /// The engine's half (session, worker, render lane, thumbnails).
    engine: Option<pdf_core::diagnostics::EngineStats>,
    /// The runtime's self-reported view, when one has published.
    #[serde(skip_serializing_if = "Option::is_none")]
    runtime: Option<RuntimeSnapshot>,
    /// The host's workspace: its panes and the one active one.
    #[serde(skip_serializing_if = "Option::is_none")]
    host: Option<crate::host::HostSnapshot>,
    wasm_heap_bytes: Option<u64>,
    heap_high_water_bytes: u64,
}

impl Snapshot {
    /// The post-close baseline: nothing reader-owned live, engine drained.
    pub(crate) fn at_baseline(&self) -> bool {
        !self.reader_runtime_live
            && self.pane_live == 0
            && self.virtualizer_live == 0
            && self.retained_virtual_items == 0
            && self.lookahead_samples_active == 0
            // FAIL CLOSED: a create/dispose pair that went impossible is
            // bookkeeping corruption, not a drained reader.
            && self.accounting_consistent
            // FAIL CLOSED: an unreadable engine is not a drained engine.
            && matches!(&self.engine, Some(engine) if engine.drained())
    }

    /// The baseline of a realm without the engine: only the reader half.
    fn at_baseline_without_engine(&self) -> bool {
        !self.reader_runtime_live
            && self.pane_live == 0
            && self.virtualizer_live == 0
            && self.retained_virtual_items == 0
            && self.lookahead_samples_active == 0
            && self.accounting_consistent
    }

    /// This realm's verdict: the full gate where the engine runs, else the
    /// reader half.
    fn realm_at_baseline(&self) -> bool {
        if ENGINE_EXPECTED.load(Ordering::Relaxed) {
            self.at_baseline()
        } else {
            self.at_baseline_without_engine()
        }
    }
}

/// The viewer's page, pushed by the reading-progress sync.
pub fn set_reader_page(page: u32) {
    READER_PAGE.store(page, Ordering::Relaxed);
}

/// Declare whether this realm runs the PDF engine.
pub fn expect_engine(on: bool) {
    ENGINE_EXPECTED.store(
        on && cfg!(all(feature = "engine", feature = "pdf")),
        Ordering::Relaxed,
    );
}

/// Whether a session owns this artifact; set at session start and end.
pub fn set_reader_live(live: bool) {
    READER_LIVE.store(live, Ordering::Relaxed);
}

/// Take a snapshot; `reader_runtime_live` comes from the caller.
pub(crate) fn snapshot() -> Snapshot {
    observe_heap();
    let runtime_view = RUNTIME_VIEW.with(|cell| *cell.borrow());
    let host = host_probe();
    let pane_virtualizers: usize = host
        .as_ref()
        .map(|host| {
            host.panes
                .iter()
                .map(|pane| pane.resources.virtualizers)
                .sum()
        })
        .unwrap_or(0);
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
        disposal_epoch: crate::services::document::session::current_epoch()
            .saturating_sub(EPOCH_OFFSET.load(Ordering::Relaxed)),
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
        #[cfg(feature = "pdf")]
        lookahead_samples_active: pdf_engine::backdrop::pending_samples(),
        #[cfg(not(feature = "pdf"))]
        lookahead_samples_active: 0,
        virtualizers_created: VIRTUALIZERS_CREATED.load(Ordering::Relaxed),
        virtualizers_disposed: VIRTUALIZERS_DISPOSED.load(Ordering::Relaxed),
        live_window_items,
        retained_virtual_items,
        virtualizer_listeners,
        virtualizer_observers,
        virtualizer_timers,
        render_budget_max_items: crate::features::virtualizers::RENDER_BUDGET.max_items as u32,
        runtime: runtime_view.map(|view| {
            let engine_stats: pdf_core::diagnostics::EngineStats = engine.unwrap_or_default();
            RuntimeSnapshot {
                state: view.lifecycle,
                generation: view.generation,
                active_document: engine_stats.has_document,
                active_render_tasks: engine_stats.active_renders,
                active_prefetch: engine_stats.active_prefetches,
                registered_pages: engine_stats.pages,
                virtualizer_count: pane_virtualizers,
                listener_count: virtualizer_listeners,
                timer_count: virtualizer_timers,
                worker_count: engine_stats
                    .workers_created
                    .saturating_sub(engine_stats.workers_terminated),
            }
        }),
        engine,
        host,
        wasm_heap_bytes: wasm_heap_bytes(),
        heap_high_water_bytes: HEAP_HIGH_WATER.load(Ordering::Relaxed),
    }
}

/// Where the runtime's document-session epoch count starts.
static EPOCH_OFFSET: AtomicU64 = AtomicU64::new(0);

/// Start the runtime's epoch count: fresh from zero, recycled carries on.
#[cfg(target_arch = "wasm32")]
pub(crate) fn begin_epoch(carry: bool) {
    if !carry {
        let now = crate::services::document::session::current_epoch();
        EPOCH_OFFSET.store(now, Ordering::Relaxed);
    }
}

/// True while a render or thumbnail prefetch is in flight.
#[cfg(target_arch = "wasm32")]
pub(crate) fn engine_in_flight() -> bool {
    engine_probe().is_some_and(|e| {
        e.active_renders > 0 || e.active_prefetches > 0 || e.page_active > 0 || e.thumb_active > 0
    })
}

/// The engine's live counters, or `None` where there is no engine.
fn engine_probe() -> Option<pdf_core::diagnostics::EngineStats> {
    #[cfg(all(target_arch = "wasm32", feature = "engine", feature = "pdf"))]
    {
        pdf_engine::api::engine_stats()
    }
    #[cfg(not(all(target_arch = "wasm32", feature = "engine", feature = "pdf")))]
    {
        None
    }
}

/// The digest as the Shell's probe receives it, pushed on changes.
pub(crate) fn snapshot_json() -> String {
    let snap = snapshot();
    let mut value = match serde_json::to_value(&snap) {
        Ok(value) => value,
        Err(_) => return "{}".to_string(),
    };
    // The verdict rides the snapshot; the Shell ANDs it with its own facts.
    let (live, finals) = crate::frame_pane::digests();
    let mut at_baseline = merge_pane_digests(&mut value, snap.realm_at_baseline(), &live, &finals);
    if let Some(lane) = crate::frame_pane::raster::snapshot() {
        at_baseline &= lane["active"].as_u64() == Some(0) && lane["queued"].as_u64() == Some(0);
        value["rasterLane"] = lane;
    }
    value["atBaseline"] = serde_json::Value::Bool(at_baseline);
    serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_string())
}

/// A JSON number, integral when it is one (the counters stay integers).
fn number(n: f64) -> serde_json::Value {
    if n.fract() == 0.0 && n >= 0.0 {
        serde_json::json!(n as u64)
    } else {
        serde_json::json!(n)
    }
}

/// The document counters a pane realm keeps, summed into the host's.
const PANE_COUNTERS: &[&str] = &[
    "readerRuntimesCreated",
    "readerDisposesCompleted",
    "virtualizersCreated",
    "virtualizersDisposed",
];

/// The gauges a live pane frame reports, summed into the host's digest.
const PANE_GAUGES: &[&str] = &[
    "virtualizerLive",
    "liveWindowItems",
    "retainedVirtualItems",
    "virtualizerListeners",
    "virtualizerObservers",
    "virtualizerTimers",
    "lookaheadSamplesActive",
];

/// Fold the pane frames' digests into the host's: counters, gauges, engine.
fn merge_pane_digests(
    value: &mut serde_json::Value,
    host_baseline: bool,
    live: &[String],
    finals: &[String],
) -> bool {
    use serde_json::Value;
    let mut baseline = host_baseline;
    if live
        .iter()
        .chain(finals)
        .any(|json| serde_json::from_str::<Value>(json).is_err())
    {
        baseline = false;
    }
    let parse = |json: &String| serde_json::from_str::<Value>(json).ok();
    let live: Vec<Value> = live.iter().filter_map(parse).collect();
    let finals: Vec<Value> = finals.iter().filter_map(parse).collect();
    if live.is_empty() && finals.is_empty() {
        return baseline;
    }
    let add = |value: &mut Value, key: &str, pane: &Value| {
        let sum = value[key].as_f64().unwrap_or(0.0) + pane[key].as_f64().unwrap_or(0.0);
        value[key] = number(sum);
    };
    for (pane, is_live) in live
        .iter()
        .map(|p| (p, true))
        .chain(finals.iter().map(|p| (p, false)))
    {
        baseline &= pane["atBaseline"].as_bool().unwrap_or(false);
        for key in PANE_COUNTERS {
            add(value, key, pane);
        }
        if is_live {
            for key in PANE_GAUGES {
                add(value, key, pane);
            }
            if pane["readerRuntimeLive"].as_bool() == Some(true) {
                value["readerRuntimeLive"] = Value::Bool(true);
            }
        }
        if let Some(engine) = pane["engine"].as_object() {
            let merged = &mut value["engine"];
            if !merged.is_object() {
                *merged = Value::Object(serde_json::Map::new());
            }
            for (key, field) in engine {
                let slot = &mut merged[key];
                match field {
                    Value::Bool(on) => *slot = Value::Bool(slot.as_bool().unwrap_or(false) || *on),
                    Value::Number(n) => {
                        let sum = slot.as_f64().unwrap_or(0.0) + n.as_f64().unwrap_or(0.0);
                        *slot = number(sum);
                    }
                    other if slot.is_null() => *slot = other.clone(),
                    _ => {}
                }
            }
        }
    }
    let created = value["virtualizersCreated"].as_u64().unwrap_or(0);
    let disposed = value["virtualizersDisposed"].as_u64().unwrap_or(0);
    if disposed > created {
        value["accountingConsistent"] = Value::Bool(false);
        baseline = false;
    }
    baseline
}

/// Reduce closed realms to one bounded record; counters stay monotonic.
pub(crate) fn fold_terminal_digest(total: Option<&str>, terminal: &str) -> String {
    let mut value = match total {
        None => serde_json::json!({ "atBaseline": true }),
        Some(json) => serde_json::from_str::<serde_json::Value>(json)
            .ok()
            .filter(|value| value.is_object())
            .unwrap_or_else(|| serde_json::json!({ "atBaseline": false })),
    };
    let previous = value["atBaseline"].as_bool().unwrap_or(false);
    let baseline = merge_pane_digests(&mut value, previous, &[], &[terminal.to_string()]);
    value["atBaseline"] = serde_json::Value::Bool(baseline);
    serde_json::to_string(&value).unwrap_or_else(|_| "{\"atBaseline\":false}".to_string())
}

/// Push the digest across the boundary now, on the moments waited on.
pub fn publish_digest(api: &dyn runtime_contract::boundary::ShellApi) {
    api.publish_digest(snapshot_json());
}

/// The artifact's own `__mareaderDiagnostics()` probe: a FRESH snapshot, so
/// "active" is true at the read.
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

/// Without a window the probe is a no-op.
#[cfg(not(target_arch = "wasm32"))]
pub fn install() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_and_dispose_notes_move_their_counters() {
        let created_before = READER_RUNTIMES_CREATED.load(Ordering::Relaxed);
        let disposed_before = READER_DISPOSES_COMPLETED.load(Ordering::Relaxed);
        note_reader_runtime_create();
        // A stamp no epoch can match: the assertion is the tail's business.
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
        publish_runtime_view(crate::runtime::RuntimeLifecycle::Ready, 3);
        let value: serde_json::Value =
            serde_json::from_str(&snapshot_json()).expect("snapshot is JSON");
        let runtime = value
            .get("runtime")
            .expect("the runtime view rides the snapshot");
        assert_eq!(runtime["state"], "ready");
        assert_eq!(runtime["generation"], 3);
        // No host installed here: no pane resources are counted.
        assert_eq!(runtime["virtualizerCount"], 0);
        // The runtime half reports the counts the baseline gates on.
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

    /// A synthetic drained snapshot, built by hand: the question is the shape.
    fn drained_engine() -> pdf_core::diagnostics::EngineStats {
        pdf_core::diagnostics::EngineStats::default()
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
            host: None,
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
        // Cannot inspect is not drained: the gate must fail closed.
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
        // A create with no dispose is deliberately not a baseline break.
        let mut snap = drained_snapshot();
        snap.reader_disposes_completed = 6;
        assert!(snap.at_baseline());
    }

    #[test]
    fn an_undrained_engine_breaks_the_baseline() {
        let mut snap = drained_snapshot();
        snap.engine = Some(pdf_core::diagnostics::EngineStats {
            pages: 1,
            ..pdf_core::diagnostics::EngineStats::default()
        });
        assert!(!snap.at_baseline());
        snap.engine = Some(pdf_core::diagnostics::EngineStats {
            prefetches_started: 2,
            prefetches_completed: 1,
            ..pdf_core::diagnostics::EngineStats::default()
        });
        assert!(!snap.at_baseline());
        // And a fully balanced engine half keeps it: the pairing rules hold.
        let mut snap = drained_snapshot();
        snap.engine = Some(pdf_core::diagnostics::EngineStats {
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
            ..pdf_core::diagnostics::EngineStats::default()
        });
        assert!(snap.at_baseline(), "{snap:?}");
    }

    #[test]
    fn no_engine_realms_still_require_every_owned_resource_to_drain() {
        let mut snap = drained_snapshot();
        snap.engine = None;
        assert!(snap.at_baseline_without_engine());
        assert!(!snap.at_baseline());
        snap.virtualizer_live = 1;
        assert!(!snap.at_baseline_without_engine());
        snap.virtualizer_live = 0;
        snap.accounting_consistent = false;
        assert!(!snap.at_baseline_without_engine());
    }

    #[test]
    fn terminal_digests_are_bounded_and_keep_lifetime_counters() {
        let terminal = serde_json::json!({
            "atBaseline": true,
            "readerRuntimesCreated": 1, "readerDisposesCompleted": 1,
            "virtualizersCreated": 2, "virtualizersDisposed": 2,
            "engine": { "sessionsOpened": 1, "sessionsDestroyed": 1, "sessionsLive": 0 }
        })
        .to_string();
        let mut total = None;
        for _ in 0..1000 {
            total = Some(fold_terminal_digest(total.as_deref(), &terminal));
        }
        let json = total.expect("the single aggregate exists");
        assert!(
            json.len() < 512,
            "aggregate grew with closed realms: {}",
            json.len()
        );
        let value: serde_json::Value = serde_json::from_str(&json).expect("aggregate JSON");
        assert_eq!(value["readerRuntimesCreated"], 1000);
        assert_eq!(value["virtualizersDisposed"], 2000);
        assert_eq!(value["engine"]["sessionsOpened"], 1000);
        assert_eq!(value["engine"]["sessionsLive"], 0);
        assert_eq!(value["atBaseline"], true);
    }

    #[test]
    fn a_bad_final_verdict_cannot_be_laundered_by_a_later_close() {
        let good = "{\"atBaseline\":true}";
        for bad in ["{\"atBaseline\":false}", "not JSON", "null"] {
            let total = fold_terminal_digest(None, bad);
            let total = fold_terminal_digest(Some(&total), good);
            let value: serde_json::Value = serde_json::from_str(&total).expect("aggregate JSON");
            assert_eq!(value["atBaseline"], false, "lost failed report {bad}");
        }
        let bad_previous = fold_terminal_digest(Some("not JSON"), good);
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&bad_previous).unwrap()["atBaseline"],
            false
        );
    }
}
