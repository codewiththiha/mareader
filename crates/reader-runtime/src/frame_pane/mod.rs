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
//! Frames: the LIVE frame is the one on screen; an in-place open of the
//! other runtime kind boots an INCOMING frame behind it and swaps the two on
//! its first paint; RETIRED frames are disposing and are removed when they
//! say so (or a timeout passes).

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
    ChromeSlot, LiftStep, OpenRequest, PaneAppearance, PaneBuild, PaneCommand, PaneDocStatus,
    PaneEnv, PaneFactory, PaneResourceCounts, PaneRuntime, PaneSite, PaneSurface, PaneTeardown,
};
use crate::host::model::{
    DocumentId, PaneBounds, PaneDescriptor, PaneError, PaneFormat, PaneId, PaneLifecycle,
};
use crate::pane_wire::{
    Boot, Hook, HostToPane, Key, Mirror, PANE_CHANNEL_KIND, PANE_HELLO_KIND, PaneKind, PaneToHost,
    Paper, WireSidebar, Write, encode,
};
use pdf_engine::types::DocStatus;
use thumbs::RemoteThumbs;

/// How long a disposing frame may take to say it is done before the host
/// removes it regardless (ms).
const DISPOSE_TIMEOUT_MS: f64 = 1500.0;

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
    kind: PaneKind,
    nonce: String,
    iframe: web_sys::HtmlIFrameElement,
    port: Option<web_sys::MessagePort>,
    on_message: Option<Closure<dyn FnMut(web_sys::MessageEvent)>>,
    boot: Option<Boot>,
    painted: bool,
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
    fn post(&self, message: &HostToPane) {
        if let (Some(port), Some(json)) = (self.port.as_ref(), encode(message)) {
            let _ = port.post_message(&JsValue::from_str(&json));
        }
    }

    fn reveal(&self) {
        let _ = self.iframe.remove_attribute("data-frame-hidden");
    }

    /// Take the frame out of the document and drop its port: its realm is
    /// collected with it.
    fn remove(&mut self) {
        if let Some(port) = self.port.take() {
            port.set_onmessage(None);
            port.close();
        }
        self.on_message = None;
        self.iframe.remove();
        forget_nonce(&self.nonce);
        if let Some(json) = self.digest.take() {
            FINALS.with(|f| f.borrow_mut().push(json));
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
    requested: Cell<PaneFormat>,
    disposed: Cell<bool>,
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
            provide_context(handle);
            let reader = crate::state::ReaderState::new(handle);
            let first = launch
                .clone()
                .unwrap_or_else(|| crate::services::document::open::bare_launch(""));
            let ctx = ReaderContext {
                reader,
                pane: handle,
                settings: env.settings,
                ui: env.ui,
                api: env.api,
                launch: RwSignal::new(first),
                id: env.session_id,
                chrome: env.chrome,
                open: env.open,
                can_split: env.can_split,
                moves: env.moves,
                relocate: env.relocate,
            };
            let status = reader.document.status;
            let error = reader.document.error;
            let surface = PaneSurface {
                status: Signal::derive(move || neutral_status(status.get())),
                error: Signal::derive(move || error.get()),
                page: reader.viewer.page.into(),
                reflowable: Signal::derive(move || reader.reflowable()),
                search_visible: reader.search.visible.into(),
                name: Signal::derive(move || reader.document.display_name()),
            };
            (ctx, surface, RemoteThumbs::new())
        });
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
            requested: Cell::new(descriptor.format),
            disposed: Cell::new(false),
        });
        thumbs.attach(Rc::downgrade(&inner));
        let launch = launch.filter(|_| descriptor.document.is_some());
        let boot = Boot {
            pane_id: id.get(),
            session_id: env.session_id,
            format: descriptor.format,
            launch: launch.clone(),
            initial_page: descriptor.initial_page,
            initial_zoom: descriptor.initial_zoom,
            settings: env.settings.get_untracked(),
            motion: Default::default(),
            look: None,
            workspace: env.workspace.get_untracked(),
            paper: board_paper(),
            active: env.active.get_untracked(),
            can_split: env.can_split.get_untracked(),
            moves: env.moves.get_untracked(),
            sidebar: WireSidebar::from(env.ui.sidebar.get_untracked()),
            settings_open: env.settings_open.get_untracked(),
        };
        let kind = launch
            .as_ref()
            .map_or(PaneKind::for_format(descriptor.format), |l| {
                PaneKind::for_path(&l.path)
            });
        *inner.live.borrow_mut() = Some(new_frame(&inner, kind, boot));
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
    let _ = iframe.set_attribute("data-pane-frame", &inner.id.get().to_string());
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
    Frame {
        kind,
        nonce,
        iframe,
        port: None,
        on_message: None,
        boot: Some(boot),
        painted: false,
        mirror: None,
        outline: None,
        paper: None,
        digest: None,
        disposed: Rc::new(Cell::new(false)),
    }
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
    // The rail's pictures were baked against the old look. (After the
    // settings broadcast: the frame re-bakes before it renders again.)
    let look = Memo::new(move |_| env.settings.with(|s| s.appearance));
    let w = weak.clone();
    Effect::new(move |previous: Option<()>| {
        look.track();
        if previous.is_some()
            && let Some(inner) = w.upgrade()
        {
            inner.thumbs.invalidate();
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
    let w = weak;
    Effect::new(move |_| {
        let on = search.visible.get();
        if let Some(inner) = w.upgrade() {
            inner.write_if(|m| m.search_visible != on, Write::SearchVisible { on });
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
        self.with_frame(nonce, move |frame| {
            frame.port = Some(port);
            frame.on_message = Some(on_message);
            if let Some(mut boot) = frame.boot.take() {
                if let Some(appearance) = appearance {
                    boot.motion = appearance.motion.into();
                    boot.look = appearance.look;
                }
                boot.paper = board_paper();
                frame.post(&HostToPane::Boot(Box::new(boot)));
            }
            frame.post(&HostToPane::Lifecycle { lifecycle });
        });
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
                return;
            }
            PaneToHost::Api { envelope } => {
                // A retired frame's last words still count: its final read
                // point and digest.
                self.api(nonce, role, &envelope);
                return;
            }
            _ if role == Role::Retired => return,
            PaneToHost::Painted => {
                self.with_frame(nonce, |frame| frame.painted = true);
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
                self.with_frame(nonce, |frame| frame.mirror = Some(mirror.clone()));
                match role {
                    Role::Live => self.apply_mirror(mirror),
                    Role::Incoming => self.try_swap(),
                    Role::Retired => {}
                }
            }
            PaneToHost::Outline { entries } => {
                if role == Role::Live {
                    self.apply_outline(&entries);
                }
                self.with_frame(nonce, |frame| frame.outline = Some(entries));
            }
            PaneToHost::Paper(paper) => {
                self.with_frame(nonce, |frame| frame.paper = Some(paper));
                if role == Role::Live {
                    board_refresh();
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
            RuntimeFrame::OpenDocument { launch } => api.open_document(&launch),
            RuntimeFrame::NavigateLibrary => api.navigate_library(),
            RuntimeFrame::ReadPoint { point } => api.read_point(&point),
            RuntimeFrame::SaveSettings { .. } => {}
            RuntimeFrame::SaveCover { path, image } => api.save_cover(&path, &image),
            RuntimeFrame::SaveGloss { key, marks } => api.save_gloss(&key, marks),
            RuntimeFrame::BakeCover { path } => api.bake_cover(&path),
            RuntimeFrame::DocStatus { report } if role == Role::Live => api.doc_status(&report),
            RuntimeFrame::Reload => api.reload(),
            _ => {}
        }
    }

    /// The live frame's report, written into the mirror. `reported` is set
    /// first, so the write-forwarding effects see their own value.
    fn apply_mirror(&self, m: Mirror) {
        *self.reported.borrow_mut() = Some(m.clone());
        let reader = self.ctx.reader;
        let document = &reader.document;
        put(document.status, m.status);
        put(document.format, m.format);
        put(document.error, m.error);
        put(document.path, m.path);
        put(document.book_id, m.book_id);
        put(document.title, m.title);
        put(document.author, m.author);
        put(document.num_pages, m.num_pages);
        put(document.outline_pending, m.outline_pending);
        put(document.content.metrics.page1_size, m.page1);
        put(reader.viewer.page, m.page);
        put(reader.viewer.mode, m.mode);
        put(reader.viewer.fit, m.fit);
        put(reader.viewer.zoom.display, m.zoom);
        put(reader.viewer.auto_scroll, m.auto_scroll);
        put(reader.search.visible, m.search_visible);
        put(reader.viewer.first_paint, m.first_paint);
        let launch_changed = self.ctx.launch.try_with_untracked(|l| *l != m.launch) == Some(true);
        if launch_changed {
            // Another document: the rail's pictures are another book's.
            self.thumbs.reset();
            self.ctx.launch.set(m.launch);
        }
    }

    fn apply_outline(&self, entries: &crate::pane_wire::WireOutline) {
        let nodes: Vec<reader_core::outline::OutlineNode> = entries
            .iter()
            .map(|(title, page, depth)| {
                reader_core::outline::OutlineNode::new(title.clone(), *page, *depth)
            })
            .collect();
        let outline = self.ctx.reader.document.outline;
        if outline.try_with_untracked(|o| o.as_slice() != nodes.as_slice()) == Some(true) {
            outline.set(std::sync::Arc::new(nodes));
        }
    }

    /// An incoming frame takes over once it has painted AND its document
    /// is on screen (or failed): the swap is one frame, never a blank.
    fn try_swap(self: &Rc<Self>) {
        let ready = self.incoming.borrow().as_ref().is_some_and(|f| {
            f.painted
                && f.mirror.as_ref().is_some_and(|m| {
                    (m.status == DocStatus::Ready && m.first_paint) || m.status == DocStatus::Error
                })
        });
        if !ready {
            return;
        }
        let Some(incoming) = self.incoming.borrow_mut().take() else {
            return;
        };
        incoming.reveal();
        let mirror = incoming.mirror.clone();
        let outline = incoming.outline.clone();
        if let Some(old) = self.live.borrow_mut().replace(incoming) {
            self.retire(old);
        }
        self.thumbs.reset();
        if let Some(mirror) = mirror {
            self.apply_mirror(mirror);
        }
        self.apply_outline(&outline.unwrap_or_default());
        board_refresh();
    }

    /// Send a frame its `Dispose` and keep it (hidden) until it answers.
    fn retire(self: &Rc<Self>, frame: Frame) {
        let _ = frame.iframe.set_attribute("data-frame-hidden", "");
        frame.post(&HostToPane::Dispose);
        let disposed = frame.disposed.clone();
        self.retired.borrow_mut().push(frame);
        let weak = Rc::downgrade(self);
        spawn_local(async move {
            wait_until(DISPOSE_TIMEOUT_MS, move || disposed.get()).await;
            if let Some(inner) = weak.upgrade() {
                inner.sweep_retired_all();
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

fn neutral_status(status: DocStatus) -> PaneDocStatus {
    match status {
        DocStatus::Idle => PaneDocStatus::Idle,
        DocStatus::Opening => PaneDocStatus::Opening,
        DocStatus::Ready => PaneDocStatus::Ready,
        DocStatus::Error => PaneDocStatus::Error,
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
        view! { <div node_ref=container class="absolute inset-0" data-pane-root-host="" /> }
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
                let kind = PaneKind::for_path(&launch.path);
                let same = inner.live.borrow().as_ref().is_some_and(|f| f.kind == kind);
                if same {
                    inner.post_live(&HostToPane::Open(launch));
                } else {
                    // The other runtime: boot it behind the current frame,
                    // swap on its first paint of the document.
                    let frame = new_frame(inner, kind, self.fresh_boot(*launch));
                    if let Some(old) = inner.incoming.borrow_mut().replace(frame) {
                        inner.retire(old);
                    }
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
        unregister(inner.id);
        board_refresh();
        inner.thumbs.reset();
        let mut frames: Vec<Frame> = Vec::new();
        frames.extend(inner.live.borrow_mut().take());
        frames.extend(inner.incoming.borrow_mut().take());
        frames.extend(inner.retired.borrow_mut().drain(..));
        for frame in &frames {
            frame.post(&HostToPane::Dispose);
        }
        inner.owner.pause();
        inner.owner.cleanup();
        crate::diagnostics::note_pane_dispose();
        drop(inner);
        Box::pin(async move {
            let flags: Vec<(Rc<Cell<bool>>, web_sys::HtmlIFrameElement)> = frames
                .iter()
                .map(|f| (f.disposed.clone(), f.iframe.clone()))
                .collect();
            // Done when every frame said so, or left the document with the
            // workspace (the session's end), or the timeout passed.
            wait_until(DISPOSE_TIMEOUT_MS, move || {
                flags
                    .iter()
                    .all(|(done, iframe)| done.get() || !iframe.is_connected())
            })
            .await;
            for mut frame in frames {
                frame.remove();
            }
        })
    }
}

impl FramePane {
    /// A boot for a replacement frame: the pane's current facts, the new
    /// launch.
    fn fresh_boot(&self, launch: LaunchDocument) -> Boot {
        let inner = &self.inner;
        let env = inner.env;
        let appearance = inner.appearance.get();
        Boot {
            pane_id: inner.id.get(),
            session_id: env.session_id,
            format: crate::pane::document::classify(&launch.path),
            initial_page: launch.resume_page.max(1),
            launch: Some(launch),
            initial_zoom: None,
            settings: env.settings.get_untracked(),
            motion: appearance.map(|a| a.motion.into()).unwrap_or_default(),
            look: appearance.and_then(|a| a.look),
            workspace: env.workspace.get_untracked(),
            paper: board_paper(),
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
    static FINALS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
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
            let Some(active) = panes()
                .into_iter()
                .find(|inner| inner.env.active.try_get_untracked() == Some(true))
            else {
                return;
            };
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
    if let Some(paper) = paper {
        for inner in panes() {
            inner.broadcast(&HostToPane::Paper(paper.clone()));
        }
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

fn broadcast_hook(hook: Hook) {
    for inner in panes() {
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
    let live = panes()
        .iter()
        .filter_map(|inner| inner.live.borrow().as_ref().and_then(|f| f.digest.clone()))
        .collect();
    let finals = FINALS.with(|f| f.borrow().clone());
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
