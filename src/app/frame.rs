//! The runtime frame: the disposable iframe boundary the Shell hosts,
//! bounded at every stage.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use runtime_contract::boundary::LaunchDocument;
use runtime_contract::protocol::{BootStage, RuntimeFrame, RuntimeKind, ShellEnvelope, ShellFrame};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};

/// Which runtime's artifact page the frame boots.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrameKind {
    Library,
    Reader,
}

/// Which half of the host a frame occupies.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrameSlot {
    /// On screen: the runtime the user is looking at. Exactly one at a time.
    Active,
    /// Navigation is booting this realm; no reveal before actual Painted.
    Incoming,
    /// Revealed past: disposing behind the runtime that replaced it.
    Retiring,
}

impl FrameSlot {
    /// The `data-mareader-slot` value CSS and the suites scope on.
    pub const fn attr(self) -> &'static str {
        match self {
            FrameSlot::Active => "active",
            FrameSlot::Incoming => "incoming",
            FrameSlot::Retiring => "retiring",
        }
    }

    /// Whether the frame is on screen.
    pub const fn is_visible(self) -> bool {
        matches!(self, FrameSlot::Active)
    }
}

impl FrameKind {
    /// The artifact page the iframe loads (§1).
    pub const fn page(self) -> &'static str {
        match self {
            FrameKind::Library => "/library.html",
            FrameKind::Reader => "/reader.html",
        }
    }

    /// What the loading card and error state say.
    pub const fn label(self) -> &'static str {
        match self {
            FrameKind::Library => "Library",
            FrameKind::Reader => "Reader",
        }
    }

    /// Machine-readable DOM kind, distinct from the human loading label.
    pub const fn attr(self) -> &'static str {
        match self {
            FrameKind::Library => "library",
            FrameKind::Reader => "reader",
        }
    }

    /// The contract's kind for the `init` handshake.
    pub const fn contract_kind(self) -> RuntimeKind {
        match self {
            FrameKind::Library => RuntimeKind::Library,
            FrameKind::Reader => RuntimeKind::Reader,
        }
    }
}

/// The Shell's boot vocabulary, bridged to boot.rs's runtime naming.
impl From<FrameKind> for crate::app::boot::RuntimeName {
    fn from(kind: FrameKind) -> Self {
        match kind {
            FrameKind::Library => crate::app::boot::RuntimeName::Library,
            FrameKind::Reader => crate::app::boot::RuntimeName::Reader,
        }
    }
}

/// A `ShellApi` boundary message coming up the port.
pub enum FrameVocabulary {
    OpenDocument(Box<LaunchDocument>),
    NavigateLibrary,
    ReadPoint(Box<runtime_contract::boundary::ReadPoint>),
    SaveSettings(Box<reader_core::settings::Settings>),
    SaveCover {
        path: String,
        image: runtime_contract::covers::CoverImage,
    },
    SaveGloss {
        key: String,
        marks: String,
    },
    BakeCover {
        path: String,
    },
    DocStatus(Box<runtime_contract::boundary::DocStatusReport>),
    PublishDigest(String),
    ResolveLaunch {
        request: u64,
        path: String,
    },
}

/// What the driver reports through [`FrameEventHook`].
pub enum FrameEvent {
    /// A handshake stage transition (`Status`).
    Stage(BootStage),
    /// The runtime itself reported a failure (protocol `Failed`).
    Failed {
        stage: BootStage,
        cause: String,
    },
    Boundary(FrameVocabulary),
    /// A message whose generation is not this frame's, kept for the ledger.
    Stale,
}

/// The stage a fatal boundary problem happened at.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrameFatalStage {
    /// The frame never answered the channel offer.
    InitializeTimeout,
    /// The frame answered but never announced `Ready`.
    ReadyTimeout,
    /// Ready arrived but the runtime never announced Painted.
    PaintTimeout,
    /// The runtime reported its own failure (protocol `Failed`).
    RuntimeFailed,
    /// A graceful disposal did not complete inside the forced timeout.
    DisposeTimeout,
}

/// One offered channel's Shell side, kept until the frame answers.
struct Offer {
    port: web_sys::MessagePort,
    listener: Closure<dyn FnMut(web_sys::MessageEvent)>,
}

/// The dispatch channel a driver reports through: the manager's
/// generation-checked events hook (§35).
pub type FrameEventHook = Rc<dyn Fn(u64, FrameEvent)>;

/// The driver's plumbing: everything JS-side the frame interacts with.
pub struct Driver {
    kind: FrameKind,
    generation: u64,
    nonce: String,
    /// Which half of the host this frame is in.
    slot: Cell<FrameSlot>,
    iframe: web_sys::HtmlIFrameElement,
    host: web_sys::Element,
    /// Offer re-posting until first contact.
    offer_ticker: Cell<Option<i32>>,
    offer_ticks: Cell<u32>,
    /// The live offer set: every port1 awaiting first contact.
    offers: RefCell<Vec<Offer>>,
    /// The adopted channel; `None` until first contact.
    lane: RefCell<Option<Offer>>,
    /// Bounded stages, by timer id. Cleared when the stage completes or the
    /// driver tears down.
    ready_timer: Cell<Option<i32>>,
    painted_timer: Cell<Option<i32>>,
    dispose_timer: Cell<Option<i32>>,
    /// Which lifecycle signals have arrived, for "what the forced path
    /// heard".
    saw_contact: Cell<bool>,
    saw_ready: Cell<bool>,
    saw_painted: Cell<bool>,
    saw_dispose_complete: Cell<bool>,
    /// The manager's reporter hook into the driver's event loop.
    events: RefCell<Option<FrameEventHook>>,
    /// The one serialized navigation owns this gate; teardown wakes it.
    ready_waiter: RefCell<Option<js_sys::Function>>,
    ready_pending: RefCell<Option<Result<(), FrameFatalStage>>>,
    paint_waiter: RefCell<Option<js_sys::Function>>,
    paint_pending: RefCell<Option<Result<(), FrameFatalStage>>>,
    dispose_gate: RefCell<Option<js_sys::Function>>,
    dispose_pending: RefCell<Option<Result<(), FrameFatalStage>>>,
    /// What this frame boots with (the Init payload), kept for re-init.
    launch: RefCell<Option<LaunchDocument>>,
    // Incoming metadata may arrive before the parent paint gate.
    boot_status: RefCell<Option<Box<runtime_contract::boundary::DocStatusReport>>>,
    boot_digest: RefCell<Option<String>>,
    torn_down: Cell<bool>,
}

thread_local! {
    static NEXT_GENERATION: Cell<u64> = const { Cell::new(1) };
    /// The live drivers, keyed by their generation, on this page's thread.
    static DRIVERS: RefCell<std::collections::HashMap<u64, Rc<Driver>>> =
        RefCell::new(std::collections::HashMap::new());
}

/// Admit a driver into the page-thread registry.
pub fn register(driver: Rc<Driver>) {
    let generation = driver.generation();
    DRIVERS.with(|drivers| drivers.borrow_mut().insert(generation, driver));
    crate::app::bake::frame_registered(generation);
}

/// The driver that owns `generation`, if it has not been torn down.
pub fn lookup(generation: u64) -> Option<Rc<Driver>> {
    DRIVERS.with(|drivers| drivers.borrow().get(&generation).cloned())
}

/// All document realms across live, incoming and retiring Reader hosts.
#[cfg(target_arch = "wasm32")]
pub fn document_frames_resident() -> usize {
    let Some(document) = window().and_then(|window| window.document()) else {
        return 0;
    };
    let Ok(hosts) = document.query_selector_all("iframe[data-mareader-runtime-frame='reader']")
    else {
        return 0;
    };
    (0..hosts.length())
        .filter_map(|index| hosts.item(index))
        .filter_map(|node| node.dyn_into::<web_sys::HtmlIFrameElement>().ok())
        .filter_map(|frame| frame.content_document())
        .filter_map(|document| document.query_selector_all("iframe.pane-frame").ok())
        .map(|frames| frames.length() as usize)
        .sum()
}

/// Remove a driver from the registry at teardown.
pub fn unregister(generation: u64) {
    DRIVERS.with(|drivers| drivers.borrow_mut().remove(&generation));
    crate::app::bake::cancel_library(generation);
}

/// How many frames of `kind` are in the page right now.
#[cfg(target_arch = "wasm32")]
pub fn resident(kind: FrameKind) -> usize {
    DRIVERS.with(|drivers| {
        drivers
            .borrow()
            .values()
            .filter(|driver| driver.kind() == kind)
            .count()
    })
}

/// Mint the next frame generation; monotonic per Shell document.
pub fn next_generation() -> u64 {
    NEXT_GENERATION.with(|g| {
        let next = g.get();
        g.set(next + 1);
        next
    })
}

fn window() -> Option<web_sys::Window> {
    web_sys::window()
}

/// A random 64-bit nonce, minted from the shell's entropy.
fn nonce() -> String {
    let mut out = String::with_capacity(32);
    for _ in 0..8 {
        let v = (js_sys::Math::random() * 4_294_967_296.0) as u32;
        out.push_str(&format!("{v:08x}"));
    }
    out
}

/// The origin the Shell posts at: exact, never "*".
fn target_origin() -> String {
    window()
        .and_then(|w| w.location().origin().ok())
        .filter(|origin| !origin.is_empty() && origin != "null")
        .unwrap_or_else(|| "*".to_string())
}

/// Build the offered-contact object the frame authenticates.
fn offer_value(generation: u64, nonce: &str) -> JsValue {
    let obj = js_sys::Object::new();
    let kind_key = JsValue::from_str("kind");
    let _ = js_sys::Reflect::set(&obj, &kind_key, &JsValue::from_str("mareader.channel"));
    let _ = js_sys::Reflect::set(
        &obj,
        &JsValue::from_str("generation"),
        &JsValue::from_f64(generation as f64),
    );
    let _ = js_sys::Reflect::set(&obj, &JsValue::from_str("nonce"), &JsValue::from_str(nonce));
    obj.into()
}

/// Serialize the Shell frame into the port's vocabulary.
fn post_shell_frame(lane: &Offer, generation: u64, nonce: &str, body: &ShellFrame) {
    let envelope = ShellEnvelope {
        generation,
        nonce: nonce.to_string(),
        // Owned: the envelope is the contract-tested serde shape.
        body: body.clone(),
    };
    let Ok(json) = serde_json::to_string(&envelope) else {
        return;
    };
    let Ok(obj) = js_sys::JSON::parse(&json) else {
        return;
    };
    let _ = lane.port.post_message(&obj);
}

/// One runtime envelope, decoded off the wire.
fn parse_runtime_event(
    event: &web_sys::MessageEvent,
) -> Option<runtime_contract::protocol::RuntimeEnvelope> {
    let text = event.data().as_string()?;
    serde_json::from_str(&text).ok()
}

impl Driver {
    /// Create the frame element with its boot identity in the URL.
    pub fn new(
        kind: FrameKind,
        host: &web_sys::Element,
        generation: u64,
        slot: FrameSlot,
    ) -> Option<Rc<Self>> {
        let nonce = nonce();
        let src = format!("{}?hosted=1&g={}&n={}", kind.page(), generation, nonce);
        let document = window().and_then(|w| w.document())?;
        let element = document.create_element("iframe").ok()?;
        let iframe: web_sys::HtmlIFrameElement = element.unchecked_into();
        iframe.set_class_name("runtime-frame");
        iframe
            .set_attribute("title", &format!("MAReader {}", kind.label()))
            .ok()?;
        iframe
            .set_attribute("data-mareader-runtime-frame", kind.attr())
            .ok()?;
        iframe
            .set_attribute("data-mareader-generation", &generation.to_string())
            .ok()?;
        iframe
            .set_attribute("data-mareader-slot", slot.attr())
            .ok()?;
        iframe.set_src(&src);
        Some(Rc::new(Self {
            kind,
            generation,
            nonce,
            slot: Cell::new(slot),
            iframe,
            host: host.clone(),
            offer_ticker: Cell::new(None),
            offer_ticks: Cell::new(0),
            offers: RefCell::new(Vec::new()),
            lane: RefCell::new(None),
            ready_timer: Cell::new(None),
            painted_timer: Cell::new(None),
            dispose_timer: Cell::new(None),
            saw_contact: Cell::new(false),
            saw_ready: Cell::new(false),
            saw_painted: Cell::new(false),
            saw_dispose_complete: Cell::new(false),
            events: RefCell::new(None),
            ready_waiter: RefCell::new(None),
            ready_pending: RefCell::new(None),
            paint_waiter: RefCell::new(None),
            paint_pending: RefCell::new(None),
            dispose_gate: RefCell::new(None),
            dispose_pending: RefCell::new(None),
            launch: RefCell::new(None),
            boot_status: RefCell::new(None),
            boot_digest: RefCell::new(None),
            torn_down: Cell::new(false),
        }))
    }

    pub const fn kind(&self) -> FrameKind {
        self.kind
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    pub fn slot(&self) -> FrameSlot {
        self.slot.get()
    }

    /// Reveal the fresh incoming realm only after its Ready/Painted gates.
    pub fn reveal(&self) {
        self.set_slot(FrameSlot::Active);
    }

    /// The Shell has just admitted this incoming generation.
    pub fn publish_boot_metadata(&self) {
        let status = self.boot_status.borrow_mut().take();
        if let Some(report) = status {
            self.report(FrameEvent::Boundary(FrameVocabulary::DocStatus(report)));
        }
        let digest = self.boot_digest.borrow_mut().take();
        if let Some(json) = digest {
            self.report(FrameEvent::Boundary(FrameVocabulary::PublishDigest(json)));
        }
    }

    /// Hide a displaced frame the instant its replacement is revealed.
    pub fn begin_retiring(&self) {
        self.set_slot(FrameSlot::Retiring);
        if self.kind == FrameKind::Library {
            crate::app::bake::cancel_library(self.generation);
        }
    }

    fn set_slot(&self, slot: FrameSlot) {
        self.slot.set(slot);
        let _ = self.iframe.set_attribute("data-mareader-slot", slot.attr());
    }

    /// Insert the frame and start the handshake.
    pub fn start(self: &Rc<Self>, launch: Option<LaunchDocument>, events: FrameEventHook) {
        *self.events.borrow_mut() = Some(events);
        *self.launch.borrow_mut() = launch;
        // The frame enters the host as the ONLY permanent child.
        let _ = self.host.append_child(self.iframe.as_ref());
        self.post_offer();
        self.start_offer_ticker();
        self.arm_ready_timer();
        self.report(FrameEvent::Stage(BootStage::Loading));
    }

    /// Frame-side facts and stage moves, never the manager's decisions.
    fn report(&self, event: FrameEvent) {
        if let Some(events) = self.events.borrow().as_ref() {
            events(self.generation, event);
        }
    }

    /// Post one offer at the frame: a fresh `MessageChannel`.
    fn post_offer(self: &Rc<Self>) {
        let (Some(_), Ok(channel)) = (window(), web_sys::MessageChannel::new()) else {
            return;
        };
        let port = channel.port1();
        let weak = Rc::downgrade(self);
        let listener = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |event| {
            let Some(driver) = weak.upgrade() else {
                return;
            };
            driver.dispatch_port(&event);
        });
        port.set_onmessage(Some(listener.as_ref().unchecked_ref()));
        let offer = offer_value(self.generation, &self.nonce);
        let transfer = js_sys::Array::new();
        transfer.push(channel.port2().as_ref());
        let Some(target) = self.iframe.content_window() else {
            // The frame has no window to speak to.
            return;
        };
        // The typed postMessage overload with options has no web-sys feature.
        let Ok(post) = js_sys::Reflect::get(target.as_ref(), &JsValue::from_str("postMessage"))
        else {
            return;
        };
        let post: js_sys::Function = post.unchecked_into();
        let _ = post.call3(
            target.as_ref(),
            &offer,
            &JsValue::from_str(&target_origin()),
            transfer.as_ref(),
        );
        self.offers.borrow_mut().push(Offer { port, listener });
    }

    /// Keep re-offering until the frame answers.
    fn start_offer_ticker(self: &Rc<Self>) {
        let Some(window) = window() else {
            return;
        };
        let weak = Rc::downgrade(self);
        let tick = Closure::<dyn FnMut()>::new(move || {
            let Some(driver) = weak.upgrade() else {
                return;
            };
            if driver.saw_contact.get() || driver.torn_down.get() {
                driver.clear_offer_ticker();
                return;
            }
            let ticks = driver.offer_ticks.get() + 1;
            driver.offer_ticks.set(ticks);
            if ticks > OFFER_TICK_LIMIT {
                driver.clear_offer_ticker();
                driver.fatal_ready(FrameFatalStage::InitializeTimeout);
                return;
            }
            driver.post_offer();
        });
        let Ok(id) = window.set_interval_with_callback_and_timeout_and_arguments_0(
            tick.as_ref().unchecked_ref(),
            OFFER_TICK_MS,
        ) else {
            return;
        };
        self.offer_ticker.set(Some(id));
        tick.into_js_value();
    }

    fn clear_offer_ticker(&self) {
        if let (Some(id), Some(window)) = (self.offer_ticker.take(), window()) {
            window.clear_interval_with_handle(id);
        }
        // Every un-adopted offer dies with its port.
        self.offers.borrow_mut().clear();
    }

    /// The timer bounding "frame inserted" to "Ready announced".
    fn arm_ready_timer(self: &Rc<Self>) {
        let Some(window) = window() else {
            return;
        };
        let weak = Rc::downgrade(self);
        let timer = Closure::<dyn FnMut()>::new(move || {
            let Some(driver) = weak.upgrade() else {
                return;
            };
            driver.ready_timer.set(None);
            if driver.saw_contact.get() {
                driver.fatal_ready(FrameFatalStage::ReadyTimeout);
            } else {
                // No contact at 20 s means the frame's OWN boot never ran.
                driver.fatal_ready(FrameFatalStage::InitializeTimeout);
            }
        });
        let Ok(id) = window.set_timeout_with_callback_and_timeout_and_arguments_0(
            timer.as_ref().unchecked_ref(),
            READY_TIMEOUT_MS,
        ) else {
            return;
        };
        self.ready_timer.set(Some(id));
        timer.into_js_value();
    }

    /// Ready is a mount, not paint.
    fn arm_painted_timeout(self: &Rc<Self>) {
        let Some(window) = window() else {
            return;
        };
        let weak = Rc::downgrade(self);
        let timer = Closure::<dyn FnMut()>::new(move || {
            let Some(driver) = weak.upgrade() else {
                return;
            };
            driver.painted_timer.set(None);
            driver.finish_painted_timeout();
        });
        let Ok(id) = window.set_timeout_with_callback_and_timeout_and_arguments_0(
            timer.as_ref().unchecked_ref(),
            PAINTED_TIMEOUT_MS,
        ) else {
            return;
        };
        self.painted_timer.set(Some(id));
        timer.into_js_value();
    }

    fn finish_painted_timeout(&self) {
        if self.torn_down.get() {
            return;
        }
        self.resolve_painted(Err(FrameFatalStage::PaintTimeout));
        self.fatal_ready(FrameFatalStage::PaintTimeout);
    }

    /// A fatal boundary problem during boot.
    fn fatal_ready(&self, stage: FrameFatalStage) {
        if self.torn_down.get() {
            return;
        }
        self.clear_offer_ticker();
        if let (Some(id), Some(window)) = (self.ready_timer.take(), window()) {
            window.clear_timeout_with_handle(id);
        }
        let cause = fatal_cause(self, stage).into_owned();
        if self.saw_ready.get() {
            self.report(FrameEvent::Failed {
                stage: BootStage::Failed,
                cause,
            });
            return;
        }
        self.resolve_ready(Err(stage));
    }

    /// The port listener's entry point, identity generation-checked first.
    fn dispatch_port(self: &Rc<Self>, event: &web_sys::MessageEvent) {
        let Some(envelope) = parse_runtime_event(event) else {
            return;
        };
        if envelope.generation != self.generation {
            self.report(FrameEvent::Stale);
            return;
        }
        // First contact on THIS port: adopt it as the lane.
        if !self.saw_contact.get() {
            self.saw_contact.set(true);
            self.claim_lane(event);
            // The handshake's next beat belongs to the Shell.
            self.send_init();
        }
        match envelope.body {
            RuntimeFrame::Status { stage } => {
                // A `Disposed` stage needs no branch.
                if stage == BootStage::Failed {
                    // The runtime's failure becomes the shell's error state.
                    self.fatal_ready(FrameFatalStage::RuntimeFailed);
                    return;
                }
                self.report(FrameEvent::Stage(stage));
            }
            RuntimeFrame::Ready => {
                self.try_resolve_ready();
                // Readiness and paint have separate bounded gates.
                if self.paint_pending.borrow().is_none() {
                    self.arm_painted_timeout();
                }
            }
            RuntimeFrame::Painted => {
                self.saw_painted.set(true);
                self.resolve_painted(Ok(()));
                if let (Some(id), Some(window)) = (self.painted_timer.take(), window()) {
                    window.clear_timeout_with_handle(id);
                }
            }
            RuntimeFrame::Failed { stage, cause } => {
                self.report(FrameEvent::Failed { stage, cause });
            }
            RuntimeFrame::DisposeComplete => {
                self.saw_dispose_complete.set(true);
                self.resolve_dispose(Ok(()));
            }
            RuntimeFrame::OpenDocument { launch } => {
                self.report(FrameEvent::Boundary(FrameVocabulary::OpenDocument(launch)));
            }
            RuntimeFrame::NavigateLibrary => {
                self.report(FrameEvent::Boundary(FrameVocabulary::NavigateLibrary));
            }
            RuntimeFrame::ReadPoint { point } => {
                self.report(FrameEvent::Boundary(FrameVocabulary::ReadPoint(point)));
            }
            RuntimeFrame::SaveSettings { settings } => {
                self.report(FrameEvent::Boundary(FrameVocabulary::SaveSettings(
                    settings,
                )));
            }
            RuntimeFrame::SaveCover { path, image } => {
                self.report(FrameEvent::Boundary(FrameVocabulary::SaveCover {
                    path,
                    image,
                }));
            }
            RuntimeFrame::SaveGloss { key, marks } => {
                self.report(FrameEvent::Boundary(FrameVocabulary::SaveGloss {
                    key,
                    marks,
                }));
            }
            RuntimeFrame::BakeCover { path } => {
                self.report(FrameEvent::Boundary(FrameVocabulary::BakeCover { path }));
            }
            RuntimeFrame::DocStatus { report } => {
                if self.slot.get() == FrameSlot::Incoming {
                    *self.boot_status.borrow_mut() = Some(Box::new(report.clone()));
                }
                self.report(FrameEvent::Boundary(FrameVocabulary::DocStatus(Box::new(
                    report,
                ))));
            }
            RuntimeFrame::PublishDigest { json } => {
                if self.slot.get() == FrameSlot::Incoming {
                    *self.boot_digest.borrow_mut() = Some(json.clone());
                }
                self.report(FrameEvent::Boundary(FrameVocabulary::PublishDigest(json)));
            }
            RuntimeFrame::ResolveLaunch { request, path } => {
                self.report(FrameEvent::Boundary(FrameVocabulary::ResolveLaunch {
                    request,
                    path,
                }));
            }
        }
    }

    /// Adopt the port that first contact arrived on.
    fn claim_lane(&self, event: &web_sys::MessageEvent) {
        let target = event.target();
        let mut chosen: Option<Offer> = None;
        let mut keep: Vec<Offer> = Vec::new();
        for offer in self.offers.borrow_mut().drain(..) {
            let is_target = target
                .as_ref()
                .map(|t| {
                    let target_js: &JsValue = t.as_ref();
                    offer.port.as_ref() as &JsValue == target_js
                })
                .unwrap_or(false);
            if chosen.is_none() && is_target {
                chosen = Some(offer);
            } else {
                keep.push(offer);
            }
        }
        self.clear_offer_ticker();
        if let Some(offer) = chosen {
            offer.port.start();
            *self.lane.borrow_mut() = Some(offer);
        }
    }

    /// The frame's `init`: identity and, for the reader, its launch.
    fn send_init(&self) {
        let lane = self.lane.borrow();
        let Some(lane) = lane.as_ref() else {
            return;
        };
        post_shell_frame(
            lane,
            self.generation,
            &self.nonce,
            &ShellFrame::Init {
                runtime: self.kind.contract_kind(),
                launch: self
                    .launch
                    .borrow()
                    .as_ref()
                    .map(|launch| Box::new(launch.clone())),
                // Incoming frames paint without starting Library scans.
                hidden: self.slot.get() != FrameSlot::Active,
            },
        );
    }

    /// Send a shell frame over the live lane.
    pub fn send(&self, body: &ShellFrame) {
        let lane = self.lane.borrow();
        if let Some(lane) = lane.as_ref() {
            post_shell_frame(lane, self.generation, &self.nonce, body);
        }
    }

    /// The boot verdict promise.
    pub fn wait_verdict(&self) -> js_sys::Promise {
        js_sys::Promise::new(&mut |resolve, _reject| {
            if self.ready_pending.borrow().is_some() {
                let _ = resolve.call0(&JsValue::NULL);
            } else {
                assert!(
                    self.ready_waiter.borrow_mut().replace(resolve).is_none(),
                    "one navigation owns Ready"
                );
            }
        })
    }

    /// Every handoff awaits this gate as well as Ready.
    pub fn wait_painted(&self) -> js_sys::Promise {
        js_sys::Promise::new(&mut |resolve, _reject| {
            if self.paint_pending.borrow().is_some() {
                let _ = resolve.call0(&JsValue::NULL);
            } else {
                assert!(
                    self.paint_waiter.borrow_mut().replace(resolve).is_none(),
                    "one navigation owns Painted"
                );
            }
        })
    }

    pub fn paint_outcome(&self) -> Option<Result<(), FrameFatalStage>> {
        *self.paint_pending.borrow()
    }

    fn resolve_painted(&self, outcome: Result<(), FrameFatalStage>) {
        if self.paint_pending.borrow().is_some() {
            return;
        }
        *self.paint_pending.borrow_mut() = Some(outcome);
        if let Some(resolve) = self.paint_waiter.borrow_mut().take() {
            let _ = resolve.call0(&JsValue::NULL);
        }
    }

    /// Read the resolved boot gate without consuming its verdict.
    pub fn ready_outcome(&self) -> Option<Result<(), FrameFatalStage>> {
        *self.ready_pending.borrow()
    }

    fn try_resolve_ready(&self) -> bool {
        if self.saw_ready.replace(true) {
            return false;
        }
        if let (Some(id), Some(window)) = (self.ready_timer.take(), window()) {
            window.clear_timeout_with_handle(id);
        }
        self.resolve_ready(Ok(()));
        true
    }

    /// Publish the boot verdict and wake its navigation.
    fn resolve_ready(&self, outcome: Result<(), FrameFatalStage>) {
        *self.ready_pending.borrow_mut() = Some(outcome);
        if let Some(resolve) = self.ready_waiter.borrow_mut().take() {
            let _ = resolve.call0(&JsValue::NULL);
        }
    }

    /// Phase 1 of disposal, from the Shell's side: ask, then wait.
    pub fn grace_dispose(self: &Rc<Self>) -> js_sys::Promise {
        self.send(&ShellFrame::Dispose);
        if let Some(window) = window() {
            let weak = Rc::downgrade(self);
            let timer = Closure::<dyn FnMut()>::new(move || {
                let Some(driver) = weak.upgrade() else {
                    return;
                };
                driver.dispose_timer.set(None);
                driver.resolve_dispose(Err(FrameFatalStage::DisposeTimeout));
            });
            if let Ok(id) = window.set_timeout_with_callback_and_timeout_and_arguments_0(
                timer.as_ref().unchecked_ref(),
                DISPOSE_TIMEOUT_MS,
            ) {
                self.dispose_timer.set(Some(id));
                timer.into_js_value();
            }
        }
        js_sys::Promise::new(&mut |resolve, _reject| {
            self.dispose_gate.borrow_mut().replace(resolve);
        })
    }

    /// The awaited disposal verdict: `Ok` or the forced path.
    pub fn take_dispose_outcome(&self) -> Option<Result<(), FrameFatalStage>> {
        self.dispose_pending.borrow_mut().take()
    }

    fn resolve_dispose(&self, outcome: Result<(), FrameFatalStage>) {
        if outcome.is_ok()
            && let (Some(id), Some(window)) = (self.dispose_timer.take(), window())
        {
            window.clear_timeout_with_handle(id);
        }
        *self.dispose_pending.borrow_mut() = Some(outcome);
        if let Some(resolve) = self.dispose_gate.borrow_mut().take() {
            let _ = resolve.call0(&JsValue::NULL);
        }
    }

    /// Tear down everything JS-side: timers, offers, listeners, the element.
    pub fn teardown(&self) {
        unregister(self.generation);
        if self.torn_down.replace(true) {
            return;
        }
        if self.ready_pending.borrow().is_none() {
            self.resolve_ready(Err(FrameFatalStage::RuntimeFailed));
        }
        self.resolve_painted(Err(FrameFatalStage::RuntimeFailed));
        for cell in [&self.ready_timer, &self.painted_timer, &self.dispose_timer] {
            if let (Some(id), Some(window)) = (cell.take(), window()) {
                window.clear_timeout_with_handle(id);
            }
        }
        self.clear_offer_ticker();
        if let Some(lane) = self.lane.borrow_mut().take() {
            lane.port.set_onmessage(None);
            lane.port.close();
            drop(lane.listener);
        }
        if let Some(parent) = self.iframe.parent_node() {
            let _ = parent.remove_child(self.iframe.as_ref());
        }
        if self.kind == FrameKind::Reader {
            // A dying host must not leave descendants holding window leases.
            if let Some(window) = window()
                && let Ok(lane) = js_sys::Reflect::get(&window, &"__mareaderRasterLane".into())
                && let Ok(method) = js_sys::Reflect::get(&lane, &"retireScope".into())
                && let Ok(retire) = method.dyn_into::<js_sys::Function>()
            {
                let scope = format!("reader:{}:{}", self.generation, self.nonce);
                let _ = retire.call1(&lane, &scope.into());
            }
        }
    }
}

impl Drop for Driver {
    fn drop(&mut self) {
        self.teardown();
    }
}

/// The bounded-contact budget: one offer per 300 ms, 60 s.
const OFFER_TICK_MS: i32 = 300;
const OFFER_TICK_LIMIT: u32 = 200;
/// The frame's whole boot, insert to `Ready`, has 20 s.
const READY_TIMEOUT_MS: i32 = 20_000;
/// `Painted` after `Ready`: two animation frames plus render jitter.
const PAINTED_TIMEOUT_MS: i32 = 2_500;
/// §12's forced removal: phase 1 gets 8 s, then the Shell takes phase 2.
const DISPOSE_TIMEOUT_MS: i32 = 8_000;

/// What the forced path heard before the frame was taken down.
pub fn heard_summary(driver: &Driver) -> String {
    format!(
        "contact={} ready={} painted={} disposed={}",
        driver.saw_contact.get(),
        driver.saw_ready.get(),
        driver.saw_painted.get(),
        driver.saw_dispose_complete.get()
    )
}

pub(crate) fn fatal_cause(
    driver: &Driver,
    stage: FrameFatalStage,
) -> std::borrow::Cow<'static, str> {
    let detail = heard_summary(driver);
    let (page, script) = match driver.kind() {
        FrameKind::Library => ("/library.html", "/library.js"),
        FrameKind::Reader => ("/reader.html", "/reader.js"),
    };
    match stage {
        FrameFatalStage::InitializeTimeout => format!(
            "the {} frame never answered its channel offer — {page} is missing, {script} \
             failed, or the boot never reached it ({detail})",
            driver.kind().label()
        )
        .into(),
        FrameFatalStage::PaintTimeout => format!(
            "the {} frame answered Ready but never announced Painted ({detail})",
            driver.kind().label()
        )
        .into(),
        FrameFatalStage::ReadyTimeout => {
            format!("the frame spoke but did not announce Ready within 20 s ({detail})").into()
        }
        FrameFatalStage::RuntimeFailed => "the runtime reported its own failure".into(),
        FrameFatalStage::DisposeTimeout => {
            format!("the frame did not finish its graceful disposal within 8 s ({detail})").into()
        }
    }
}

impl FrameFatalStage {
    /// The boot.rs stage the error card quotes.
    pub fn boot_stage(self) -> crate::app::boot::BootStage {
        match self {
            FrameFatalStage::InitializeTimeout => crate::app::boot::BootStage::ModuleLoad,
            FrameFatalStage::ReadyTimeout | FrameFatalStage::PaintTimeout => {
                crate::app::boot::BootStage::Start
            }
            FrameFatalStage::RuntimeFailed => crate::app::boot::BootStage::Start,
            FrameFatalStage::DisposeTimeout => crate::app::boot::BootStage::Dispose,
        }
    }
}

/// The protocol's stage as the shell's error-stage vocabulary.
pub fn protocol_boot_stage(stage: BootStage) -> crate::app::boot::BootStage {
    match stage {
        BootStage::Loading | BootStage::Initialized => crate::app::boot::BootStage::Init,
        BootStage::Mounted | BootStage::Ready | BootStage::Failed => {
            crate::app::boot::BootStage::Start
        }
        BootStage::Disposing | BootStage::Disposed => crate::app::boot::BootStage::Dispose,
    }
}

/// One word from the protocol stage for the visible detail line.
pub const fn protocol_stage_label(stage: BootStage) -> &'static str {
    match stage {
        BootStage::Loading => "loading",
        BootStage::Initialized => "initialized",
        BootStage::Mounted => "mounted",
        BootStage::Ready => "ready",
        BootStage::Disposing => "disposing",
        BootStage::Disposed => "disposed",
        BootStage::Failed => "failed",
    }
}

#[cfg(test)]
mod tests {
    use super::FrameKind;

    #[test]
    fn dom_kind_markers_are_lower_case() {
        assert_eq!(FrameKind::Library.attr(), "library");
        assert_eq!(FrameKind::Reader.attr(), "reader");
    }
}
