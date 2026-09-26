//! The runtime frame: the disposable execution boundary the Shell hosts (§1,
//! §5–§12). A runtime is an iframed artifact page (`/library.html`,
//! `/reader.html`) with a boot marker in its URL (`?hosted=1&g=<generation>
//! &n=<nonce>`), adopted over a dedicated `MessageChannel` port the Shell
//! re-offers until the frame answers, and put down in two phases — the
//! runtime's `DisposeComplete` first, the iframe removal after §12's phase 2,
//! forced after a strict timeout.
//!
//! The Shell's invariants live in the type rather than in memory:
//!
//! * The URL is the boot descriptor (§6): the frame either fully claims a
//!   hosted boot with the identity the Shell will echo, or it boots
//!   standalone — never "hosted at the syntax level".
//! * Every envelope travelling the port carries the frame's generation
//!   (§8): a stale iframe — from a replaced session, a crashed one, a
//!   half-disposed one — cannot mutate the current state, because its
//!   generation is not the live one and both directions of the port check it.
//! * The nonce authenticates the channel establishment itself (§8): it
//!   exists only in the URL and in the Shell's memory, and the frame proves
//!   it heard its own address by echoing the gen back on the port.
//! * A stage never runs without a bound (§6, §11): offer retries stop at
//!   60 s, `Ready` must arrive inside 20 s of the frame's insertion,
//!   `Painted` inside 15 s of `Ready`, and a disposal has 8 s to complete
//!   phase 1 before the Shell takes phase 2 into its own hands. Every
//!   timeout becomes the runtime's visible error state — never a silent
//!   blank, never a fallback (§11).
//!
//! What this module is NOT: a loader of code. The artifact pages are the
//! deployment layout the build owns, and the iframe is where their own
//! module resolution happens — the Shell neither imports `/library.js` nor
//! caches its module map (§3's "the layout each deployment stands alone
//! under" — and §30, once green: no query-string-per-session imports).

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

    /// The contract's kind for the `init` handshake.
    pub const fn contract_kind(self) -> RuntimeKind {
        match self {
            FrameKind::Library => RuntimeKind::Library,
            FrameKind::Reader => RuntimeKind::Reader,
        }
    }
}

/// The Shell's boot vocabulary, bridged to boot.rs's runtime naming: the
/// loading card and the boot-error state keep speaking in artifact names.
impl From<FrameKind> for crate::app::boot::RuntimeName {
    fn from(kind: FrameKind) -> Self {
        match kind {
            FrameKind::Library => crate::app::boot::RuntimeName::Library,
            FrameKind::Reader => crate::app::boot::RuntimeName::Reader,
        }
    }
}

/// A `ShellApi` boundary message coming up the port, lifted out of the
/// protocol enum so the manager's dispatch reads in vocabulary terms.
pub enum FrameVocabulary {
    OpenDocument(Box<LaunchDocument>),
    NavigateLibrary,
    ReadPoint(Box<runtime_contract::boundary::ReadPoint>),
    SaveSettings(Box<reader_core::settings::Settings>),
    SaveLibrary(Box<library_core::blob::LibraryBlob>),
    SaveCovers(Box<runtime_contract::covers::CoverMap>),
    SaveCover {
        path: String,
        image: runtime_contract::covers::CoverImage,
    },
    BakeCover {
        path: String,
    },
    DocStatus(Box<runtime_contract::boundary::DocStatusReport>),
    PublishDigest(String),
    Reload,
    ResolveLaunch {
        request: u64,
        path: String,
    },
}

/// What the driver reports through [`FrameEvents`]. The manager's closure
/// decides; the driver's own generation stamps every item, so nothing a
/// stale frame says can present as current.
pub enum FrameEvent {
    /// The frame spoke on the port for the first time and adopted a channel.
    Contact,
    /// A handshake stage transition (`Status`).
    Stage(BootStage),
    Ready,
    Painted,
    /// The runtime itself reported a failure (protocol `Failed`).
    Failed {
        stage: BootStage,
        cause: String,
    },
    /// §12 phase 1 acknowledged — the iframe may come down.
    DisposeComplete,
    Boundary(FrameVocabulary),
    /// A message whose generation is not this frame's — kept for the
    /// diagnostics ledger, never applied (§35).
    Stale,
}

/// The stage a fatal boundary problem happened at. Mapped into boot.rs's
/// public stage names so the error card keeps speaking the same language it
/// always did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FrameFatalStage {
    /// The frame never answered the channel offer (its document did not
    /// load, or its script never ran): the frame-equivalent of the artifact
    /// load / wasm init stages.
    InitializeTimeout,
    /// The frame answered but never announced `Ready`.
    ReadyTimeout,
    /// The runtime reported its own failure (protocol `Failed`).
    RuntimeFailed,
    /// A graceful disposal did not complete inside the forced timeout — the
    /// removal proceeds by force, and the boot report names it (§12,
    /// "what the forced path heard").
    DisposeTimeout,
}

/// One offered channel's Shell side. Kept until the frame answers (only one
/// of the retry offers is ever answered; the rest are closed when the lane
/// is claimed or the driver tears down).
struct Offer {
    port: web_sys::MessagePort,
    listener: Closure<dyn FnMut(web_sys::MessageEvent)>,
}

/// The dispatch channel a driver reports through: the manager's
/// generation-checked events hook (§35).
pub type FrameEventHook = Rc<dyn Fn(u64, FrameEvent)>;

/// The driver's plumbing — everything JS-side the frame interacts with, in
/// `RefCell`s because the listener closures arrive on the event loop while
/// the manager's awaits are suspended elsewhere.
pub struct Driver {
    kind: FrameKind,
    generation: u64,
    nonce: String,
    iframe: web_sys::HtmlIFrameElement,
    host: web_sys::Element,
    /// Offer re-posting until first contact (§7's "re-init must be
    /// idempotent": the Shell keeps addressing the same identity — same
    /// generation, same nonce, fresh `MessageChannel` — and the frame's
    /// adoption of whichever copy it actually got is which one its answers
    /// arrive on).
    offer_ticker: Cell<Option<i32>>,
    offer_ticks: Cell<u32>,
    /// The live offer set: every port1 awaiting first contact. Once the
    /// lane is claimed only the adopted port survives; the rest are
    /// discarded with their listeners.
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
    /// The oneshot gates the manager's awaits resolve through.
    ready_gate: RefCell<Option<js_sys::Function>>,
    ready_pending: RefCell<Option<Result<(), FrameFatalStage>>>,
    dispose_gate: RefCell<Option<js_sys::Function>>,
    dispose_pending: RefCell<Option<Result<(), FrameFatalStage>>>,
    /// What this frame boots with (the Init payload), kept for re-init.
    launch: RefCell<Option<LaunchDocument>>,
    torn_down: Cell<bool>,
}

thread_local! {
    static NEXT_GENERATION: Cell<u64> = const { Cell::new(1) };
    /// The live drivers, keyed by their generation. NOT a manager field: a
    /// driver is `Rc`/DOM plumbing and the manager sits behind `Arc` in the
    /// shell state (Leptos context demands `Send + Sync`), so the drivers
    /// live on the one thread the manager's awaits and the frame's events
    /// all run on — this page's. The manager keeps only the generation
    /// number, and every lookup resolves it here.
    static DRIVERS: RefCell<std::collections::HashMap<u64, Rc<Driver>>> =
        RefCell::new(std::collections::HashMap::new());
}

/// Admit a driver into the page-thread registry (at creation).
pub fn register(driver: Rc<Driver>) {
    DRIVERS.with(|drivers| drivers.borrow_mut().insert(driver.generation(), driver));
}

/// The driver that owns `generation`, if it has not been torn down.
pub fn lookup(generation: u64) -> Option<Rc<Driver>> {
    DRIVERS.with(|drivers| drivers.borrow().get(&generation).cloned())
}

/// Remove a driver from the registry (at teardown). Idempotent: a forced
/// and a graceful path can both reach it for one generation.
pub fn unregister(generation: u64) {
    DRIVERS.with(|drivers| drivers.borrow_mut().remove(&generation));
}

/// Mint the next frame generation: monotonic while this Shell documents
/// lives, so `$g=1` in the browser suite's evidence means "the first frame
/// this shell ever hosted" and a jump in the number names a replacement.
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

/// A random 64-bit nonce. `Math.random` is where ALL entropy in this shell
/// comes from today (services' rng); the marker is for marking live identities,
/// not for proving secrecy across origins (§8's attack surface is the stale
/// iframe inside the SAME shell document).
fn nonce() -> String {
    let mut out = String::with_capacity(32);
    for _ in 0..8 {
        let v = (js_sys::Math::random() * 4_294_967_296.0) as u32;
        out.push_str(&format!("{v:08x}"));
    }
    out
}

/// The origin the Shell posts at (§8: exact, never "*"). Tauri's webview
/// serves from a custom origin whose string is the only authority for "the
/// frame this Shell owns".
fn target_origin() -> String {
    window()
        .and_then(|w| w.location().origin().ok())
        .filter(|origin| !origin.is_empty() && origin != "null")
        .unwrap_or_else(|| "*".to_string())
}

/// Build the offered-contact object the frame's `match_offer` authenticates:
/// `{kind, generation, nonce}` (frame_transport::CHANNEL_KIND is the kind,
/// and the frame's own URL is the only place outside this driver where the
/// nonce exists — §8).
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

/// Serialize the Shell frame into the port's vocabulary: the frame parses
/// the envelope with `serde_json` against a `JSON.stringify`d event data,
/// so posting the OBJECT is what each direction reads (the runtime posts
/// its own envelopes as compact strings; both directions flatten the same
/// `serde` shape).
fn post_shell_frame(lane: &Offer, generation: u64, nonce: &str, body: &ShellFrame) {
    let envelope = ShellEnvelope {
        generation,
        nonce: nonce.to_string(),
        body: match body {
            // The borrowed form cannot skip the serialise round trip:
            // envelope's Owned body is what the contract test round-trips.
            ShellFrame::Init { runtime, launch } => ShellFrame::Init {
                runtime: *runtime,
                launch: launch.clone(),
            },
            ShellFrame::Launch { document } => ShellFrame::Launch {
                document: document.clone(),
            },
            ShellFrame::Dispose => ShellFrame::Dispose,
            ShellFrame::CoverBaked { path, image } => ShellFrame::CoverBaked {
                path: path.clone(),
                image: image.clone(),
            },
            ShellFrame::ResolveLaunchAnswer { request, document } => {
                ShellFrame::ResolveLaunchAnswer {
                    request: *request,
                    document: document.clone(),
                }
            }
        },
    };
    let Ok(json) = serde_json::to_string(&envelope) else {
        return;
    };
    let Ok(obj) = js_sys::JSON::parse(&json) else {
        return;
    };
    let _ = lane.port.post_message(&obj);
}

/// One runtime envelope, decoded off the wire. The runtime posts compact
/// JSON STRINGS (frame_transport's wire is `post_json`), so anything whose
/// data is not a string was not our frame — the source check already bound
/// the window, this binds the shape.
fn parse_runtime_event(
    event: &web_sys::MessageEvent,
) -> Option<runtime_contract::protocol::RuntimeEnvelope> {
    let text = event.data().as_string()?;
    serde_json::from_str(&text).ok()
}

impl Driver {
    /// Create the frame element with its boot identity in the URL. The
    /// driver is inert until [`Driver::start`].
    pub fn new(kind: FrameKind, host: &web_sys::Element, generation: u64) -> Option<Rc<Self>> {
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
            .set_attribute("data-mareader-runtime-frame", kind.label())
            .ok()?;
        iframe
            .set_attribute("data-mareader-generation", &generation.to_string())
            .ok()?;
        iframe.set_src(&src);
        Some(Rc::new(Self {
            kind,
            generation,
            nonce,
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
            ready_gate: RefCell::new(None),
            ready_pending: RefCell::new(None),
            dispose_gate: RefCell::new(None),
            dispose_pending: RefCell::new(None),
            launch: RefCell::new(None),
            torn_down: Cell::new(false),
        }))
    }

    pub const fn kind(&self) -> FrameKind {
        self.kind
    }

    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Insert the frame and start the handshake. The loading cover is the
    /// caller's business and is ALREADY painted (§11 — covered before the
    /// first await); this driver's insertion is the first thing behind it.
    pub fn start(self: &Rc<Self>, launch: Option<LaunchDocument>, events: FrameEventHook) {
        *self.events.borrow_mut() = Some(events);
        *self.launch.borrow_mut() = launch;
        // The frame enters the host as the ONLY permanent child (§1's "only
        // the iframe holds focus"): the boot card overlays it until Painted.
        let _ = self.host.append_child(self.iframe.as_ref());
        self.post_offer();
        self.start_offer_ticker();
        self.arm_ready_timer();
        self.emit(FrameEvent::Stage(BootStage::Loading));
    }

    fn report(&self, event: FrameEvent) {
        if let Some(events) = self.events.borrow().as_ref() {
            events(self.generation, event);
        }
    }

    /// Frame-side facts and stage moves, never the manager's decisions. The
    /// manager decides what a signal means for the CURRENT frame; the driver
    /// simply cannot emit for a generation that is not its own.
    fn emit(&self, event: FrameEvent) {
        self.report(event);
    }

    /// Post one offer at the frame: a fresh `MessageChannel` whose port1
    /// gets the Shell's listener and whose port2 crosses in the message.
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
            // The frame has no window to speak to (document never built):
            // the ticker will keep the offer alive until the bound expires.
            return;
        };
        // postMessage(message, targetOrigin, transfer-for-ports): the typed
        // overload with an options dict has no web-sys feature in the pinned
        // version, so the call goes through reflect — the origin stays the
        // frame's own, and the offer object is the only thing that crosses.
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

    /// Keep re-offering until the frame answers — §7's idempotent re-init at
    /// the transport level, bounded at 60 s (§6's never-unbounded rule).
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
        // Every un-adopted offer dies with its port: the frame adopted at
        // most one, and the rest only ever received OUR offer (never a
        // message from the frame), so nothing in them is live traffic.
        self.offers.borrow_mut().clear();
    }

    /// The timer that bounds "frame inserted" → "Ready announced". It is the
    /// boot fix's execution boundary (§6): without it a frame that loaded a
    /// broken artifact answers with silence and the Shell would hold the
    /// loading card forever.
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
                // No contact at 20 s means the frame's OWN boot never ran —
                // its page is missing or its script rejected. That is the
                // artifact-load class: same stage the missing-`/library.js`
                // regression has always asserted (§6, §7).
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

    /// The `Painted` grace after `Ready`: a session that is up ought to paint
    /// on its next animation frames. If it does not, the cover comes down on
    /// this timeout anyway — the Shell never lets the loading state outrun
    /// the runtime it is covering for (§11), and the grace's expiry is itself
    /// reported as a stage so the failure mode is NAMED.
    fn arm_painted_grace(self: &Rc<Self>) {
        let Some(window) = window() else {
            return;
        };
        let weak = Rc::downgrade(self);
        let timer = Closure::<dyn FnMut()>::new(move || {
            let Some(driver) = weak.upgrade() else {
                return;
            };
            driver.painted_timer.set(None);
            driver.finish_painted_grace();
        });
        let Ok(id) = window.set_timeout_with_callback_and_timeout_and_arguments_0(
            timer.as_ref().unchecked_ref(),
            PAINTED_GRACE_MS,
        ) else {
            return;
        };
        self.painted_timer.set(Some(id));
        timer.into_js_value();
    }

    fn finish_painted_grace(&self) {
        if self.torn_down.get() {
            return;
        }
        // The grace expiring IS the cover-lift signal when Painted never
        // arrives: the manager applies it as the painted event (it already
        // names its own source in the digest).
        self.report(FrameEvent::Painted);
    }

    /// A fatal boundary problem during boot: resolve the ready gate with the
    /// stage and make sure no timer ever fires after it. The manager paints
    /// the error state and takes the frame down. After `Ready` the same
    /// class of problem is reported as a live-frame failure instead — there
    /// is no boot left to fail.
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
        *self.ready_pending.borrow_mut() = Some(Err(stage));
        if let Some(resolve) = self.ready_gate.borrow_mut().take() {
            let _ = resolve.call0(&JsValue::NULL);
        }
    }

    /// The port listener's entry point, with the frame's own identity
    /// recovered by the closure's captured `Rc<Driver>` — generation checks
    /// run before ANY signal becomes visible (§8, §35).
    fn dispatch_port(self: &Rc<Self>, event: &web_sys::MessageEvent) {
        let Some(envelope) = parse_runtime_event(event) else {
            return;
        };
        if envelope.generation != self.generation {
            self.report(FrameEvent::Stale);
            return;
        }
        // First contact on THIS port: adopt it as the lane. Only one of the
        // offered channels ever gets here (frame_transport's adopt-guard
        // makes the frame answer exactly one).
        if !self.saw_contact.get() {
            self.saw_contact.set(true);
            self.claim_lane(event);
            self.report(FrameEvent::Contact);
            // The handshake's next beat belongs to the Shell: `init` rides
            // the lane back (§7, framed README's mermaid), complete with
            // the identity and the reader's launch when there is one.
            self.send_init();
        }
        match envelope.body {
            RuntimeFrame::Status { stage } => {
                if stage == BootStage::Disposed {
                    // The runtime also sends DisposeComplete explicitly; the
                    // stage is an extra fact, not a second answer.
                }
                if stage == BootStage::Failed {
                    // §9, §11: the runtime's own failure becomes the shell's
                    // visible runtime error state, never a silent blank.
                    self.fatal_ready(FrameFatalStage::RuntimeFailed);
                    return;
                }
                self.report(FrameEvent::Stage(stage));
            }
            RuntimeFrame::Ready => {
                if self.try_resolve_ready() {
                    self.arm_painted_grace();
                }
                self.report(FrameEvent::Ready);
            }
            RuntimeFrame::Painted => {
                self.saw_painted.set(true);
                if let (Some(id), Some(window)) = (self.painted_timer.take(), window()) {
                    window.clear_timeout_with_handle(id);
                }
                self.report(FrameEvent::Painted);
            }
            RuntimeFrame::Failed { stage, cause } => {
                self.report(FrameEvent::Failed { stage, cause });
            }
            RuntimeFrame::DisposeComplete => {
                self.saw_dispose_complete.set(true);
                self.resolve_dispose(Ok(()));
                self.report(FrameEvent::DisposeComplete);
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
            RuntimeFrame::SaveLibrary { blob } => {
                self.report(FrameEvent::Boundary(FrameVocabulary::SaveLibrary(blob)));
            }
            RuntimeFrame::SaveCovers { covers } => {
                self.report(FrameEvent::Boundary(FrameVocabulary::SaveCovers(covers)));
            }
            RuntimeFrame::SaveCover { path, image } => {
                self.report(FrameEvent::Boundary(FrameVocabulary::SaveCover {
                    path,
                    image,
                }));
            }
            RuntimeFrame::BakeCover { path } => {
                self.report(FrameEvent::Boundary(FrameVocabulary::BakeCover { path }));
            }
            RuntimeFrame::DocStatus { report } => {
                self.report(FrameEvent::Boundary(FrameVocabulary::DocStatus(Box::new(
                    report,
                ))));
            }
            RuntimeFrame::PublishDigest { json } => {
                self.report(FrameEvent::Boundary(FrameVocabulary::PublishDigest(json)));
            }
            RuntimeFrame::Reload => {
                self.report(FrameEvent::Boundary(FrameVocabulary::Reload));
            }
            RuntimeFrame::ResolveLaunch { request, path } => {
                self.report(FrameEvent::Boundary(FrameVocabulary::ResolveLaunch {
                    request,
                    path,
                }));
            }
        }
    }

    /// Adopt the port that first contact arrived on. The offer set is closed
    /// with it; the adopted one is the only Offer that survives.
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

    /// The frame's `init`: identity and, for the reader, its launch (§7's
    /// "re-init must reuse the same nonce" — re-sends happen here, never
    /// with a minted identity).
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
            },
        );
    }

    /// Send a shell frame over the live lane. A lane that does not exist
    /// means the frame was never claimed (nothing to receive the message —
    /// the boundary answer dies with it, by design in §35).
    pub fn send(&self, body: &ShellFrame) {
        let lane = self.lane.borrow();
        if let Some(lane) = lane.as_ref() {
            post_shell_frame(lane, self.generation, &self.nonce, body);
        }
    }

    /// The manager's "wait for Ready": a promise the READY emission or the
    /// fatal timeout resolves. The caller inspects
    /// [`Driver::take_ready_outcome`] after the await for the stage verdict.
    pub fn wait_ready(&self) -> js_sys::Promise {
        js_sys::Promise::new(&mut |resolve, _reject| {
            self.ready_gate.borrow_mut().replace(resolve);
        })
    }

    /// The awaited boot verdict (set by `try_resolve_ready`/`fatal_ready`,
    /// taken by the manager once its wait resolves).
    pub fn take_ready_outcome(&self) -> Option<Result<(), FrameFatalStage>> {
        self.ready_pending.borrow_mut().take()
    }

    fn try_resolve_ready(&self) -> bool {
        if self.saw_ready.replace(true) {
            return false;
        }
        if let (Some(id), Some(window)) = (self.ready_timer.take(), window()) {
            window.clear_timeout_with_handle(id);
        }
        *self.ready_pending.borrow_mut() = Some(Ok(()));
        if let Some(resolve) = self.ready_gate.borrow_mut().take() {
            let _ = resolve.call0(&JsValue::NULL);
        }
        true
    }

    /// §12 phase 1, from the Shell's side: ask for the graceful shutdown and
    /// wait — bounded by the forced-removal timeout. The manager removes the
    /// frame itself when this resolves `Err`, and the digest names what the
    /// forced path heard.
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

    /// The awaited disposal verdict (`Ok` = DisposeComplete; `Err` = the
    /// forced path ran), taken by the manager once its wait resolves.
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

    /// Tear down everything JS-side: timers, the offers, the lane's listener,
    /// the window listener, and finally the iframe element. After this the
    /// driver is inert; generation checks keep stale stragglers silent.
    pub fn teardown(&self) {
        unregister(self.generation);
        if self.torn_down.replace(true) {
            return;
        }
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
    }
}

impl Drop for Driver {
    fn drop(&mut self) {
        self.teardown();
    }
}

/// The bounded-contact budget: one offer every 300 ms for up to 60 s —
/// the init-welcome space §6's sidebar calls for.
const OFFER_TICK_MS: i32 = 300;
const OFFER_TICK_LIMIT: u32 = 200;
/// The frame's whole boot (insert → `Ready`) has 20 s, and the Nil-error
/// discovery the guide wants is exactly "it got this long and didn't finish"
/// (§6 — never an unbounded loading state).
const READY_TIMEOUT_MS: i32 = 20_000;
/// `Painted` after `Ready`: two animation frames plus render jitter — and,
/// since the Active phase (and its `boot: <runtime>` line) is published on
/// Painted, this grace also bounds how long the terminal and the loading
/// cover can lag a mounted runtime. Two seconds is many frames of render
/// jitter; 15 s was a wait the user could feel.
const PAINTED_GRACE_MS: i32 = 2_500;
/// §12's forced removal: phase 1 gets 8 s, then the Shell takes phase 2.
const DISPOSE_TIMEOUT_MS: i32 = 8_000;

/// What the forced path heard (§12): the exact signals the frame managed to
/// emit before it was taken down. It is in every fatal cause and (through
/// the digest) in the probe, so a "nothing after Ready" boot reads exactly
/// that.
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
    /// The boot.rs stage the error card quotes (§6's stage names were the
    /// production vocabulary before frames; keep them — the card has never
    /// needed new words, only new truth inside the old ones).
    pub fn boot_stage(self) -> crate::app::boot::BootStage {
        match self {
            FrameFatalStage::InitializeTimeout => crate::app::boot::BootStage::ModuleLoad,
            FrameFatalStage::ReadyTimeout => crate::app::boot::BootStage::Start,
            FrameFatalStage::RuntimeFailed => crate::app::boot::BootStage::Start,
            FrameFatalStage::DisposeTimeout => crate::app::boot::BootStage::Dispose,
        }
    }
}

/// The protocol's stage as the shell's error-stage vocabulary: a reader
/// turning on a failure at `Mounted` means "start" to a user (the loading
/// stage names were about windows, not about serde tags).
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
