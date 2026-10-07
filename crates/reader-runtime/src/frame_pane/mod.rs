//! The workspace host's pane: a `PaneRuntime` whose document runs in a
//! frame of its own (docs/pane-runtimes.md). The pane realm's half is
//! `crate::pane_frame`; the vocabulary is `crate::pane_wire`.
//!
//! A `FramePane` owns its iframe(s), the port to each, and a MIRROR of the
//! pane's chrome-facing state: a `ReaderContext` in the host realm whose
//! signals the frame's reports write and whose writes (page, view mode,
//! fit, search) go back to the frame. The host's chrome — title, view menu,
//! rail, settings — renders against the mirror with the same components as
//! ever.
//!
//! Frames: the LIVE frame is the one on screen; every document replacement
//! boots an INCOMING realm behind it, even for the same format, and swaps on
//! actual document paint; RETIRED frames are removed when they
//! say so (or a timeout passes).

mod mirror;
pub(crate) mod raster;
pub mod thumbs;

use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

use leptos::prelude::*;
use leptos::tachys::reactive_graph::OwnedView;
use leptos::task::spawn_local;
use runtime_contract::boundary::{LaunchDocument, ShellApi};
use runtime_contract::protocol::{RuntimeEnvelope, RuntimeFrame};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};

use crate::context::ReaderContext;
use crate::host::contract::{
    ChromeSlot, LiftStep, OpenRequest, PaneAppearance, PaneBuild, PaneCommand, PaneEnv,
    PaneFactory, PaneResourceCounts, PaneRuntime, PaneSite, PaneSurface, PaneTeardown,
};
use crate::host::model::{
    DocumentId, PaneBounds, PaneDescriptor, PaneError, PaneFormat, PaneId, PaneLifecycle,
};
use crate::pane_wire::{
    Boot, Hook, HookState, HostToPane, Key, Mirror, PANE_CHANNEL_KIND, PANE_HELLO_KIND, PaneKind,
    PaneToHost, Paper, WireSidebar, Write, encode,
};
use reader_core::document::DocStatus;
use thumbs::RemoteThumbs;

/// How long a disposing frame may take to say it is done before the host
/// removes it regardless (ms).
const DISPOSE_TIMEOUT_MS: f64 = 1500.0;

/// How long a new frame may take to say hello before the pane reports that
/// its runtime never started (ms): a missing or broken artifact answers with
/// silence, and the pane must say so instead of staying blank.
const HELLO_TIMEOUT_MS: u64 = 10_000;
/// A realm that answered but never lands a document also fails visibly.
const PAINT_TIMEOUT_MS: u64 = 30_000;

/// The factory the composition root hands the host: every pane is a frame.
pub fn factory() -> PaneFactory {
    Rc::new(build) as Rc<PaneBuild>
}

fn build(
    env: PaneEnv,
    descriptor: PaneDescriptor,
    launch: Option<LaunchDocument>,
) -> Rc<dyn PaneRuntime> {
    FramePane::create(env, descriptor, launch)
}

/// One frame: an iframe running a pane realm, and its port.
struct Frame {
    nonce: String,
    iframe: web_sys::HtmlIFrameElement,
    port: Option<web_sys::MessagePort>,
    on_message: Option<Closure<dyn FnMut(web_sys::MessageEvent)>>,
    boot: Option<Boot>,
    painted: bool,
    hello_timeout: Option<TimeoutHandle>,
    paint_timeout: Option<TimeoutHandle>,
    mirror: Option<Mirror>,
    outline: Option<crate::pane_wire::WireOutline>,
    paper: Option<Paper>,
    /// The frame's latest diagnostics digest; the last one it sent before
    /// `Disposed` is its final word.
    digest: Option<String>,
    /// The frame answered `Dispose`.
    disposed: Rc<Cell<bool>>,
}

impl Frame {
    fn painted_document(&self) -> bool {
        handoff_ready(
            self.painted,
            self.mirror.as_ref().map(|m| (m.status, m.first_paint)),
        )
    }

    fn cancel_completed_deadline(&mut self) {
        if self.painted_document()
            && let Some(timeout) = self.paint_timeout.take()
        {
            timeout.clear();
        }
    }

    fn post(&self, message: &HostToPane) {
        if let (Some(port), Some(json)) = (self.port.as_ref(), encode(message)) {
            let _ = port.post_message(&JsValue::from_str(&json));
        }
    }

    /// The frame's digest as of now: its realm is same-origin, so its own
    /// probe answers synchronously; the last posted digest covers a realm
    /// that has not installed the probe yet.
    fn fresh_digest(&self) -> Option<String> {
        let fresh = self.iframe.content_window().and_then(|window| {
            let probe =
                js_sys::Reflect::get(&window, &JsValue::from_str("__mareaderDiagnostics")).ok()?;
            let probe: &js_sys::Function = probe.dyn_ref()?;
            probe.call0(&JsValue::NULL).ok()?.as_string()
        });
        fresh.or_else(|| self.digest.clone())
    }

    fn reveal(&self) {
        let _ = self.iframe.remove_attribute("data-frame-hidden");
    }

    /// Take the frame out of the document and drop its port: its realm is
    /// collected with it.
    fn remove(&mut self) {
        for timeout in [self.hello_timeout.take(), self.paint_timeout.take()]
            .into_iter()
            .flatten()
        {
            timeout.clear();
        }
        // The realm's last word is read before it goes: work its dispose
        // settled after its last posted digest still counts.
        self.digest = self.fresh_digest().or_else(|| {
            self.port
                .as_ref()
                .map(|_| "{\"atBaseline\":false}".to_string())
        });
        if let Some(port) = self.port.take() {
            port.set_onmessage(None);
            port.close();
        }
        self.on_message = None;
        raster::retire(&self.nonce);
        self.iframe.remove();
        forget_nonce(&self.nonce);
        if let Some(json) = self.digest.take() {
            FINALS.with(|f| {
                let mut total = f.borrow_mut();
                *total = Some(crate::diagnostics::fold_terminal_digest(
                    total.as_deref(),
                    &json,
                ));
            });
        }
    }
}

/// Which of a pane's frames a message came from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Role {
    Live,
    Incoming,
    Retired,
}

struct Inner {
    id: PaneId,
    env: PaneEnv,
    owner: Owner,
    ctx: ReaderContext,
    surface: PaneSurface,
    thumbs: RemoteThumbs,
    container: RefCell<Option<web_sys::Element>>,
    live: RefCell<Option<Frame>>,
    incoming: RefCell<Option<Frame>>,
    retired: RefCell<Vec<Frame>>,
    /// What the live frame last reported: a mirror write equal to it is the
    /// frame's own value coming back, never forwarded.
    reported: RefCell<Option<Mirror>>,
    lifecycle: Cell<PaneLifecycle>,
    appearance: Cell<Option<PaneAppearance>>,
    hooks: Cell<HookState>,
    requested: Cell<PaneFormat>,
    /// Set when a frame never said hello: the pane draws it over the empty
    /// frame (a frame that started draws its own errors).
    boot_error: RwSignal<Option<String>>,
    disposed: Cell<bool>,
    /// The pane holds a document: its close is a claim on the epoch.
    holds: Cell<bool>,
}

/// The host's pane over a frame.
pub struct FramePane {
    inner: Rc<Inner>,
}

impl FramePane {
    fn create(
        env: PaneEnv,
        descriptor: PaneDescriptor,
        launch: Option<LaunchDocument>,
    ) -> Rc<dyn PaneRuntime> {
        let id = descriptor.pane_id;
        let owner = Owner::new();
        let (ctx, surface, thumbs) = owner.with(|| {
            let handle = crate::pane::handle::PaneHandle::new(id, env.runtime);
            let first = launch
                .clone()
                .unwrap_or_else(crate::pane::base::empty_launch);
            let (ctx, surface) = crate::pane::base::contexts(env, handle, first);
            (ctx, surface, RemoteThumbs::new())
        });
        let boot_error = owner.with(|| RwSignal::new(None));
        let inner = Rc::new(Inner {
            id,
            env,
            owner,
            ctx,
            surface,
            thumbs,
            container: RefCell::new(None),
            live: RefCell::new(None),
            incoming: RefCell::new(None),
            retired: RefCell::new(Vec::new()),
            reported: RefCell::new(None),
            lifecycle: Cell::new(PaneLifecycle::New),
            appearance: Cell::new(None),
            hooks: Cell::new(HookState::default()),
            requested: Cell::new(descriptor.format),
            boot_error,
            disposed: Cell::new(false),
            holds: Cell::new(false),
        });
        thumbs.attach(Rc::downgrade(&inner));
        // A documentless host slot owns only its mirror/chrome. It creates
        // no empty reflow realm; the first real open boots a document once.
        if let Some(mut launch) = launch.filter(|_| descriptor.document.is_some()) {
            launch.resume_page = descriptor.initial_page.max(1);
            inner.claim_open();
            inner.expect_open(&launch);
            let mut boot = inner.fresh_boot(launch);
            boot.initial_zoom = descriptor.initial_zoom;
            let kind = PaneKind::for_path(&boot.launch.path);
            *inner.live.borrow_mut() = Some(new_frame(&inner, kind, boot));
        }
        inner.owner.with(|| install_effects(&inner));
        register(&inner);
        Rc::new(FramePane { inner })
    }
}

/// A frame for `kind`, hidden until its first paint. It joins the document
/// when the pane's container exists (at mount, or right away after).
fn new_frame(inner: &Rc<Inner>, kind: PaneKind, boot: Boot) -> Frame {
    let document = web_sys::window()
        .and_then(|w| w.document())
        .expect("a host document");
    let iframe: web_sys::HtmlIFrameElement = document
        .create_element("iframe")
        .expect("an iframe")
        .unchecked_into();
    let nonce = mint_nonce();
    iframe.set_class_name("pane-frame");
    let _ = iframe.set_attribute("data-frame-hidden", "");
    let _ = iframe.set_attribute("title", "Document");
    iframe.set_src(&format!("{}?pane={nonce}", kind.page()));
    if let Some(container) = inner.container.borrow().as_ref() {
        let _ = container.append_child(&iframe);
    }
    NONCES.with(|n| {
        n.borrow_mut()
            .push((nonce.clone(), Rc::downgrade(inner), iframe.clone()))
    });
    ensure_hello_listener();
    let hello_timeout = watch_boot(inner, kind, nonce.clone(), true);
    let paint_timeout = watch_boot(inner, kind, nonce.clone(), false);
    Frame {
        nonce,
        iframe,
        port: None,
        on_message: None,
        boot: Some(boot),
        painted: false,
        hello_timeout,
        paint_timeout,
        mirror: None,
        outline: None,
        paper: None,
        digest: None,
        disposed: Rc::new(Cell::new(false)),
    }
}

/// Fail the pane if the frame `nonce` has not said hello within
/// `HELLO_TIMEOUT_MS`: its document reports the error (the pane draws it,
/// and the Shell's status reports carry it) and the console keeps the
/// artifact that did not start.
fn watch_boot(
    inner: &Rc<Inner>,
    kind: PaneKind,
    nonce: String,
    hello: bool,
) -> Option<TimeoutHandle> {
    let weak = Rc::downgrade(inner);
    set_timeout_with_handle(
        move || {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            if inner.disposed.get() || inner.role_of(&nonce) == Some(Role::Retired) {
                return;
            }
            let stalled = inner.with_frame(&nonce, |frame| {
                if hello {
                    frame.port.is_none()
                } else {
                    !frame.painted_document()
                }
            });
            if stalled != Some(true) {
                return;
            }
            let script = kind.page().replace(".html", ".js");
            let message = if hello {
                format!(
                    "could not start the document runtime: /{script} did not load or never answered"
                )
            } else {
                "the document runtime started but did not finish its first paint".to_string()
            };
            web_sys::console::error_1(&format!("[mareader] pane boot failed: {message}").into());
            let document = inner.ctx.reader.document;
            put(document.error, Some(message.clone()));
            put(document.status, DocStatus::Error);
            put(inner.boot_error, Some(message));
        },
        std::time::Duration::from_millis(if hello {
            HELLO_TIMEOUT_MS
        } else {
            PAINT_TIMEOUT_MS
        }),
    )
    .ok()
}

/// A mounted realm is not yet a painted document surface.
fn handoff_ready(painted: bool, mirror: Option<(DocStatus, bool)>) -> bool {
    painted
        && mirror.is_some_and(|(status, first)| {
            (status == DocStatus::Ready && first) || status == DocStatus::Error
        })
}

/// The host → pane flow: every host-owned fact the pane mirrors, sent to
/// the frames whenever it changes.
fn install_effects(inner: &Rc<Inner>) {
    let env = inner.env;
    let weak = Rc::downgrade(inner);
    let w = weak.clone();
    Effect::new(move |_| {
        let settings = env.settings.get();
        if let Some(inner) = w.upgrade() {
            inner.broadcast(&HostToPane::Settings(Box::new(settings)));
        }
    });
    let w = weak.clone();
    Effect::new(move |_| {
        let look = env.workspace.get();
        if let Some(inner) = w.upgrade() {
            inner.broadcast(&HostToPane::Workspace(look));
        }
    });
    let w = weak.clone();
    Effect::new(move |_| {
        let on = env.active.get();
        if let Some(inner) = w.upgrade() {
            inner.broadcast(&HostToPane::Active { on });
        }
    });
    let w = weak.clone();
    Effect::new(move |_| {
        let can_split = env.can_split.get();
        let moves = env.moves.get();
        if let Some(inner) = w.upgrade() {
            inner.broadcast(&HostToPane::Layout { can_split, moves });
        }
    });
    let w = weak.clone();
    Effect::new(move |_| {
        let mode = WireSidebar::from(env.ui.sidebar.get());
        if let Some(inner) = w.upgrade() {
            inner.broadcast(&HostToPane::Sidebar { mode });
        }
    });
    let w = weak.clone();
    Effect::new(move |_| {
        let on = env.settings_open.get();
        if let Some(inner) = w.upgrade() {
            inner.broadcast(&HostToPane::SettingsOpen { on });
        }
    });

    // The chrome's writes into the mirror, forwarded when they differ from
    // what the frame last said.
    let viewer = inner.ctx.reader.viewer;
    let search = inner.ctx.reader.search;
    let w = weak.clone();
    Effect::new(move |_| {
        let page = viewer.page.get();
        if let Some(inner) = w.upgrade() {
            inner.write_if(|m| m.page != page, Write::Page { page });
        }
    });
    let w = weak.clone();
    Effect::new(move |_| {
        let mode = viewer.mode.get();
        if let Some(inner) = w.upgrade() {
            inner.write_if(|m| m.mode != mode, Write::Mode { mode });
        }
    });
    let w = weak.clone();
    Effect::new(move |_| {
        let fit = viewer.fit.get();
        if let Some(inner) = w.upgrade() {
            inner.write_if(|m| m.fit != fit, Write::Fit { fit });
        }
    });
    let w = weak.clone();
    Effect::new(move |_| {
        let on = viewer.auto_scroll.get();
        if let Some(inner) = w.upgrade() {
            inner.write_if(|m| m.auto_scroll != on, Write::AutoScroll { on });
        }
    });
    let w = weak.clone();
    Effect::new(move |_| {
        let on = search.visible.get();
        if let Some(inner) = w.upgrade() {
            inner.write_if(|m| m.search_visible != on, Write::SearchVisible { on });
        }
    });

    // A zoom step is a directive for the same reason the outline jump is:
    // there is no target scale to compare against, because the ladder step
    // resolves against the window, the mode and the page, which only the
    // pane's own coordinator knows. Taken on the way to the frame, so a press
    // that arrives with no frame to take it is gone rather than replayed.
    let w = weak.clone();
    Effect::new(move |_| {
        let Some(step) = viewer.take_zoom_step() else {
            return;
        };
        if let Some(inner) = w.upgrade() {
            inner.step_zoom(step);
        }
    });

    // The outline's jump is not a mirrored value, so `write_if` cannot guard
    // it: there is no mirror field to compare against. It is TAKEN instead —
    // the host takes one on the way to a frame, the pane's own arm takes one on
    // the way to the stream (`ViewerSignals::take_outline_jump`). A value left
    // standing would be re-sent at a handoff (a lift, a swap, a mode flip) as a
    // jump the reader made long ago, and a second click on the same entry
    // would not be a change.
    let w = weak;
    Effect::new(move |_| {
        let Some(index) = viewer.take_outline_jump() else {
            return;
        };
        if let Some(inner) = w.upgrade() {
            inner.jump_outline(index);
        }
    });
}

impl Inner {
    fn broadcast(&self, message: &HostToPane) {
        if let Some(frame) = self.live.borrow().as_ref() {
            frame.post(message);
        }
        if let Some(frame) = self.incoming.borrow().as_ref() {
            frame.post(message);
        }
    }

    fn post_live(&self, message: &HostToPane) {
        if let Some(frame) = self.live.borrow().as_ref() {
            frame.post(message);
        }
    }

    /// Hand the pane realm a one-shot directive: show outline entry `index`.
    /// The mirrored writes go through [`Inner::write_if`]; a directive has no
    /// mirror field to compare, so it is posted and taken instead.
    fn jump_outline(&self, index: u32) {
        self.post_live(&HostToPane::Write(Write::Outline { index }));
    }

    /// Hand the pane realm a one-shot directive: one step along the zoom
    /// ladder. The pane's coordinator owns the resolving (a step depends on
    /// the window, the mode and the page — see [`Write::ZoomStep`]), so the
    /// host posts the press and nothing else.
    fn step_zoom(&self, step: i32) {
        self.post_live(&HostToPane::Write(Write::ZoomStep { step }));
    }

    /// Forward a chrome write unless the live frame already reported it.
    fn write_if(&self, differs: impl FnOnce(&Mirror) -> bool, write: Write) {
        let forward = self.reported.borrow().as_ref().is_some_and(differs);
        if forward {
            self.post_live(&HostToPane::Write(write));
        }
    }

    fn role_of(&self, nonce: &str) -> Option<Role> {
        if self
            .live
            .borrow()
            .as_ref()
            .is_some_and(|f| f.nonce == nonce)
        {
            return Some(Role::Live);
        }
        if self
            .incoming
            .borrow()
            .as_ref()
            .is_some_and(|f| f.nonce == nonce)
        {
            return Some(Role::Incoming);
        }
        self.retired
            .borrow()
            .iter()
            .any(|f| f.nonce == nonce)
            .then_some(Role::Retired)
    }

    fn with_frame<R>(&self, nonce: &str, f: impl FnOnce(&mut Frame) -> R) -> Option<R> {
        if let Some(frame) = self
            .live
            .borrow_mut()
            .as_mut()
            .filter(|fr| fr.nonce == nonce)
        {
            return Some(f(frame));
        }
        if let Some(frame) = self
            .incoming
            .borrow_mut()
            .as_mut()
            .filter(|fr| fr.nonce == nonce)
        {
            return Some(f(frame));
        }
        self.retired
            .borrow_mut()
            .iter_mut()
            .find(|fr| fr.nonce == nonce)
            .map(f)
    }

    /// The frame's realm said hello and was handed `port`: listen, then
    /// boot it.
    fn adopt(self: &Rc<Self>, nonce: &str, port: web_sys::MessagePort) {
        let weak = Rc::downgrade(self);
        let tag = nonce.to_string();
        let on_message =
            Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |ev: web_sys::MessageEvent| {
                if let Some(inner) = weak.upgrade() {
                    inner.receive_raw(&tag, ev.data());
                }
            });
        port.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
        let lifecycle = self.lifecycle.get();
        let appearance = self.appearance.get();
        // Read before borrowing the frame: the board walks every pane's.
        let paper = board_paper();
        let hooks = self.hooks.get();
        // The boot was drafted when the frame was created; what the env said
        // since went to a frame without a port, so the boot carries now.
        let env = &self.env;
        let settings = env.settings.get_untracked();
        let workspace = env.workspace.get_untracked();
        let active = env.active.get_untracked();
        let can_split = env.can_split.get_untracked();
        let moves = env.moves.get_untracked();
        let sidebar = WireSidebar::from(env.ui.sidebar.get_untracked());
        let settings_open = env.settings_open.get_untracked();
        self.with_frame(nonce, move |frame| {
            if let Some(timeout) = frame.hello_timeout.take() {
                timeout.clear();
            }
            frame.port = Some(port);
            frame.on_message = Some(on_message);
            if let Some(mut boot) = frame.boot.take() {
                if let Some(appearance) = appearance {
                    boot.motion = appearance.motion.into();
                    boot.look = appearance.look;
                }
                boot.paper = paper;
                boot.hooks = hooks;
                boot.settings = settings;
                boot.workspace = workspace;
                boot.active = active;
                boot.can_split = can_split;
                boot.moves = moves;
                boot.sidebar = sidebar;
                boot.settings_open = settings_open;
                frame.post(&HostToPane::Boot(Box::new(boot)));
            }
            frame.post(&HostToPane::Lifecycle { lifecycle });
        });
    }

    /// An open is on its way to the frame: the mirror says so at once, so
    /// the host's chrome (title, close) follows the click instead of the
    /// frame's first report. The frame's own reports take over from here.
    fn expect_open(&self, launch: &LaunchDocument) {
        put(self.boot_error, None);
        self.thumbs.reset();
        put(self.ctx.launch, launch.clone());
        let document = self.ctx.reader.document;
        put(document.error, None);
        put(document.path, Some(launch.path.clone()));
        put(document.book_id, launch.book_id.clone());
        put(document.title, None);
        put(document.author, None);
        put(document.num_pages, 0);
        put(document.content.metrics.page1_size, None);
        put(document.outline_pending, true);
        self.apply_outline(&Vec::new());
        put(
            document.format,
            reader_core::format::format_of(&launch.path),
        );
        put(document.status, DocStatus::Opening);
    }

    fn receive_raw(self: &Rc<Self>, nonce: &str, data: JsValue) {
        if let Some(json) = data.as_string() {
            match serde_json::from_str::<PaneToHost>(&json) {
                Ok(message) => self.receive(nonce, message),
                Err(err) => {
                    web_sys::console::warn_1(&format!("[host] bad pane message: {err}").into());
                }
            }
            return;
        }
        // A thumbnail: a plain object with its bitmap.
        let field = |name: &str| js_sys::Reflect::get(&data, &JsValue::from_str(name)).ok();
        if field("t").and_then(|t| t.as_string()).as_deref()
            == Some(crate::pane_wire::THUMB_MESSAGE)
            && let Some(req) = field("req").and_then(|r| r.as_f64())
            && let Some(bitmap) = field("bitmap")
        {
            let role = self.role_of(nonce);
            if role == Some(Role::Live) {
                self.thumbs.deliver(req as u64, bitmap);
            } else if let Ok(bitmap) = bitmap.dyn_into::<web_sys::ImageBitmap>() {
                bitmap.close();
            }
        }
    }

    fn receive(self: &Rc<Self>, nonce: &str, message: PaneToHost) {
        let Some(role) = self.role_of(nonce) else {
            return;
        };
        let env = self.env;
        match message {
            PaneToHost::Disposed => {
                self.with_frame(nonce, |frame| frame.disposed.set(true));
                self.sweep_retired();
            }
            PaneToHost::Api { envelope } => {
                // A retired frame's last words still count: its final read
                // point and digest.
                self.api(nonce, role, &envelope);
            }
            _ if role == Role::Retired => {}
            PaneToHost::Painted => {
                self.with_frame(nonce, |frame| {
                    frame.painted = true;
                    frame.cancel_completed_deadline();
                });
                match role {
                    Role::Live => {
                        if let Some(frame) = self.live.borrow().as_ref() {
                            frame.reveal();
                        }
                    }
                    Role::Incoming => self.try_swap(),
                    Role::Retired => {}
                }
            }
            PaneToHost::Mirror(mirror) => {
                let mirror = *mirror;
                self.with_frame(nonce, |frame| {
                    frame.mirror = Some(mirror.clone());
                    frame.cancel_completed_deadline();
                });
                match role {
                    // A live frame being replaced speaks for a document
                    // the pane has already left: the pane shows the open.
                    Role::Live if self.incoming.borrow().is_some() => {}
                    Role::Live => self.apply_mirror(mirror),
                    Role::Incoming => self.try_swap(),
                    Role::Retired => {}
                }
            }
            PaneToHost::Outline { entries } => {
                if role == Role::Live && self.incoming.borrow().is_none() {
                    self.apply_outline(&entries);
                }
                self.with_frame(nonce, |frame| frame.outline = Some(entries));
            }
            PaneToHost::Paper(paper) => {
                self.with_frame(nonce, |frame| frame.paper = Some(paper));
                if role == Role::Live && self.incoming.borrow().is_none() {
                    board_refresh();
                }
            }
            PaneToHost::ThumbsStale => {
                // The pane re-baked its look: the rail's pictures of THIS
                // pane were baked against the look before it, and the host
                // holds the only copies. Clearing `painted` and bumping the
                // epoch makes every settled cell render again — the frame's
                // cache answers with the new bake. Only a LIVE pane's cards
                // are on screen; the other roles speak for a document the
                // rail has already left.
                if role == Role::Live {
                    self.thumbs.invalidate();
                }
            }
            _ if role != Role::Live => {}
            PaneToHost::Press => {
                // The host's own outside-press handlers (menus, popovers)
                // never see a press inside a frame: replay one on the frame
                // element, which is outside all of them.
                let iframe = self.live.borrow().as_ref().map(|f| f.iframe.clone());
                if let Some(iframe) = iframe
                    && let Ok(event) =
                        web_sys::PointerEvent::new_with_event_init_dict("pointerdown", &{
                            let init = web_sys::PointerEventInit::new();
                            init.set_bubbles(true);
                            init
                        })
                {
                    let _ = iframe.dispatch_event(&event);
                }
                env.request_focus.try_run(());
            }
            PaneToHost::Focus => {
                env.request_focus.try_run(());
            }
            PaneToHost::Settings(settings) => {
                let settings = *settings;
                if env.settings.try_with_untracked(|s| *s != settings) == Some(true) {
                    env.settings.set(settings);
                }
            }
            PaneToHost::Open { launch, placement } => {
                env.open.try_run(OpenRequest {
                    launch: *launch,
                    placement: placement.into(),
                });
            }
            PaneToHost::OpenPath { path, placement } => {
                crate::services::document::open::open_path(self.ctx, path, placement.into());
            }
            PaneToHost::Relocate { direction } => {
                env.relocate.try_run(direction);
            }
            PaneToHost::Sidebar { mode } => {
                let mode: app_state::SidebarMode = mode.into();
                if env.ui.sidebar.try_get_untracked() != Some(mode) {
                    env.ui.sidebar.set(mode);
                }
            }
            PaneToHost::SettingsOpen { on } => {
                if env.settings_open.try_get_untracked() != Some(on) {
                    env.settings_open.set(on);
                }
            }
            PaneToHost::ThumbFailed { req, cancelled } => self.thumbs.fail(req, cancelled),
            PaneToHost::Lift { phase, x, y } => {
                let at = self.to_host((x, y));
                env.lift.try_run(LiftStep { phase, at });
            }
        }
    }

    /// A point in the live frame's client coordinates, in the host's. The
    /// frame may be scaled (a lifted pane rides as a smaller card).
    fn to_host(&self, (x, y): (f64, f64)) -> (f64, f64) {
        let Some(frame) = self.live.borrow().as_ref().map(|f| f.iframe.clone()) else {
            return (x, y);
        };
        let rect = frame.get_bounding_client_rect();
        let width = f64::from(frame.client_width()).max(1.0);
        let height = f64::from(frame.client_height()).max(1.0);
        (
            rect.left() + x * rect.width() / width,
            rect.top() + y * rect.height() / height,
        )
    }

    /// A shell call the pane realm made, answered by the host's own api.
    fn api(&self, nonce: &str, role: Role, envelope: &str) {
        let Ok(envelope) = serde_json::from_str::<RuntimeEnvelope>(envelope) else {
            return;
        };
        let api = self.env.api;
        match envelope.body {
            RuntimeFrame::PublishDigest { json } => {
                self.with_frame(nonce, |frame| frame.digest = Some(json));
            }
            RuntimeFrame::OpenDocument { launch } if role == Role::Live => {
                api.open_document(&launch)
            }
            RuntimeFrame::NavigateLibrary if role == Role::Live => api.navigate_library(),
            RuntimeFrame::ReadPoint { point } => api.read_point(&point),
            RuntimeFrame::SaveSettings { .. } => {}
            RuntimeFrame::SaveCover { path, image } => api.save_cover(&path, &image),
            RuntimeFrame::SaveGloss { key, marks } => api.save_gloss(&key, marks),
            RuntimeFrame::BakeCover { path } => api.bake_cover(&path),
            // Only the live frame acts for the user; the others' last words
            // are their state (read point, digest, cover, gloss, a bake).
            // The document status the Shell hears is the workspace's, which
            // the host reports from the mirrors: a realm's own report (its
            // boot-time Idle can land after the host's open) is not the
            // workspace's word.
            _ => {}
        }
    }

    /// An incoming frame takes over once it has painted AND its document
    /// is on screen (or failed): the swap is one frame, never a blank.
    fn try_swap(self: &Rc<Self>) {
        let ready = self
            .incoming
            .borrow()
            .as_ref()
            .is_some_and(Frame::painted_document);
        if !ready {
            return;
        }
        let Some(incoming) = self.incoming.borrow_mut().take() else {
            return;
        };
        incoming.reveal();
        let mirror = incoming.mirror.clone();
        let outline = incoming.outline.clone();
        let old = self.live.borrow_mut().replace(incoming);
        if let Some(old) = old {
            self.retire(old);
        }
        self.thumbs.reset();
        if let Some(mirror) = mirror {
            self.apply_mirror(mirror);
        }
        self.apply_outline(&outline.unwrap_or_default());
        board_refresh();
    }

    /// The pane opens a document: a claim on the epoch, after the close of
    /// the one it held.
    fn claim_open(&self) {
        if self.holds.replace(true) {
            claim_epoch();
        }
        claim_epoch();
    }

    /// Send a frame its `Dispose` and keep it (hidden) until it answers.
    fn retire(self: &Rc<Self>, mut frame: Frame) {
        for timeout in [frame.hello_timeout.take(), frame.paint_timeout.take()]
            .into_iter()
            .flatten()
        {
            timeout.clear();
        }
        let _ = frame.iframe.set_attribute("data-frame-hidden", "");
        frame.post(&HostToPane::Dispose);
        let disposed = frame.disposed.clone();
        let nonce = frame.nonce.clone();
        self.retired.borrow_mut().push(frame);
        let weak = Rc::downgrade(self);
        spawn_local(async move {
            wait_until(DISPOSE_TIMEOUT_MS, move || disposed.get()).await;
            if let Some(inner) = weak.upgrade() {
                inner.retired.borrow_mut().retain_mut(|frame| {
                    if frame.nonce == nonce {
                        frame.remove();
                        false
                    } else {
                        true
                    }
                });
            }
        });
    }

    /// Remove the retired frames that said they are done.
    fn sweep_retired(&self) {
        let mut retired = self.retired.borrow_mut();
        retired.retain_mut(|frame| {
            if frame.disposed.get() {
                frame.remove();
                false
            } else {
                true
            }
        });
    }

    /// The timeout passed: every retired frame goes.
    fn sweep_retired_all(&self) {
        for mut frame in self.retired.borrow_mut().drain(..) {
            frame.remove();
        }
    }

    fn paper(&self) -> Option<Paper> {
        self.live.borrow().as_ref().and_then(|f| f.paper.clone())
    }
}

/// Set a signal only when the value changed (and only while it lives).
fn put<T: PartialEq + Send + Sync + 'static>(signal: RwSignal<T>, value: T) {
    if signal.try_with_untracked(|v| *v != value) == Some(true) {
        signal.set(value);
    }
}

impl PaneRuntime for FramePane {
    fn id(&self) -> PaneId {
        self.inner.id
    }

    fn format(&self) -> PaneFormat {
        let document = &self.inner.ctx.reader.document;
        if document
            .path
            .try_with_untracked(Option::is_none)
            .unwrap_or(true)
        {
            return self.inner.requested.get();
        }
        match document.format.try_get_untracked() {
            Some(reader_core::format::Format::Pdf) => PaneFormat::Pdf,
            Some(reader_core::format::Format::Markdown) => PaneFormat::Markdown,
            Some(reader_core::format::Format::Text) => PaneFormat::Text,
            None => PaneFormat::Pending,
        }
    }

    fn document(&self) -> Option<DocumentId> {
        let document = &self.inner.ctx.reader.document;
        let path = document.path.try_get_untracked().flatten()?;
        let book_id = document.book_id.try_get_untracked().flatten();
        DocumentId::from_launch(book_id.as_deref(), &path)
    }

    fn lifecycle_changed(&self, lifecycle: PaneLifecycle) {
        self.inner.lifecycle.set(lifecycle);
        self.inner.ctx.pane.publish_lifecycle(lifecycle);
        self.inner.broadcast(&HostToPane::Lifecycle { lifecycle });
    }

    fn surface(&self) -> PaneSurface {
        self.inner.surface
    }

    fn mount(&self, bounds: PaneBounds, _site: PaneSite) -> AnyView {
        self.inner.ctx.reader.dom.set_bounds(bounds);
        let container: NodeRef<leptos::html::Div> = NodeRef::new();
        let weak = Rc::downgrade(&self.inner);
        container.on_load(move |el| {
            let Some(inner) = weak.upgrade() else {
                return;
            };
            let el: web_sys::Element = el.into();
            for frame in [
                inner.live.borrow().as_ref(),
                inner.incoming.borrow().as_ref(),
            ]
            .into_iter()
            .flatten()
            {
                if !frame.iframe.is_connected() {
                    let _ = el.append_child(&frame.iframe);
                }
            }
            *inner.container.borrow_mut() = Some(el);
        });
        let boot_error = self.inner.boot_error;
        let failed = move || boot_error.try_get().flatten();
        view! {
            <div node_ref=container class="absolute inset-0" />
            {move || {
                failed()
                    .map(|message| {
                        view! {
                            <div
                                class="pane-boot-error absolute inset-0 flex items-center justify-center p-6 text-center text-sm"
                                data-pane-boot="error"
                                role="alert"
                            >
                                {message}
                            </div>
                        }
                    })
            }}
        }
        .into_any()
    }

    fn chrome(&self, slot: ChromeSlot, site: PaneSite) -> Option<AnyView> {
        let inner = &self.inner;
        let ctx = inner.ctx;
        let settings_open = inner.env.settings_open;
        let thumbs = inner.thumbs;
        let child = inner.owner.child();
        let view = child.with(|| {
            if let Some(title_bar) = site.title_bar {
                provide_context(title_bar);
            }
            provide_context(thumbs);
            untrack(|| match slot {
                ChromeSlot::TitleCenter => view! {
                    <crate::components::shell::titlebar::document_title::CenteredDocTitle
                        state=ctx
                    />
                }
                .into_any(),
                ChromeSlot::TitleTrailing => view! {
                    <crate::components::menus::reader_menu::ReaderMenu
                        state=ctx
                        settings_open=settings_open
                    />
                }
                .into_any(),
                ChromeSlot::Rail => {
                    let shell =
                        expect_context::<app_ui::components::shell::controller::ShellController>();
                    view! { <crate::features::rail::ReaderRail state=ctx shell=shell /> }.into_any()
                }
                ChromeSlot::Settings => view! {
                    <crate::components::settings::modal::SettingsModal
                        state=ctx
                        open=settings_open
                    />
                }
                .into_any(),
            })
        });
        Some(OwnedView::new_with_owner(view, child).into_any())
    }

    fn resize(&self, bounds: PaneBounds) {
        // The frame fills the entry; its own window resize re-measures it.
        self.inner.ctx.reader.dom.set_bounds(bounds);
    }

    fn focus(&self) {
        board_touch(self.inner.id);
    }

    fn blur(&self) {}

    fn appearance(&self, appearance: PaneAppearance) {
        self.inner.appearance.set(Some(appearance));
        let motion = self.inner.ctx.reader.viewer.motion;
        put(motion, appearance.motion);
        put(self.inner.ctx.reader.viewer.look, appearance.look);
        self.inner.broadcast(&HostToPane::Appearance {
            motion: appearance.motion.into(),
            look: appearance.look,
        });
    }

    fn command(&self, command: PaneCommand) -> Result<(), PaneError> {
        let inner = &self.inner;
        if !inner.lifecycle.get().is_live() {
            return Err(PaneError::Gone(inner.id));
        }
        match command {
            PaneCommand::Open(launch) => {
                inner
                    .requested
                    .set(crate::pane::document::classify(&launch.path));
                inner.expect_open(&launch);
                inner.claim_open();
                let kind = PaneKind::for_path(&launch.path);
                // Every actual replacement is a fresh realm, including a
                // same-format or not-yet-adopted predecessor. Its outgoing
                // pixels remain until the incoming document really paints.
                let frame = new_frame(inner, kind, inner.fresh_boot(*launch));
                let old = inner.incoming.borrow_mut().replace(frame);
                if let Some(old) = old {
                    inner.retire(old);
                }
            }
            PaneCommand::PrepareLeave => inner.post_live(&HostToPane::PrepareLeave),
        }
        Ok(())
    }

    fn resources(&self) -> PaneResourceCounts {
        self.inner
            .reported
            .borrow()
            .as_ref()
            .map(|m| m.resources)
            .unwrap_or_default()
    }

    fn dispose(&self) -> PaneTeardown {
        let inner = self.inner.clone();
        inner.disposed.set(true);
        if inner.holds.replace(false) {
            claim_epoch();
        }
        unregister(inner.id);
        DISPOSING.with(|d| {
            let mut d = d.borrow_mut();
            d.retain(|w| w.strong_count() > 0);
            d.push(Rc::downgrade(&inner));
        });
        board_refresh();
        inner.thumbs.reset();
        // Every frame joins the retired list, where its last words (the
        // final digest, then `Disposed`) still find it: the sweep removes
        // each as it says it is done.
        let mut frames: Vec<Frame> = Vec::new();
        frames.extend(inner.live.borrow_mut().take());
        frames.extend(inner.incoming.borrow_mut().take());
        for frame in &frames {
            frame.post(&HostToPane::Dispose);
        }
        inner.retired.borrow_mut().extend(frames);
        let flags: Vec<(Rc<Cell<bool>>, web_sys::HtmlIFrameElement)> = inner
            .retired
            .borrow()
            .iter()
            .map(|f| (f.disposed.clone(), f.iframe.clone()))
            .collect();
        inner.owner.pause();
        inner.owner.cleanup();
        crate::diagnostics::note_pane_dispose();
        Box::pin(async move {
            // Done when every frame said so, or left the document with the
            // workspace (the session's end), or the timeout passed.
            wait_until(DISPOSE_TIMEOUT_MS, move || {
                flags
                    .iter()
                    .all(|(done, iframe)| done.get() || !iframe.is_connected())
            })
            .await;
            inner.sweep_retired_all();
        })
    }
}

impl Inner {
    /// A boot for a replacement frame: the pane's current facts, the new
    /// launch.
    fn fresh_boot(&self, launch: LaunchDocument) -> Boot {
        let inner = self;
        let env = inner.env;
        let appearance = inner.appearance.get();
        Boot {
            pane_id: inner.id.get(),
            session_id: env.session_id,
            format: crate::pane::document::classify(&launch.path),
            initial_page: launch.resume_page.max(1),
            launch,
            initial_zoom: None,
            settings: env.settings.get_untracked(),
            motion: appearance.map(|a| a.motion.into()).unwrap_or_default(),
            look: appearance.and_then(|a| a.look),
            workspace: env.workspace.get_untracked(),
            paper: board_paper(),
            hooks: inner.hooks.get(),
            active: env.active.get_untracked(),
            can_split: env.can_split.get_untracked(),
            moves: env.moves.get_untracked(),
            sidebar: WireSidebar::from(env.ui.sidebar.get_untracked()),
            settings_open: env.settings_open.get_untracked(),
        }
    }
}

/// Resolve when `done` holds, polling each animation-frame-sized beat, or
/// when `timeout_ms` passes.
async fn wait_until(timeout_ms: f64, done: impl Fn() -> bool) {
    let start = now_ms();
    while !done() && now_ms() - start < timeout_ms {
        sleep_ms(16).await;
    }
}

async fn sleep_ms(ms: i32) {
    let promise = js_sys::Promise::new(&mut |resolve, _| {
        if let Some(win) = web_sys::window() {
            let _ = win.set_timeout_with_callback_and_timeout_and_arguments_0(&resolve, ms);
        }
    });
    let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
}

fn now_ms() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map_or(0.0, |p| p.now())
}

// ---------------------------------------------------------------------------
// The registry: live frame panes, the channel handshake, the shared paper,
// the engine hooks and the keyboard forward.
// ---------------------------------------------------------------------------

type HelloListener = Closure<dyn FnMut(web_sys::MessageEvent)>;
type KeyListener = Closure<dyn FnMut(web_sys::KeyboardEvent)>;

thread_local! {
    /// Live frame panes, most recently focused first.
    static PANES: RefCell<Vec<Weak<Inner>>> = const { RefCell::new(Vec::new()) };
    /// Frames waiting for their hello: nonce, pane, element.
    static NONCES: RefCell<Vec<(String, Weak<Inner>, web_sys::HtmlIFrameElement)>> =
        const { RefCell::new(Vec::new()) };
    static HELLO: RefCell<Option<HelloListener>> = const { RefCell::new(None) };
    static KEYS: RefCell<Option<KeyListener>> = const { RefCell::new(None) };
    static NEXT_NONCE: Cell<u64> = const { Cell::new(0) };
    /// The paper the host last shared.
    static SHARED_PAPER: RefCell<Option<Paper>> = const { RefCell::new(None) };
    /// Disposed panes' final digests, for the diagnostics balances.
    static FINALS: RefCell<Option<String>> = const { RefCell::new(None) };
    /// Disposed panes whose frames have not all gone yet.
    static DISPOSING: RefCell<Vec<Weak<Inner>>> = const { RefCell::new(Vec::new()) };
}

/// The document-session epoch, which the host claims for its pane realms
/// (each open, each close of a held document): realms come and go — a kind
/// swap boots a new one — while the workspace's sessions are one count.
fn claim_epoch() {
    crate::services::document::session::next_generation();
}

fn mint_nonce() -> String {
    let n = NEXT_NONCE.with(|c| {
        let n = c.get().wrapping_add(1);
        c.set(n);
        n
    });
    let salt = (js_sys::Math::random() * 1e12) as u64;
    format!("{n:x}-{salt:x}")
}

fn forget_nonce(nonce: &str) {
    NONCES.with(|n| n.borrow_mut().retain(|(other, _, _)| other != nonce));
}

fn register(inner: &Rc<Inner>) {
    PANES.with(|p| p.borrow_mut().push(Rc::downgrade(inner)));
    ensure_key_forward();
}

fn unregister(id: PaneId) {
    let empty = PANES.with(|p| {
        let mut panes = p.borrow_mut();
        panes.retain(|w| w.upgrade().is_some_and(|inner| inner.id != id));
        panes.is_empty()
    });
    if empty {
        drop_listeners();
    }
}

fn panes() -> Vec<Rc<Inner>> {
    PANES.with(|p| p.borrow().iter().filter_map(Weak::upgrade).collect())
}

/// The window listener that answers a pane realm's hello with its port.
/// Installed with the first frame, removed with the last pane.
fn ensure_hello_listener() {
    if HELLO.with(|h| h.borrow().is_some()) {
        return;
    }
    let listener = Closure::<dyn FnMut(web_sys::MessageEvent)>::new(|ev: web_sys::MessageEvent| {
        let data = ev.data();
        let field = |name: &str| {
            js_sys::Reflect::get(&data, &JsValue::from_str(name))
                .ok()
                .and_then(|v| v.as_string())
        };
        if field("kind").as_deref() != Some(PANE_HELLO_KIND) {
            return;
        }
        let Some(nonce) = field("nonce") else {
            return;
        };
        let found = NONCES.with(|n| {
            n.borrow()
                .iter()
                .find(|(other, _, _)| *other == nonce)
                .map(|(_, inner, iframe)| (inner.clone(), iframe.clone()))
        });
        let Some((inner, iframe)) = found else {
            return;
        };
        // The hello must come from THAT frame's window.
        let Some(window) = iframe.content_window() else {
            return;
        };
        let from_it = ev
            .source()
            .is_some_and(|source| JsValue::from(source) == JsValue::from(window.clone()));
        if !from_it {
            return;
        }
        let Some(inner) = inner.upgrade() else {
            return;
        };
        if inner.disposed.get() || inner.role_of(&nonce) == Some(Role::Retired) {
            return;
        }
        let Ok(channel) = web_sys::MessageChannel::new() else {
            return;
        };
        forget_nonce(&nonce);
        let offer = js_sys::Object::new();
        let _ = js_sys::Reflect::set(&offer, &"kind".into(), &PANE_CHANNEL_KIND.into());
        let _ = js_sys::Reflect::set(&offer, &"nonce".into(), &nonce.clone().into());
        let origin = web_sys::window()
            .and_then(|w| w.location().origin().ok())
            .unwrap_or_else(|| "*".into());
        let transfer = js_sys::Array::of1(&channel.port2());
        if window
            .post_message_with_transfer(&offer, &origin, &transfer)
            .is_ok()
        {
            inner.adopt(&nonce, channel.port1());
        }
    });
    if let Some(win) = web_sys::window() {
        let _ = win.add_event_listener_with_callback("message", listener.as_ref().unchecked_ref());
    }
    HELLO.with(|h| *h.borrow_mut() = Some(listener));
}

/// Keys pressed while the HOST document has focus (after a click on the
/// title bar, say) go to the active pane, unless the host is typing.
fn ensure_key_forward() {
    if KEYS.with(|k| k.borrow().is_some()) {
        return;
    }
    let listener =
        Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(|ev: web_sys::KeyboardEvent| {
            if !ev.is_trusted() || ev.default_prevented() || typing(&ev) {
                return;
            }
            // An Escape a host surface (a modal, a menu) claims is the
            // host's: the pane must not also read it as "close the rail".
            if ev.key() == "Escape" && app_chrome::floating::dismiss::escape_is_claimed() {
                return;
            }
            let Some(active) = panes()
                .into_iter()
                .find(|inner| inner.env.active.try_get_untracked() == Some(true))
            else {
                return;
            };
            if active.live.borrow().is_none() {
                if ev.key().eq_ignore_ascii_case("o")
                    && (ev.meta_key() || ev.ctrl_key())
                    && !ev.repeat()
                {
                    ev.prevent_default();
                    crate::services::document::open::open_dialog(
                        active.ctx,
                        crate::host::contract::Placement::Here,
                    );
                }
                return;
            }
            active.post_live(&HostToPane::Key(Key {
                up: ev.type_() == "keyup",
                repeat: ev.repeat(),
                key: ev.key(),
                code: ev.code(),
                meta: ev.meta_key(),
                ctrl: ev.ctrl_key(),
                alt: ev.alt_key(),
                shift: ev.shift_key(),
            }));
        });
    if let Some(win) = web_sys::window() {
        // Bubble phase: a live drag's capture listener cancels on Escape and
        // marks the press consumed before this forward sees it. A modal's
        // claim is released by a reactive cleanup, after this dispatch.
        for name in ["keydown", "keyup"] {
            let _ = win.add_event_listener_with_callback(name, listener.as_ref().unchecked_ref());
        }
    }
    KEYS.with(|k| *k.borrow_mut() = Some(listener));
}

/// Whether a key event belongs to a host text field or an open host menu.
fn typing(ev: &web_sys::KeyboardEvent) -> bool {
    let Some(target) = ev
        .target()
        .and_then(|t| t.dyn_into::<web_sys::Element>().ok())
    else {
        return false;
    };
    target
        .closest("input, textarea, select, [contenteditable], [role=menu], [role=dialog]")
        .ok()
        .flatten()
        .is_some()
}

fn drop_listeners() {
    let Some(win) = web_sys::window() else {
        return;
    };
    if let Some(listener) = HELLO.with(|h| h.borrow_mut().take()) {
        let _ =
            win.remove_event_listener_with_callback("message", listener.as_ref().unchecked_ref());
    }
    if let Some(listener) = KEYS.with(|k| k.borrow_mut().take()) {
        for name in ["keydown", "keyup"] {
            let _ =
                win.remove_event_listener_with_callback(name, listener.as_ref().unchecked_ref());
        }
    }
}

/// Pane `id` took focus: first in the recency order the shared paper
/// follows.
fn board_touch(id: PaneId) {
    PANES.with(|p| {
        let mut panes = p.borrow_mut();
        if let Some(at) = panes
            .iter()
            .position(|w| w.upgrade().is_some_and(|inner| inner.id == id))
        {
            let entry = panes.remove(at);
            panes.insert(0, entry);
        }
    });
    board_refresh();
}

/// The paper every pane shows in blend: the most recently focused pane
/// that has one.
fn board_paper() -> Option<Paper> {
    panes().iter().find_map(|inner| inner.paper())
}

/// Recompute the shared paper; when it changed, paint the host's root with
/// it and hand it to every pane.
fn board_refresh() {
    let paper = board_paper();
    let changed = SHARED_PAPER.with(|s| *s.borrow() != paper);
    if !changed {
        return;
    }
    SHARED_PAPER.with(|s| *s.borrow_mut() = paper.clone());
    if let Some(style) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.document_element())
        .and_then(|el| el.dyn_into::<web_sys::HtmlElement>().ok())
        .map(|el| el.style())
    {
        match &paper {
            Some(p) => {
                let _ = style.set_property("--pdf-paper", &p.raw);
                let _ = style.set_property("--pdf-paper-baked", &p.baked);
            }
            None => {
                let _ = style.remove_property("--pdf-paper");
                let _ = style.remove_property("--pdf-paper-baked");
            }
        }
    }
    for inner in panes() {
        inner.broadcast(&HostToPane::Paper {
            paper: paper.clone(),
        });
    }
}

/// The appearance menu's engine hooks, answered by the PDF panes' frames.
struct FrameHooks;

impl app_chrome::appearance_hooks::AppearanceEngineHooks for FrameHooks {
    fn refresh_theme(&self) {
        broadcast_hook(Hook::Refresh);
    }
    fn set_scrub_mode(&self, on: bool) {
        broadcast_hook(Hook::Scrub { on });
    }
    fn set_appearance_menu_open(&self, on: bool) {
        broadcast_hook(Hook::MenuOpen { on });
    }
}

/// Select the realm here: a host DOM scope id has no ancestor inside an
/// iframe. End messages visit all realms so a focus/scope change cannot
/// strand a pane in scrub or raw-retention mode.
fn hook_targets_pane(hook: Hook, pane: u64, scrub: Option<u64>, menu: Option<u64>) -> bool {
    match hook {
        Hook::Scrub { on: true } => scrub.is_none_or(|id| id == pane),
        Hook::MenuOpen { on: true } => menu.is_none_or(|id| id == pane),
        _ => true,
    }
}

fn broadcast_hook(hook: Hook) {
    let held = panes();
    let scrub = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.document_element())
        .and_then(|e| e.get_attribute("data-appearance-scope"))
        .and_then(|id| id.parse::<u64>().ok());
    let menu = held.iter().find_map(|inner| {
        (inner.env.workspace.with_untracked(|w| w.independent) && inner.env.active.get_untracked())
            .then_some(inner.id.get())
    });
    for inner in held {
        if !hook_targets_pane(hook, inner.id.get(), scrub, menu) {
            continue;
        }
        let mut state = inner.hooks.get();
        match hook {
            Hook::Scrub { on } => state.scrubbing = on,
            Hook::MenuOpen { on } => state.menu_open = on,
            Hook::Refresh => {}
        }
        inner.hooks.set(state);
        inner.broadcast(&HostToPane::Hook(hook));
    }
}

/// Install the frame panes' engine hooks for the session; the guard's drop
/// takes them away with it.
pub fn install_hooks() -> app_chrome::appearance_hooks::AppearanceHooksGuard {
    app_chrome::appearance_hooks::install(Rc::new(FrameHooks))
}

/// The pane frames' diagnostics digests: the live panes' latest, and the
/// final ones of panes since disposed. The host's own digest sums them.
pub fn digests() -> (Vec<String>, Vec<String>) {
    // Every frame still in the document speaks: a disposed pane's frames
    // run until they answer, and their work is not done before that.
    let disposing: Vec<Rc<Inner>> =
        DISPOSING.with(|d| d.borrow().iter().filter_map(Weak::upgrade).collect());
    let mut live = Vec::new();
    for inner in panes().iter().chain(disposing.iter()) {
        live.extend(inner.live.borrow().as_ref().and_then(Frame::fresh_digest));
        live.extend(
            inner
                .incoming
                .borrow()
                .as_ref()
                .and_then(Frame::fresh_digest),
        );
        live.extend(
            inner
                .retired
                .borrow()
                .iter()
                .filter_map(Frame::fresh_digest),
        );
    }
    let finals = FINALS.with(|f| f.borrow().iter().cloned().collect());
    (live, finals)
}

/// The live frame pane `id`'s thumbnails, for the rail's glide prefetch.
pub fn prefetch_thumbs(id: PaneId, pages: impl IntoIterator<Item = u32>) {
    if let Some(inner) = panes().into_iter().find(|inner| inner.id == id) {
        for page in pages {
            inner.post_live(&HostToPane::ThumbPrefetch { page });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handoff_requires_real_document_paint_or_a_visible_error() {
        assert!(!handoff_ready(false, Some((DocStatus::Ready, true))));
        assert!(!handoff_ready(true, None));
        assert!(!handoff_ready(true, Some((DocStatus::Opening, true))));
        assert!(!handoff_ready(true, Some((DocStatus::Ready, false))));
        assert!(handoff_ready(true, Some((DocStatus::Ready, true))));
        assert!(handoff_ready(true, Some((DocStatus::Error, false))));
    }

    #[test]
    fn scoped_appearance_starts_only_its_pane_and_ends_everywhere() {
        let scrub = Hook::Scrub { on: true };
        assert!(hook_targets_pane(scrub, 7, Some(7), None));
        assert!(!hook_targets_pane(scrub, 8, Some(7), None));
        assert!(hook_targets_pane(scrub, 8, None, None));
        let menu = Hook::MenuOpen { on: true };
        assert!(hook_targets_pane(menu, 7, None, Some(7)));
        assert!(!hook_targets_pane(menu, 8, None, Some(7)));
        for pane in [7, 8] {
            assert!(hook_targets_pane(
                Hook::Scrub { on: false },
                pane,
                Some(7),
                Some(7)
            ));
            assert!(hook_targets_pane(
                Hook::MenuOpen { on: false },
                pane,
                Some(7),
                Some(7)
            ));
        }
    }
}

