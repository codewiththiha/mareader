//! The reader runtime: one reader session's state, resources, effects and
//! UI, compiled as its own WASM artifact and mounted by the Shell's runtime
//! manager.
//!
//! A session is an explicit instance: [`start_session`] creates the reactive
//! ownership root, the session's `ReaderRuntime` (its lifecycle owner) and
//! settings/UI slices, and composes the production path inside that scope:
//!
//! ```text
//! ReaderRuntime → ReaderHost (crate::host) → PaneManager → document pane (crate::pane)
//! ```
//!
//! The host owns the workspace (chrome placement, focus, bounds, commands);
//! each pane owns one document session and builds its own
//! [`ReaderContext`]. [`dispose`] has the host dispose its panes while the
//! session is alive, then unmounts the root, and resolves only when the
//! runtime reports its own disposal complete. The compiled module stays
//! cached between sessions; nothing live does.

pub mod appearance_hooks;
pub mod components;
pub mod context;
pub mod diagnostics;
pub mod effects;
pub mod features;
#[cfg(target_arch = "wasm32")]
pub mod frame;
pub mod host;

/// The frame boot is a wasm-artifact path — off wasm there is no iframe and
/// no port. The stub compiles the frame's call sites (`context`'s dispatch,
/// the document open flow's frame branch, the bin's gate) with the frame's
/// own signatures.
#[cfg(not(target_arch = "wasm32"))]
pub mod frame {
    /// Off wasm an artifact is never frame-hosted.
    pub fn boot_if_hosted() -> bool {
        false
    }

    /// The frame api never exists off-wasm: nothing to run `f` against.
    pub fn with_api<R>(
        _f: impl FnOnce(&frame_transport::PortShellApi<frame_transport::wasm::PortWire>) -> R,
    ) -> Option<R> {
        None
    }

    /// The frame open flow never runs off-wasm; the branch that would call
    /// this is never taken, so the parked continuation never exists.
    pub fn open_path_in_frame(
        _ctx: crate::context::ReaderContext,
        _path: String,
        _placement: crate::host::contract::Placement,
    ) {
    }
}
pub mod pane;
pub mod runtime;
pub mod services;
pub mod state;
pub mod zoom;

use std::cell::{Cell, RefCell};

use app_state::state::UiState;
use app_ui::components::primitives::overlay::lanes::OverlayBoard;
use leptos::prelude::*;
use runtime_contract::boundary::{DocStatusReport, LaunchDocument, ShellApi};
use wasm_bindgen::JsCast;

pub use context::ReaderContext;

/// One live reader session: the unmount handle plus the launch it was
/// started with. The manager holds this; dropping it after `dispose`
/// releases the last Shell-side reference to the session.
pub struct Session {
    pub id: u32,
    unmount: Box<dyn FnOnce()>,
    pub launch: LaunchDocument,
}

/// The live session's handles for the entry points outside its reactive
/// scope: the host in-session commands and the dispose export reach, the
/// session-level blend override a command updates, and the session's root
/// owner, which an in-session command re-enters so whatever it builds (a
/// pane, when the workspace has none) belongs to the session.
///
/// The owner here is the ONE strong reference besides the unmount handle's,
/// and it lives outside the session's arena: the unmount takes it out
/// before it drops the handle, so the handle's drop is always the last one
/// and releases the whole tree — the same single-rooted lifetime the
/// session had before the host existed. Nothing inside the arena holds it.
#[derive(Clone)]
struct LiveSession {
    host: crate::host::ReaderHost,
    blend_override: RwSignal<bool>,
    owner: Owner,
}

thread_local! {
    /// The live session: in-session commands (`drop`-to-open) run against
    /// its host while the session exists, and dispose clears it — the last
    /// runtime reference to the session (§6: the recorded lifetime object,
    /// released with the instance). The host, not a context: there is no
    /// global "current document" here; the host routes to its panes.
    static LIVE_SESSION: RefCell<Option<LiveSession>> = const { RefCell::new(None) };
}

thread_local! {
    /// The one live session. The manager's type/enum makes two primary
    /// runtimes impossible at the Shell; this mirrors it inside the artifact:
    /// a second `start` while one is live is a caller bug and asserts.
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
    static NEXT_ID: Cell<u32> = const { Cell::new(1) };
    /// The resolve half of the dispose promise, taken by the diagnostics
    /// surface the moment the runtime reports its disposal complete.
    static PENDING_DISPOSE: RefCell<Option<js_sys::Function>> = const { RefCell::new(None) };
}

/// Mount a reader session into `host` and open the launch document. Returns
/// the session id the manager uses for [`dispose`] and [`command`].
pub fn start_session(
    host: &web_sys::Element,
    launch: LaunchDocument,
    api: crate::context::ApiHandle,
) -> u32 {
    // The artifact's own diagnostics probe (this window, fresh snapshots) —
    // free-standing from the Shell's global, which merges one beat behind.
    diagnostics::install();
    let id = NEXT_ID.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    });
    SESSION.with(|s| {
        assert!(
            s.borrow().is_none(),
            "a reader session is already live — the manager disposes before it starts"
        );
    });
    diagnostics::note_session_create(id);

    let session_launch = launch.clone();
    let host: web_sys::HtmlElement = host.clone().unchecked_into();
    let handle = mount_to(host, {
        move || {
            // Everything below is scoped to THIS session's owner: the host,
            // its panes, their effects and listeners all die with the
            // unmount. The session's settings copy: seeded from the durable
            // blob the Shell writes through the boundary (§15 — persisted
            // data, not the Shell's live signals), so a reader boots with the
            // saved look and type.
            let settings = RwSignal::new(storage::load_settings());
            if launch.blend_override {
                use leptos::prelude::Update;
                settings.update(|s| s.layout.blend_mode = true);
            }
            let runtime = crate::runtime::ReaderRuntime::new();
            let ui = UiState {
                sidebar: RwSignal::new(app_state::SidebarMode::None),
                toast: RwSignal::new(None),
                window_maximized: RwSignal::new(false),
            };
            // The per-open blend override (the test hook's `?blend=1`): the
            // launch's, and each later launch the Shell hands this session.
            let blend_override = RwSignal::new(launch.blend_override);

            // What the reader's panes and effects consume (§17: the runtime
            // provides the contexts its session reads; the Shell keeps the
            // <html> paints). The look narrows once and the page hosts
            // subscribe to the texture slice of it, so a tint nudge cannot
            // re-run their `texture-*` class.
            let appearance: app_state::AppearanceSignal =
                Memo::new(move |_| settings.with(|s| s.appearance));
            let texture: crate::state::TextureSignal = Memo::new(move |_| appearance.get().texture);
            let typography: crate::state::TypographySignal =
                Memo::new(move |_| settings.with(|s| s.text.clone()));
            provide_context(texture);
            provide_context(typography);

            // One overlay registry for this session: the reader's menus and
            // modals arbitrate through it, and it dies with the unmount.
            provide_context(OverlayBoard::default());

            // THE SESSION IS THE LIFECYCLE BOUNDARY: begin the runtime's
            // mount in this scope.
            runtime.begin_mount();

            // The composition root: the production path is
            // ReaderRuntime → ReaderHost → PaneManager → document pane. The
            // host is handed the pane implementation here — its factory, and
            // its reading of a document address for the descriptor — and
            // never names it.
            let host = crate::host::ReaderHost::new(
                crate::host::HostSession {
                    runtime,
                    settings,
                    ui,
                    api,
                    session_id: id,
                },
                crate::pane::document::factory(),
                crate::pane::document::classify,
            );
            // The session's end, as the unmount runs it: the host disposes
            // its panes (a no-op when the dispose export already did, while
            // the session was still alive), then the runtime awaits the
            // panes' teardown tails and reports its own completion.
            on_cleanup(move || {
                host.dispose();
                runtime.dispose(api, host.take_teardown());
            });

            // The shared appearance chrome's raster hooks are THIS session's
            // engine to answer: while the reader is live the menu's
            // re-bake/scrub/retain-raws calls reach the PDF engine, and the
            // guard's cleanup at unmount takes the answerer away with the
            // session (the library runtime installs none of these).
            let appearance_hooks_guard = appearance_hooks::install();
            on_cleanup(move || drop(appearance_hooks_guard));

            // The AI chunk listener: a session-wide Tauri event bridge (the
            // chunks it re-dispatches are addressed by request, not by pane),
            // unregistered with this scope.
            crate::services::ai::install_ai_chunk_bridge();

            // This frame's own `<html>`: the Shell paints only its document,
            // so the reader paints its look, typography and motion here, and
            // hands its edits to the Shell for persistence. A launch's blend
            // override is per-open, never the user's saved choice: it is
            // stripped on the way out and re-applied to an adopted blob.
            app_ui::frame_theme::install_frame_theme(
                settings,
                app_ui::frame_theme::FramePipeline::Reader,
                move |s| {
                    if blend_override.try_get_untracked() == Some(true) {
                        let mut saved = s.clone();
                        saved.layout.blend_mode = storage::load_settings().layout.blend_mode;
                        api.save_settings(&saved);
                    } else {
                        api.save_settings(s);
                    }
                },
                move |s| {
                    if blend_override.try_get_untracked() == Some(true) {
                        s.layout.blend_mode = true;
                    }
                },
            );

            let live = LiveSession {
                host,
                blend_override,
                owner: Owner::current().expect("the session builds inside its mount's owner"),
            };
            LIVE_SESSION.with(|c| *c.borrow_mut() = Some(live));

            // The launch the Shell handed over (§13): the minimal descriptor,
            // opened by the pane that owns the document — the workspace's
            // root. A warm session (empty launch) still gets its one pane,
            // waiting for the launch its promotion hands over.
            let first = (!launch.path.is_empty()).then_some(launch);
            if let Err(err) = host.create_root(first) {
                web_sys::console::error_1(&format!("[reader] no pane: {err:?}").into());
            }

            // The digest cadence runs in the web build only: the browser
            // suite's probe samples it through scrolls and jumps (a blink of
            // look-ahead activity has to be catchable), and the packaged app
            // pays nothing for a dev instrument (§21).
            if !tauri_bridge::has_tauri() {
                start_digest_beat(api);
                install_open_in_hook();
            }

            // The host and its first pane exist: the session is live.
            runtime.mark_ready();

            view! { <crate::host::ReaderHostView host=host /> }
        }
    });
    let unmount: Box<dyn FnOnce()> = Box::new(move || {
        // Out of the thread-local FIRST (and dropped outside its borrow):
        // the handle must hold the session's last strong owner reference,
        // so dropping it runs the owner's teardown right here.
        let live = LIVE_SESSION.with(|c| c.borrow_mut().take());
        drop(live);
        drop(handle);
    });
    SESSION.with(|s| {
        *s.borrow_mut() = Some(Session {
            id,
            unmount,
            launch: session_launch,
        });
    });
    id
}

/// Dispose the session: the host disposes its panes while the session is
/// still alive (each pane writes its read point, closes its document
/// session, releases its owner — explicitly, observably), then the unmount
/// runs the runtime's disposal, which resolves when the runtime reports its
/// own completion. The manager awaits this before it starts the next
/// runtime (§5).
pub fn dispose(id: u32) -> js_sys::Promise {
    let (promise, resolve) = take_dispose_resolver();
    let live = SESSION.with(|s| s.borrow().as_ref().map(|x| x.id) == Some(id));
    if !live {
        // Already gone (double dispose): resolve immediately, never revive.
        return js_sys::Promise::resolve(&wasm_bindgen::JsValue::from_bool(true));
    }
    diagnostics::note_dispose_request(id);
    if let Some(resolve) = resolve {
        PENDING_DISPOSE.with(|p| *p.borrow_mut() = Some(resolve));
    }
    // The workspace's disposal runs while the session is still ALIVE: each
    // pane's durable write and document close touch the pane's own state,
    // and this is the last moment that state exists (§15: the durable write
    // precedes the disposal, it does not chase it).
    let host = LIVE_SESSION.with(|c| c.borrow().as_ref().map(|live| live.host));
    if let Some(host) = host {
        host.dispose();
    }
    SESSION.with(|s| {
        if let Some(session) = s.borrow_mut().take() {
            (session.unmount)();
        }
    });
    promise
}

/// An in-session command from the Shell: open a document inside the live
/// session — a drop or a dialog, and the launch a warm reader is handed when
/// it is promoted. The HOST routes it: into its active pane, in place
/// ([`host::OpenTarget::Active`]).
pub fn command(id: u32, cmd: runtime_contract::boundary::LaunchDocument) {
    let live = SESSION.with(|s| s.borrow().as_ref().filter(|x| x.id == id).map(|_| ()));
    if live.is_none() {
        return;
    }
    let Some(live) = LIVE_SESSION.with(|c| c.borrow().clone()) else {
        return;
    };
    let _ = live.blend_override.try_set(cmd.blend_override);
    // The descriptor, not the path: the Shell already resolved the row, the
    // resume page and the blend override, and re-resolving over the port
    // would cost a round trip on the one path that is supposed to feel
    // instant. Inside the session's owner: a pane this creates is the
    // session's child, never an orphan.
    let opened = live
        .owner
        .with(|| live.host.open_document(cmd, host::OpenTarget::Active));
    if let Err(err) = opened {
        web_sys::console::warn_1(&format!("[reader] open refused: {err:?}").into());
    }
}

fn take_dispose_resolver() -> (js_sys::Promise, Option<js_sys::Function>) {
    let mut resolve_fn: Option<js_sys::Function> = None;
    let mut executor = |resolve: js_sys::Function, _reject: js_sys::Function| {
        resolve_fn = Some(resolve);
    };
    let promise = js_sys::Promise::new(&mut executor);
    (promise, resolve_fn)
}

/// Called by the runtime's disposal tail when it completed — with or
/// without a document to close: resolves the Shell's dispose promise. A
/// take, so a tail that reaches it twice resolves once.
pub fn resolve_dispose() {
    PENDING_DISPOSE.with(|p| {
        if let Some(resolve) = p.borrow_mut().take() {
            let _ = resolve.call0(&js_sys::global());
        }
    });
}

/// How often a live session pushes its digest (ms). The counters the Shell
/// serves move as renders land and the Shell can only serve the last push, so
/// the beat is faster than the thing reading it: a probe must never be handed
/// a value older than the moment it asks — the look-ahead gauge in particular
/// is live for a blink.
#[cfg(target_arch = "wasm32")]
const DIGEST_BEAT_MS: i32 = 25;

/// The digest cadence (§21): while a session lives, its snapshot goes across
/// the boundary on a beat, so a probe taken at any moment is current. The
/// interval dies with the session scope; the disposal tail pushes the final,
/// drained one.
#[cfg(target_arch = "wasm32")]
fn start_digest_beat(api: crate::context::ApiHandle) {
    use wasm_bindgen::prelude::Closure;
    let Some(win) = web_sys::window() else {
        return;
    };
    let tick = Closure::<dyn FnMut()>::new(move || diagnostics::publish_digest(&api));
    let callback = tick.as_ref().unchecked_ref::<js_sys::Function>();
    let Ok(id) =
        win.set_interval_with_callback_and_timeout_and_arguments_0(callback, DIGEST_BEAT_MS)
    else {
        return;
    };
    // The closure is OWNED, not forgotten. The interval needs the JS reference
    // to keep firing; `forget()` would leak that callback — and the `ApiHandle`
    // it captures — for the module's life, which is exactly what a runtime
    // whose point is explicit resource lifetime must not do. The closure parks
    // in owner-scoped storage (a cleanup hook must be `Send + Sync` and a
    // `Closure` is neither), and the cleanup releases it: the interval is
    // cleared first, then the closure is dropped, so the callback and the
    // handle it captured are freed the moment their timer is.
    let owned = StoredValue::new_local(Some(tick));
    on_cleanup(move || {
        if let Some(win) = web_sys::window() {
            win.clear_interval_with_handle(id);
        }
        let _ = owned.try_set_value(None);
    });
}

/// A host build has no bridge and no timer to push through.
#[cfg(not(target_arch = "wasm32"))]
fn start_digest_beat(_api: crate::context::ApiHandle) {}

/// The browser suite's workspace hook (web build only, like `?open=`):
/// `window.__mareaderOpenIn(path, target)` opens a bundled sample through
/// the host's one open command — `"active"` in place, `"right"` / `"down"`
/// in a new pane beside the active one. Only `/samples/` paths, and `true`
/// when the host placed it. The hook dies with the session scope: the
/// window property is deleted and the closure dropped, so a disposed
/// session leaves nothing reachable from the page.
#[cfg(target_arch = "wasm32")]
fn install_open_in_hook() {
    use wasm_bindgen::prelude::Closure;
    const HOOK: &str = "__mareaderOpenIn";
    let Some(win) = web_sys::window() else {
        return;
    };
    let hook = Closure::<dyn Fn(String, String) -> bool>::new(|path: String, target: String| {
        if !path.starts_with("/samples/") {
            return false;
        }
        let Some(live) = LIVE_SESSION.with(|c| c.borrow().clone()) else {
            return false;
        };
        let active = untrack(|| live.host.manager().active());
        let target = match (target.as_str(), active) {
            ("active", _) => host::OpenTarget::Active,
            ("right", Some(of)) => host::OpenTarget::Beside {
                of,
                axis: host::tree::SplitAxis::Horizontal,
            },
            ("down", Some(of)) => host::OpenTarget::Beside {
                of,
                axis: host::tree::SplitAxis::Vertical,
            },
            _ => return false,
        };
        let launch = LaunchDocument {
            book_id: None,
            path,
            resume_page: 1,
            saved_fraction: None,
            blend_override: false,
            cover_data_url: None,
            display_name: None,
        };
        live.owner
            .with(|| live.host.open_document(launch, target))
            .is_ok()
    });
    let key = wasm_bindgen::JsValue::from_str(HOOK);
    if js_sys::Reflect::set(&win, &key, hook.as_ref()).is_err() {
        return;
    }
    let owned = StoredValue::new_local(Some(hook));
    on_cleanup(move || {
        if let Some(win) = web_sys::window() {
            let _ = js_sys::Reflect::delete_property(&win, &wasm_bindgen::JsValue::from_str(HOOK));
        }
        let _ = owned.try_set_value(None);
    });
}

/// A host build has no page to hang a hook on.
#[cfg(not(target_arch = "wasm32"))]
fn install_open_in_hook() {}

/// The doc-status bridge: the session's document state is what the Shell's
/// URL policy and probe read. Called by the status effect in the entry view.
pub fn report_status(api: &dyn ShellApi, status: &str, error: Option<String>) {
    api.doc_status(&DocStatusReport {
        status: status.to_string(),
        error,
    });
}

/// Standalone boot (`reader.html`): no Shell — a storage-backed API and the
/// URL's own launch parameters. This is the artifact's proof that it loads
/// and runs without the unified app anywhere in the page.
pub fn run_standalone() {
    console_error_panic_hook::set_once();
    let launch = web_launch();
    let host = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.body())
        .map(web_sys::Element::from)
        .expect("document body for the standalone reader");
    start_session(&host, launch, context::ApiHandle::Standalone);
}

/// The URL launch (`?open=/samples/…&blend=1`), the same hook the browser
/// suite drives — parsed by the reader itself when no Shell owns the page.
pub fn web_launch() -> LaunchDocument {
    let mut launch = LaunchDocument {
        book_id: None,
        path: String::new(),
        resume_page: 1,
        saved_fraction: None,
        blend_override: false,
        cover_data_url: None,
        display_name: None,
    };
    if let Some(window) = web_sys::window()
        && let Ok(search) = window.location().search()
        && let Ok(params) = web_sys::UrlSearchParams::new_with_str(&search)
    {
        if params.get("blend").is_some() {
            launch.blend_override = true;
        }
        if let Some(path) = params.get("open")
            && path.starts_with("/samples/")
        {
            launch.path = path;
        }
    }
    launch
}
