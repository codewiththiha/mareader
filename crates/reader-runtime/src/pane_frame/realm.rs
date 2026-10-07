//! The wasm half of the pane realm: the handshake, the port, the pane and
//! the effects that keep the host's mirror current.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use app_state::state::UiState;
use app_ui::components::primitives::overlay::lanes::OverlayBoard;
use frame_transport::PortShellApi;
use leptos::prelude::*;
use reader_core::settings::Settings;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};

use super::PaneApiWire;
use crate::host::contract::{
    LiftPhase, OpenRequest, PaneAppearance, PaneCommand, PaneRuntime, PaneSite, PaneTeardown,
    WorkspaceLook,
};
use crate::host::model::{
    DocumentId, DocumentRef, PaneBounds, PaneDescriptor, PaneId, PaneRequest,
};
use crate::host::tree::Moves;
use crate::pane::document::DocumentPane;
#[cfg(feature = "pdf")]
use crate::pane_wire::Hook;
use crate::pane_wire::{
    Boot, HostToPane, Key, Mirror, PANE_CHANNEL_KIND, PANE_HELLO_KIND, PaneKind, PaneToHost, Paper,
    WireSidebar, Write, encode,
};

/// The port and the shell api over it, once the host handed the port over.
struct Link {
    port: web_sys::MessagePort,
    api: Rc<PortShellApi<PaneApiWire>>,
    /// The port's message handler, owned for as long as the port is open.
    _on_message: Closure<dyn FnMut(web_sys::MessageEvent)>,
}

/// What the message handler reaches once the pane exists: Copy handles
/// onto the pane realm's signals, and the pane itself.
#[derive(Clone)]
struct Live {
    #[cfg(feature = "pdf")]
    kind: PaneKind,
    pane: Rc<DocumentPane>,
    settings: RwSignal<Settings>,
    ui: UiState,
    active: RwSignal<bool>,
    settings_open: RwSignal<bool>,
    can_split: RwSignal<bool>,
    moves: RwSignal<Moves>,
    workspace: RwSignal<WorkspaceLook>,
    paper: RwSignal<Option<Paper>>,
}

/// The values the host last sent, so a local signal written from a host
/// message never echoes back as a pane change.
#[derive(Default)]
struct Heard {
    settings: Option<Settings>,
    sidebar: Option<WireSidebar>,
    settings_open: Option<bool>,
}

thread_local! {
    static LINK: RefCell<Option<Link>> = const { RefCell::new(None) };
    static LIVE: RefCell<Option<Live>> = const { RefCell::new(None) };
    /// Where the digest beat publishes; the final digest after disposal
    /// goes the same way.
    static DIGEST_API: Cell<Option<crate::context::ApiHandle>> = const { Cell::new(None) };
    static DISPOSED: Cell<bool> = const { Cell::new(false) };
    static HEARD: RefCell<Heard> = RefCell::new(Heard::default());
    /// The pane's mount, dropped at dispose (its cleanup runs the runtime's
    /// disposal with the pane's teardown tail).
    static UNMOUNT: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
    /// The pane's async teardown, handed to the runtime's disposal.
    static TEARDOWN: RefCell<Option<PaneTeardown>> = const { RefCell::new(None) };
}

/// Post one message to the host. Dropped when no port is adopted yet or the
/// host already closed it: nothing on this side can revive a dead port.
pub(super) fn send(message: &PaneToHost) {
    let Some(json) = encode(message) else {
        return;
    };
    LINK.with(|l| {
        if let Some(link) = l.borrow().as_ref() {
            let _ = link.port.post_message(&JsValue::from_str(&json));
        }
    });
}

/// Post a raw object (a thumbnail with its transferred bitmap).
#[cfg(feature = "pdf")]
pub(super) fn send_object(message: &JsValue, transfer: &JsValue) -> bool {
    LINK.with(|l| {
        l.borrow().as_ref().is_some_and(|link| {
            link.port
                .post_message_with_transferable(message, &js_sys::Array::of1(transfer))
                .is_ok()
        })
    })
}

pub(super) fn with_api<R>(f: impl FnOnce(&PortShellApi<PaneApiWire>) -> R) -> Option<R> {
    let api = LINK.with(|l| l.borrow().as_ref().map(|link| link.api.clone()))?;
    Some(f(&api))
}

/// The pane's live document pane, for the thumbnail lane.
#[cfg(feature = "pdf")]
pub(super) fn pane() -> Option<Rc<DocumentPane>> {
    LIVE.with(|l| l.borrow().as_ref().map(|live| live.pane.clone()))
}

pub(super) fn boot(kind: PaneKind) {
    console_error_panic_hook::set_once();
    let Some(window) = web_sys::window() else {
        return;
    };
    let Some(nonce) = window
        .location()
        .search()
        .ok()
        .and_then(|search| web_sys::UrlSearchParams::new_with_str(&search).ok())
        .and_then(|params| params.get("pane"))
    else {
        return;
    };
    let Ok(Some(parent)) = window.parent() else {
        return;
    };

    // The host answers the hello with the port, tagged with the same nonce.
    // The listener goes the moment the port arrives.
    let slot: Rc<RefCell<Option<Closure<dyn FnMut(web_sys::MessageEvent)>>>> =
        Rc::new(RefCell::new(None));
    let slot_in = slot.clone();
    let expected = nonce.clone();
    let on_offer =
        Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |ev: web_sys::MessageEvent| {
            let from_parent = web_sys::window()
                .and_then(|w| w.parent().ok().flatten())
                .is_some_and(|parent| {
                    ev.source()
                        .is_some_and(|source| JsValue::from(source) == JsValue::from(parent))
                });
            if !from_parent {
                return;
            }
            let data = ev.data();
            let tag = js_sys::Reflect::get(&data, &JsValue::from_str("kind"))
                .ok()
                .and_then(|v| v.as_string());
            let offered = js_sys::Reflect::get(&data, &JsValue::from_str("nonce"))
                .ok()
                .and_then(|v| v.as_string());
            if tag.as_deref() != Some(PANE_CHANNEL_KIND) || offered.as_deref() != Some(&expected) {
                return;
            }
            let Some(port) = ev.ports().get(0).dyn_into::<web_sys::MessagePort>().ok() else {
                return;
            };
            if let Some(listener) = slot_in.borrow_mut().take()
                && let Some(window) = web_sys::window()
            {
                let _ = window.remove_event_listener_with_callback(
                    "message",
                    listener.as_ref().unchecked_ref(),
                );
            }
            adopt(kind, port);
        });
    let _ = window.add_event_listener_with_callback("message", on_offer.as_ref().unchecked_ref());
    *slot.borrow_mut() = Some(on_offer);

    let hello = js_sys::Object::new();
    let _ = js_sys::Reflect::set(&hello, &"kind".into(), &PANE_HELLO_KIND.into());
    let _ = js_sys::Reflect::set(&hello, &"nonce".into(), &nonce.into());
    let origin = window.location().origin().unwrap_or_else(|_| "*".into());
    let _ = parent.post_message(&hello, &origin);
}

/// The port arrived: listen on it, and boot the pane on the host's `Boot`.
fn adopt(kind: PaneKind, port: web_sys::MessagePort) {
    let on_message =
        Closure::<dyn FnMut(web_sys::MessageEvent)>::new(move |ev: web_sys::MessageEvent| {
            let Some(json) = ev.data().as_string() else {
                return;
            };
            match serde_json::from_str::<HostToPane>(&json) {
                Ok(message) => receive(kind, message),
                Err(err) => {
                    web_sys::console::warn_1(&format!("[pane] bad message: {err}").into());
                }
            }
        });
    port.set_onmessage(Some(on_message.as_ref().unchecked_ref()));
    let api = Rc::new(PortShellApi::new(PaneApiWire, 0));
    LINK.with(|l| {
        *l.borrow_mut() = Some(Link {
            port,
            api,
            _on_message: on_message,
        });
    });
}

fn receive(kind: PaneKind, message: HostToPane) {
    if DISPOSED.with(Cell::get) {
        return;
    }
    if let HostToPane::Boot(boot) = message {
        if LIVE.with(|l| l.borrow().is_none()) {
            start(kind, *boot);
        }
        return;
    }
    let Some(live) = LIVE.with(|l| l.borrow().clone()) else {
        return;
    };
    match message {
        HostToPane::Boot(_) => {}
        HostToPane::Settings(settings) => {
            HEARD.with(|h| h.borrow_mut().settings = Some((*settings).clone()));
            put(live.settings, *settings);
        }
        HostToPane::Appearance { motion, look } => live.pane.appearance(PaneAppearance {
            motion: motion.into(),
            look,
        }),
        HostToPane::Workspace(look) => put(live.workspace, look),
        HostToPane::Paper { paper } => put(live.paper, paper),
        HostToPane::Active { on } => {
            put(live.active, on);
            if on {
                live.pane.focus();
            } else {
                live.pane.blur();
            }
        }
        HostToPane::Layout { can_split, moves } => {
            put(live.can_split, can_split);
            put(live.moves, moves);
        }
        HostToPane::Sidebar { mode } => {
            HEARD.with(|h| h.borrow_mut().sidebar = Some(mode));
            put(live.ui.sidebar, mode.into());
        }
        HostToPane::SettingsOpen { on } => {
            HEARD.with(|h| h.borrow_mut().settings_open = Some(on));
            put(live.settings_open, on);
        }
        HostToPane::Lifecycle { lifecycle } => live.pane.lifecycle_changed(lifecycle),
        HostToPane::Write(write) => apply_write(&live, write),
        #[cfg(feature = "pdf")]
        HostToPane::Hook(hook) => apply_hook(live.kind, hook),
        #[cfg(not(feature = "pdf"))]
        HostToPane::Hook(_) => {}
        HostToPane::PrepareLeave => {
            let _ = live.pane.command(PaneCommand::PrepareLeave);
        }
        #[cfg(feature = "pdf")]
        HostToPane::Thumb { req, page } => super::thumbs::render(req, page),
        #[cfg(feature = "pdf")]
        HostToPane::ThumbCancel { req } => super::thumbs::cancel(req),
        #[cfg(feature = "pdf")]
        HostToPane::ThumbPrefetch { page } => super::thumbs::prefetch(page),
        #[cfg(not(feature = "pdf"))]
        HostToPane::Thumb { req, .. } => send(&PaneToHost::ThumbFailed {
            req,
            cancelled: false,
        }),
        #[cfg(not(feature = "pdf"))]
        HostToPane::ThumbCancel { .. } | HostToPane::ThumbPrefetch { .. } => {}
        HostToPane::Key(key) => dispatch_key(&key),
        HostToPane::Dispose => dispose(live),
    }
}

/// Set a signal only when the value changed: a host echo of the pane's own
/// value must not re-run everything that reads it.
fn put<T: PartialEq + Send + Sync + 'static>(signal: RwSignal<T>, value: T) {
    if signal.try_with_untracked(|v| *v != value) == Some(true) {
        signal.set(value);
    }
}

fn apply_write(live: &Live, write: Write) {
    let reader = live.pane.context().reader;
    match write {
        Write::Page { page } => put(reader.viewer.page, page),
        Write::Mode { mode } => put(reader.viewer.mode, mode),
        Write::Fit { fit } => put(reader.viewer.fit, fit),
        Write::ZoomStep { step } => reader
            .viewer
            .zoom
            .post(crate::state::zoom::ZoomCommand::Step(step), true),
        Write::AutoScroll { on } => put(reader.viewer.auto_scroll, on),
        Write::SearchVisible { on } => put(reader.search.visible, on),
        // The outline's jump is a directive, not mirrored state: the arm that
        // owns the stream's geometry consumes it
        // (`effects::reader::outline_jump`), and an index no heading answers
        // is not a jump.
        Write::Outline { index } => put(reader.viewer.outline_jump, Some(index)),
    }
}

#[cfg(feature = "pdf")]
fn apply_hook(kind: PaneKind, hook: Hook) {
    // Only a PDF realm has an engine to re-bake or scrub.
    if kind != PaneKind::Pdf {
        return;
    }
    match hook {
        Hook::Refresh => crate::appearance_hooks::refresh(),
        Hook::Scrub { on } => {
            super::thumbs::set_scrubbing(on);
            pdf_engine::api::set_scrub_mode(on);
            // The drag is over: the exit settles the look the drag landed on
            // into every raster, which is the bake the rail's pictures — raw
            // ones from the drag, or the bake it started from — never saw.
            if !on {
                crate::pane_frame::pictures_stale();
            }
        }
        Hook::MenuOpen { on } => pdf_engine::api::set_appearance_menu_open(on),
    }
}

/// A key the host document received: replayed on this document's body, so
/// the pane's own keyboard arm (a window listener) answers it.
fn dispatch_key(key: &Key) {
    let Some(body) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.body())
    else {
        return;
    };
    let init = web_sys::KeyboardEventInit::new();
    init.set_key(&key.key);
    init.set_code(&key.code);
    init.set_meta_key(key.meta);
    init.set_ctrl_key(key.ctrl);
    init.set_alt_key(key.alt);
    init.set_shift_key(key.shift);
    init.set_repeat(key.repeat);
    init.set_bubbles(true);
    init.set_cancelable(true);
    let kind = if key.up { "keyup" } else { "keydown" };
    if let Ok(event) = web_sys::KeyboardEvent::new_with_keyboard_event_init_dict(kind, &init) {
        let _ = body.dispatch_event(&event);
    }
}

/// The host's `Boot`: build the pane realm's session and its one pane.
fn start(kind: PaneKind, boot: Boot) {
    crate::diagnostics::install();
    HEARD.with(|h| {
        let mut heard = h.borrow_mut();
        heard.settings = Some(boot.settings.clone());
        heard.sidebar = Some(boot.sidebar);
        heard.settings_open = Some(boot.settings_open);
    });
    let Some(body) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.body())
    else {
        return;
    };
    let handle = mount_to(body, move || build(kind, boot));
    UNMOUNT.with(|u| *u.borrow_mut() = Some(Box::new(move || drop(handle))));
}

fn build(kind: PaneKind, boot: Boot) -> impl IntoView {
    let api = crate::context::ApiHandle::Pane;
    let settings = RwSignal::new(boot.settings.clone());
    let ui = UiState {
        sidebar: RwSignal::new(boot.sidebar.into()),
        toast: RwSignal::new(None),
        window_maximized: RwSignal::new(false),
    };
    let runtime = crate::runtime::ReaderRuntime::new();
    let appearance: app_state::AppearanceSignal =
        Memo::new(move |_| settings.with(|s| s.appearance));
    // The look THIS pane shows, mirrored from the pane's own signal: the pane
    // is built below, and its page hosts ask for this context while they are
    // being built, so the mirror — not the pane's copy of settings — is what
    // lets the texture memo, and with it every carrier's `texture-*` class,
    // follow the LOOK the host routed to this pane. A per-pane edit lands in
    // that look and never in settings, which is why deriving the mode from
    // settings made a split's texture picker move both dials and no pattern.
    // Seeded from `boot.look` so the first paint is already the right one.
    let look = RwSignal::new(boot.look);
    let texture: crate::state::TextureSignal = Memo::new(move |_| {
        look.get()
            .map(|a| a.texture)
            .unwrap_or_else(|| appearance.get().texture)
    });
    provide_context(texture);
    let typography: crate::state::TypographySignal =
        Memo::new(move |_| settings.with(|s| s.text.clone()));
    provide_context(typography);
    provide_context(OverlayBoard::default());
    runtime.begin_mount();

    let active = RwSignal::new(boot.active);
    let settings_open = RwSignal::new(boot.settings_open);
    let can_split = RwSignal::new(boot.can_split);
    let moves = RwSignal::new(boot.moves);
    let workspace = RwSignal::new(boot.workspace.clone());
    let paper = RwSignal::new(boot.paper.clone());
    let reflowable = RwSignal::new(kind == PaneKind::Reflow);
    let search_visible = RwSignal::new(false);
    let chrome = app_state::ChromeState {
        settings,
        ui,
        reader: app_state::ReaderSurface {
            reflowable: reflowable.into(),
            search_visible: search_visible.into(),
            sidebar_slide: RwSignal::new(boot.motion.into()),
        },
    };
    provide_context(app_ui::components::shell::controller::ShellController::reader(chrome));

    let env = crate::host::contract::PaneEnv {
        runtime,
        settings,
        ui,
        api,
        session_id: boot.session_id,
        chrome,
        active: active.into(),
        settings_open,
        request_focus: Callback::new(|_| send(&PaneToHost::Focus)),
        open: Callback::new(|request: OpenRequest| {
            send(&PaneToHost::Open {
                launch: Box::new(request.launch),
                placement: request.placement.into(),
            });
        }),
        can_split: can_split.into(),
        moves: moves.into(),
        relocate: Callback::new(|direction| send(&PaneToHost::Relocate { direction })),
        workspace: workspace.into(),
        lift: Callback::new(|step: crate::host::contract::LiftStep| {
            send(&PaneToHost::Lift {
                phase: step.phase,
                x: step.at.0,
                y: step.at.1,
            });
        }),
    };
    let id = PaneId::from_raw(boot.pane_id);
    let launch = &boot.launch;
    let document =
        DocumentId::from_launch(launch.book_id.as_deref(), &launch.path).map(|document_id| {
            DocumentRef {
                document_id,
                path: launch.path.clone(),
            }
        });
    let descriptor = PaneDescriptor::remote(
        id,
        PaneRequest {
            document,
            format: boot.format,
            initial_page: boot.initial_page,
            initial_zoom: boot.initial_zoom,
            request_focus: boot.active,
        },
    );
    let pane = Rc::new(DocumentPane::create(
        env,
        descriptor,
        Some(boot.launch.clone()),
    ));
    // This realm's own books balance: its one pane, created here, disposed
    // by `DocumentPane::dispose`.
    crate::diagnostics::note_pane_create();
    pane.appearance(PaneAppearance {
        motion: boot.motion.into(),
        look: boot.look,
    });
    let ctx = pane.context();
    // The look's own mirror, next to the chrome facts the realm borrows from
    // its pane (see `texture` above): the host pushes the look into
    // `viewer.look` and this hands it to the carriers.
    Effect::new(move |_| put(look, ctx.reader.viewer.look.get()));
    // The chrome surface the pane's own components read (the bottom bar's
    // reflowable sections, the title's search hold).
    Effect::new(move |_| put(reflowable, ctx.reader.reflowable()));
    Effect::new(move |_| put(search_visible, ctx.reader.search.visible.get()));

    // This frame's `<html>` paints from the host's settings. The host owns
    // persistence: the pane hands its own edits up (below), never to storage.
    app_ui::frame_theme::install_frame_theme(
        settings,
        app_ui::frame_theme::FramePipeline::Pane,
        |_| {},
        |_| {},
    );
    crate::diagnostics::expect_engine(kind == PaneKind::Pdf);
    #[cfg(feature = "pdf")]
    if kind == PaneKind::Pdf {
        pdf_engine::api::set_appearance_menu_open(boot.hooks.menu_open);
        pdf_engine::api::set_scrub_mode(boot.hooks.scrubbing);
        super::thumbs::set_scrubbing(boot.hooks.scrubbing);
    }
    #[cfg(feature = "pdf")]
    if kind == PaneKind::Pdf {
        let guard = crate::appearance_hooks::install();
        on_cleanup(move || drop(guard));
    }
    crate::services::ai::install_ai_chunk_bridge();

    install_upstream(settings, ui, settings_open);
    install_mirror(&pane);
    if kind == PaneKind::Pdf {
        install_paper_watch();
    }
    install_press();
    install_digest_beat(api);

    LIVE.with(|l| {
        *l.borrow_mut() = Some(Live {
            #[cfg(feature = "pdf")]
            kind,
            pane: pane.clone(),
            settings,
            ui,
            active,
            settings_open,
            can_split,
            moves,
            workspace,
            paper,
        });
    });
    on_cleanup(move || {
        let teardown = TEARDOWN
            .with(|t| t.borrow_mut().take())
            .unwrap_or_else(|| Box::pin(async {}));
        runtime.dispose(api, teardown);
    });
    runtime.mark_ready();

    let bounds = window_bounds();
    let content = untrack(|| pane.mount(bounds, PaneSite::default()));
    let weak = StoredValue::new_local(Rc::downgrade(&pane));
    let resize = window_event_listener(leptos::ev::resize, move |_| {
        if let Some(pane) = weak.try_with_value(std::rc::Weak::upgrade).flatten() {
            pane.resize(window_bounds());
        }
    });
    on_cleanup(move || resize.remove());

    let entry: NodeRef<leptos::html::Div> = NodeRef::new();
    entry.on_load(move |el| {
        let el: web_sys::Element = el.into();
        crate::host::grab::install(&el, Rc::new(FrameLift));
        signal_painted();
    });

    view! {
        <div
            class="reader-bg relative h-full w-full overflow-hidden text-ink"
            class=("independent-themes", move || workspace.with(|w| w.independent))
            class=("split-workspace", move || workspace.with(|w| w.split))
            class=("blend", move || workspace.with(|w| w.blend))
            style=move || {
                let mut style = workspace.with(|w| w.style.clone());
                if let Some(paper) = paper.get() {
                    style.push_str(&format!(
                        ";--pdf-paper:{};--pdf-paper-baked:{}",
                        paper.raw, paper.baked
                    ));
                }
                style
            }
        >
            <main
                id=app_chrome::hooks::dom::VIEWER_SLOT_ID
                class="relative h-full w-full overflow-hidden"
                class=("no-page-shadow", move || !workspace.with(|w| w.page_shadow))
            >
                <div node_ref=entry class="pane-entry absolute inset-0">
                    {content}
                </div>
            </main>
        </div>
    }
}

fn window_bounds() -> PaneBounds {
    let (w, h) = web_sys::window()
        .map(|w| {
            let w_px = w.inner_width().ok().and_then(|v| v.as_f64()).unwrap_or(0.0);
            let h_px = w
                .inner_height()
                .ok()
                .and_then(|v| v.as_f64())
                .unwrap_or(0.0);
            (w_px, h_px)
        })
        .unwrap_or_default();
    PaneBounds::filling(w, h)
}

/// The lift a hold inside this frame starts: streamed to the host, which
/// maps the points into its own document.
struct FrameLift;

impl crate::host::grab::LiftSink for FrameLift {
    fn can_lift(&self) -> bool {
        // A lift needs another pane to drop beside: exactly when the
        // workspace holds more than this one (the host's split flag).
        LIVE.with(|l| {
            l.borrow()
                .as_ref()
                .and_then(|live| live.workspace.try_with_untracked(|w| w.split))
                .unwrap_or(false)
        })
    }
    fn begin(&self, at: (f64, f64)) -> bool {
        send(&PaneToHost::Lift {
            phase: LiftPhase::Start,
            x: at.0,
            y: at.1,
        });
        true
    }
    fn moved(&self, at: (f64, f64)) {
        send(&PaneToHost::Lift {
            phase: LiftPhase::Move,
            x: at.0,
            y: at.1,
        });
    }
    fn end(&self, commit: bool) {
        send(&PaneToHost::Lift {
            phase: if commit {
                LiftPhase::End
            } else {
                LiftPhase::Cancel
            },
            x: 0.0,
            y: 0.0,
        });
    }
}

/// The pane's own writes to host-owned state go up: a settings edit (a
/// shortcut's font step), the rail it opened or closed, the settings modal
/// it asked for. A value equal to the host's last word is the host's echo.
fn install_upstream(settings: RwSignal<Settings>, ui: UiState, settings_open: RwSignal<bool>) {
    Effect::new(move |_| {
        let now = settings.get();
        let echo = HEARD.with(|h| h.borrow().settings.as_ref() == Some(&now));
        if !echo {
            HEARD.with(|h| h.borrow_mut().settings = Some(now.clone()));
            send(&PaneToHost::Settings(Box::new(now)));
        }
    });
    Effect::new(move |_| {
        let mode = WireSidebar::from(ui.sidebar.get());
        let echo = HEARD.with(|h| h.borrow().sidebar == Some(mode));
        if !echo {
            HEARD.with(|h| h.borrow_mut().sidebar = Some(mode));
            send(&PaneToHost::Sidebar { mode });
        }
    });
    Effect::new(move |_| {
        let on = settings_open.get();
        let echo = HEARD.with(|h| h.borrow().settings_open == Some(on));
        if !echo {
            HEARD.with(|h| h.borrow_mut().settings_open = Some(on));
            send(&PaneToHost::SettingsOpen { on });
        }
    });
}

/// The chrome-facing state, sent whole whenever any of it changes, and the
/// outline whenever it lands.
fn install_mirror(pane: &Rc<DocumentPane>) {
    let ctx = pane.context();
    let weak = Rc::downgrade(pane);
    let mirror = Memo::new(move |_| {
        let reader = ctx.reader;
        let document = &reader.document;
        Mirror {
            status: document.status.get(),
            format: document.format.get(),
            error: document.error.get(),
            path: document.path.get(),
            book_id: document.book_id.get(),
            title: document.title.get(),
            author: document.author.get(),
            num_pages: document.num_pages.get(),
            outline_pending: document.outline_pending.get(),
            page1: document.content.metrics.page1_size.get(),
            page: reader.viewer.page.get(),
            mode: reader.viewer.mode.get(),
            fit: reader.viewer.fit.get(),
            zoom: reader.viewer.zoom.display.get(),
            auto_scroll: reader.viewer.auto_scroll.get(),
            search_visible: reader.search.visible.get(),
            first_paint: reader.viewer.first_paint.get(),
            launch: ctx.launch.get(),
            resources: Default::default(),
        }
    });
    Effect::new(move |_| {
        let mut now = mirror.get();
        if let Some(pane) = weak.upgrade() {
            now.resources = pane.resources();
        }
        send(&PaneToHost::Mirror(Box::new(now)));
    });
    let outline = ctx.reader.document.outline;
    Effect::new(move |_| {
        let entries = outline.with(|nodes| {
            nodes
                .iter()
                .map(|n| (n.title.clone(), n.page, n.depth))
                .collect::<Vec<_>>()
        });
        send(&PaneToHost::Outline { entries });
    });
}

/// The PDF engine publishes its paper on this frame's `<html>`; the host
/// shares the focused pane's with every pane. Watched with an observer that
/// dies with the pane's owner.
fn install_paper_watch() {
    let Some(root) = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.document_element())
    else {
        return;
    };
    let last = Rc::new(RefCell::new(Paper::default()));
    let report = {
        let root = root.clone();
        move || {
            let Some(style) = root.dyn_ref::<web_sys::HtmlElement>().map(|el| el.style()) else {
                return;
            };
            let paper = Paper {
                raw: style
                    .get_property_value("--pdf-paper")
                    .unwrap_or_default()
                    .trim()
                    .to_string(),
                baked: style
                    .get_property_value("--pdf-paper-baked")
                    .unwrap_or_default()
                    .trim()
                    .to_string(),
            };
            if paper.raw.is_empty() || *last.borrow() == paper {
                return;
            }
            *last.borrow_mut() = paper.clone();
            send(&PaneToHost::Paper(paper));
        }
    };
    report();
    let callback = Closure::<dyn FnMut()>::new(report);
    let Ok(observer) = web_sys::MutationObserver::new(callback.as_ref().unchecked_ref()) else {
        return;
    };
    let init = web_sys::MutationObserverInit::new();
    init.set_attributes(true);
    init.set_attribute_filter(&js_sys::Array::of1(&"style".into()));
    let _ = observer.observe_with_options(&root, &init);
    let owned = StoredValue::new_local(Some((observer, callback)));
    on_cleanup(move || {
        owned.try_update_value(|slot| {
            if let Some((observer, _callback)) = slot.take() {
                observer.disconnect();
            }
        });
    });
}

/// A press anywhere in the frame: the host closes its own popovers on it
/// (an outside press it would otherwise never see) and focuses the pane.
fn install_press() {
    let press = window_event_listener(leptos::ev::pointerdown, |_| send(&PaneToHost::Press));
    on_cleanup(move || press.remove());
}

/// The pane's first frame is on screen: two animation frames after its
/// view attached, the paint that follows has happened.
fn signal_painted() {
    request_animation_frame(|| request_animation_frame(|| send(&PaneToHost::Painted)));
}

/// How often a live pane pushes its diagnostics digest (ms).
const DIGEST_BEAT_MS: i32 = 25;

fn install_digest_beat(api: crate::context::ApiHandle) {
    if tauri_bridge::has_tauri() {
        return;
    }
    let Some(win) = web_sys::window() else {
        return;
    };
    DIGEST_API.with(|d| d.set(Some(api)));
    let tick = Closure::<dyn FnMut()>::new(move || crate::diagnostics::publish_digest(&api));
    let Ok(id) = win.set_interval_with_callback_and_timeout_and_arguments_0(
        tick.as_ref().unchecked_ref(),
        DIGEST_BEAT_MS,
    ) else {
        return;
    };
    let owned = StoredValue::new_local(Some(tick));
    on_cleanup(move || {
        if let Some(win) = web_sys::window() {
            win.clear_interval_with_handle(id);
        }
        let _ = owned.try_set_value(None);
    });
}

/// The host closes the pane: the pane's sync teardown now (read point,
/// document session, owner), the realm's unmount, and the runtime's tail,
/// whose completion tells the host it may remove the frame.
/// How many 25 ms beats a dispose waits for cancelled engine work to settle.
const SETTLE_TRIES: u32 = 40;

/// Answer `Disposed` once the engine's cancelled work has settled (or the
/// wait ran out): the host drops the realm on that word, and the final
/// digest must count every render and prefetch the dispose cut short.
fn finish_dispose(tries: u32) {
    if tries > 0 && crate::diagnostics::engine_in_flight() {
        let next = Closure::once_into_js(move || finish_dispose(tries - 1));
        if let Some(window) = web_sys::window()
            && window
                .set_timeout_with_callback_and_timeout_and_arguments_0(next.unchecked_ref(), 25)
                .is_ok()
        {
            return;
        }
    }
    if let Some(api) = DIGEST_API.with(Cell::take) {
        crate::diagnostics::publish_digest(&api);
    }
    send(&PaneToHost::Disposed);
}

fn dispose(live: Live) {
    DISPOSED.with(|disposed| disposed.set(true));
    LIVE.with(|l| l.borrow_mut().take());
    // The realm's session ends here: its final digest must not report it.
    crate::diagnostics::set_reader_live(false);
    #[cfg(feature = "pdf")]
    super::thumbs::cancel_all();
    // The final digest goes first: it is the one the host's balances keep,
    // and only after the release does it show the session gone.
    let done = Closure::once_into_js(|| finish_dispose(SETTLE_TRIES));
    crate::on_dispose_complete(done.unchecked_into());
    let teardown = live.pane.dispose();
    TEARDOWN.with(|t| *t.borrow_mut() = Some(teardown));
    drop(live);
    if let Some(unmount) = UNMOUNT.with(|u| u.borrow_mut().take()) {
        unmount();
    }
}

