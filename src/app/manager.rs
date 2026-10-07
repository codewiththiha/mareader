//! The Shell's route and lifecycle authority over disposable realms.

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

/// The active-runtime slot, keeping the frame's GENERATION.
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
    /// What the runtime host is showing, in the probe's plain form.
    pub boot_state: Mutex<String>,
    pub boot_error: Mutex<Option<serde_json::Value>>,
    /// Create/dispose counts for the diagnostics identity.
    pub reader_sessions_created: std::sync::atomic::AtomicU64,
    pub reader_disposes_completed: std::sync::atomic::AtomicU64,
    pub library_sessions_created: std::sync::atomic::AtomicU64,
    pub library_disposes_completed: std::sync::atomic::AtomicU64,
    host: Mutex<Option<web_sys::Element>>,
    /// Starts are serialized: a navigation mid-start becomes pending.
    starting: std::sync::atomic::AtomicBool,
    pending: Mutex<Option<(RuntimeName, Option<LaunchDocument>)>>,
    /// The cold incoming frame, cancellable before Ready and Painted.
    incoming: Mutex<Option<u64>>,
    /// The last reader digest, cached for the probe.
    pub last_digest: Mutex<Option<serde_json::Value>>,
    pub doc_status: Mutex<String>,
    pub doc_error: Mutex<Option<String>>,
    /// The stale-message ledger: a ledger, not a gate.
    pub stale_frames_seen: std::sync::atomic::AtomicU64,
    /// The reader session the Shell has handed a document to.
    reader_launched: Mutex<Option<u64>>,
    reader_armed: Mutex<Option<u64>>,
}

thread_local! {
    /// The shell state, attached once the Shell component exists.
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

    /// Attach the shell state, once, before the first boot.
    pub fn attach_state(&self, state: ShellState) {
        SHELL_STATE.with(|slot| *slot.borrow_mut() = Some(state));
    }

    /// A handle on the shared manager.
    fn handle(&self) -> Option<std::sync::Arc<RuntimeManager>> {
        SHELL_STATE.with(|slot| slot.borrow().as_ref().map(|state| state.manager.clone()))
    }

    /// Publish a boot phase, state and error as one pair.
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

    /// How many reader frames are in the page, whatever their slot.
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

    /// One start at a time: two starts never interleave their awaits.
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

    /// An open inside Reader replaces a document realm.
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

        // A reader on screen takes a new document over its own port.
        if self.active() == Some(ActiveRuntime::Reader) && runtime == RuntimeName::Reader {
            if let (Some(document), Some(driver)) = (launch, self.live_driver()) {
                driver.send(&ShellFrame::Launch {
                    document: Box::new(document),
                });
                self.note_launch(Some(driver.generation()));
            }
            // Cancelling an incoming Library returns to the Reader; restore its
            // phase.
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

    /// A fresh realm without sacrificing the outgoing pixels.
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

    /// The dispatch closure every driver reports through.
    fn events_hook(&self) -> crate::app::frame::FrameEventHook {
        let state = SHELL_STATE.with(|slot| slot.borrow().clone());
        let Some(state) = state else {
            return Rc::new(|_generation, _event| {});
        };
        Rc::new(move |generation, event| {
            let manager = state.manager.clone();
            match event {
                FrameEvent::Stage(stage) => {
                    // Frame-side telemetry: every stage lands in the console.
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
                    // The error state is the outcome: nothing half-mounted.
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
                    // The page placeholder covers the window until first paint.
                    boot::uncover_page();
                    if let Some(host) = manager.host() {
                        boot::clear_boot(&host);
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

    /// A failed start: the host paints the error state.
    fn fail(&self, error: BootError) {
        if let Some(driver) = self.live_driver() {
            driver.begin_retiring();
            self.retire(driver.generation());
            *self.slot.lock().unwrap() = Slot::None;
        }
        boot::uncover_page();
        match self.host() {
            Some(host) => {
                boot::clear_boot(&host);
                boot::paint_error(&host, &error);
            }
            None => web_sys::console::error_1(&JsValue::from_str(&error.console_line())),
        }
        self.set_phase(BootPhase::Failed(error));
    }

    // -----------------------------------------------------------------
    // Retirement
    // -----------------------------------------------------------------

    /// Take the runtime the user just left off the critical path.
    fn retire(&self, generation: u64) {
        let Some(manager) = self.handle() else {
            return;
        };
        wasm_bindgen_futures::spawn_local(async move {
            manager.run_retire(generation).await;
        });
    }

    /// Graceful disposal first, forced removal after the strict timeout.
    async fn run_retire(&self, generation: u64) {
        let Some(driver) = crate::app::frame::lookup(generation) else {
            return;
        };
        // Nothing is disposed while it is still on screen.
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
        // A launch with no path would mount a reader that shows nothing.
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

    /// Files dropped from the OS: imported by the LIBRARY, never the reader.
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

    /// Whether the library is the runtime on screen.
    #[cfg(target_arch = "wasm32")]
    pub fn library_on_screen(&self) -> bool {
        matches!(&*self.slot.lock().unwrap(), Slot::Library { .. })
    }

    /// A reader handback: reveal the library, retire the reader.
    pub fn navigate_library(&self, state: &ShellState) {
        navigate("/");
        self.start_library(state);
    }

    /// The frame-dispatched boundary vocabulary of a boot.
    fn dispatch_boundary(&self, state: &ShellState, generation: u64, item: FrameVocabulary) {
        // Only an incoming or active Library owns cover work.
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
        // A retiring frame still gets its terminal word in.
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
                // The Shell paints its own document: follow the runtime's edit.
                let _ = leptos::prelude::Set::try_set(&state.settings, *settings);
            }
            FrameVocabulary::SaveCover { path, image } => {
                crate::services::save_cover(&path, image);
            }
            // Only a reader glosses; a list from any other kind is refused.
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
                    // The session showed the book: Idle means it goes away.
                    *self.reader_armed.lock().unwrap() = Some(generation);
                }
                let armed = *self.reader_armed.lock().unwrap() == Some(generation);
                // Idle means "the book is gone" for a reader that opened it.
                if live && armed && report.status == "Idle" {
                    self.note_launch(None);
                    self.navigate_library(state);
                }
                // The document's truth, printed the moment it changes.
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

    /// Record which reader session was handed a document.
    fn note_launch(&self, generation: Option<u64>) {
        *self.reader_launched.lock().unwrap() = generation;
        *self.reader_armed.lock().unwrap() = None;
    }
}

impl RuntimeManager {
    /// The live driver, resolved from the slot's generation.
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

/// One honest line to the native host's terminal.
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

/// History carries navigation metadata, never rasters or runtimes.
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

/// A forward-history Reader entry keeps a plain launch descriptor.
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

/// History-API navigation: two paths, `/` and `/reader`.
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
