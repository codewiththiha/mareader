//! Reader runtime, its own WASM artifact: ReaderRuntime → ReaderHost
//! → PaneManager → document pane.

#[cfg(feature = "pdf")]
pub mod appearance_hooks;
pub mod components;
pub mod context;
pub mod diagnostics;
pub mod effects;
pub mod features;
#[cfg(target_arch = "wasm32")]
pub mod frame;
pub mod host;

/// The off-wasm stub, so the frame's call sites still compile.
#[cfg(not(target_arch = "wasm32"))]
pub mod frame {
    /// The frame api never exists off-wasm: nothing to run `f` against.
    pub fn with_api<R>(
        _f: impl FnOnce(&frame_transport::PortShellApi<frame_transport::wasm::PortWire>) -> R,
    ) -> Option<R> {
        None
    }

    /// Off wasm the frame open flow never runs.
    pub fn open_path_in_frame(
        _ctx: crate::context::ReaderContext,
        _path: String,
        _placement: crate::host::contract::Placement,
    ) {
    }

    /// Native lanes have no hosted frame marker.
    pub fn boot_if_hosted() -> bool {
        false
    }
}
pub mod frame_pane;
pub mod pane;
pub mod pane_frame;
pub mod pane_wire;
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

/// One live Reader session and its explicit unmount owner.
struct Session {
    id: u32,
    unmount: Box<dyn FnOnce()>,
}

/// Handles for entry points outside the reactive scope; the unmount
/// handle holds the last reference.
#[derive(Clone)]
struct LiveSession {
    host: crate::host::ReaderHost,
    blend_override: RwSignal<bool>,
    owner: Owner,
}

thread_local! {
    /// The live session for in-session commands; dispose clears it.
    static LIVE_SESSION: RefCell<Option<LiveSession>> = const { RefCell::new(None) };
}

thread_local! {
    /// The one live session: a second `start` is a caller bug.
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
    static NEXT_ID: Cell<u32> = const { Cell::new(1) };
    /// The dispose promise's resolve half, taken on completion.
    static PENDING_DISPOSE: RefCell<Option<js_sys::Function>> = const { RefCell::new(None) };
}

/// Mount a session into `host` and open the launch document; the id
/// goes to [`dispose`].
pub fn start_session(
    host: &web_sys::Element,
    launch: LaunchDocument,
    api: crate::context::ApiHandle,
) -> u32 {
    // The artifact's own diagnostics probe, free of the Shell's global.
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

    let host: web_sys::HtmlElement = host.clone().unchecked_into();
    let handle = mount_to(host, {
        move || {
            // Scoped to THIS session's owner; settings seed from the durable
            // blob.
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
            // The per-open blend override: this launch's, and each later one.
            let blend_override = RwSignal::new(launch.blend_override);

            // Typography and the settings signal: overlays read their
            // knobs from context, not props.
            let typography: crate::state::TypographySignal =
                Memo::new(move |_| settings.with(|s| s.text.clone()));
            provide_context(typography);
            provide_context(settings);

            // One overlay board for this session, dying with the unmount.
            provide_context(OverlayBoard::default());

            // THE SESSION IS THE LIFECYCLE BOUNDARY: begin the runtime's
            // mount in this scope.
            runtime.begin_mount();

            // The composition root: the host takes the pane factory here and
            // never names it.
            let host = crate::host::ReaderHost::new(
                crate::host::HostSession {
                    runtime,
                    settings,
                    ui,
                    api,
                    session_id: id,
                    enter: enter_session,
                },
                crate::frame_pane::factory(),
                crate::pane::document::classify,
            );
            // The unmount's end: the host disposes its panes, then the runtime
            // awaits the teardown tails.
            on_cleanup(move || {
                host.dispose();
                runtime.dispose(api, host.take_teardown());
            });

            // Appearance hooks come from the PDF panes' frames; the guard
            // dies with the session.
            let appearance_hooks_guard = crate::frame_pane::install_hooks();
            on_cleanup(move || drop(appearance_hooks_guard));

            // The AI chunk bridge: session-wide, unregistered with this scope.
            crate::services::ai::install_ai_chunk_bridge();
            crate::services::cefr::install_cefr_bridge();
            crate::services::dict::install_dict_bridge();

            // This frame's own `<html>`: the reader paints its look and hands
            // edits to the Shell.
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

            // The launch the Shell handed over (§13), opened by the root
            // pane.
            let first = (!launch.path.is_empty()).then_some(launch);
            if let Err(err) = host.create_root(first) {
                web_sys::console::error_1(&format!("[reader] no pane: {err:?}").into());
            }

            // Web build only: the packaged app pays nothing for a dev
            // instrument (§21).
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
        // Out of the thread-local FIRST, dropped outside its borrow: the
        // handle runs the teardown.
        let live = LIVE_SESSION.with(|c| c.borrow_mut().take());
        drop(live);
        drop(handle);
    });
    SESSION.with(|s| {
        *s.borrow_mut() = Some(Session { id, unmount });
    });
    id
}

/// Dispose the session; the promise resolves when the runtime
/// reports completion.
pub fn dispose(id: u32) -> js_sys::Promise {
    let (promise, resolve) = take_dispose_resolver();
    let live = SESSION.with(|s| s.borrow().as_ref().map(|x| x.id) == Some(id));
    if !live {
        // Already gone (double dispose): resolve immediately, never revive.
        return js_sys::Promise::resolve(&wasm_bindgen::JsValue::from_bool(true));
    }
    if let Some(resolve) = resolve {
        PENDING_DISPOSE.with(|p| *p.borrow_mut() = Some(resolve));
    }
    // Disposal runs while the session is ALIVE: each pane's durable
    // write precedes it (§15).
    let host = LIVE_SESSION.with(|c| c.borrow().as_ref().map(|live| live.host));
    let Some(session) = SESSION.with(|s| s.borrow_mut().take()) else {
        return promise;
    };
    let Some(host) = host else {
        (session.unmount)();
        return promise;
    };
    host.dispose();
    // Frames flush and close before the unmount removes them, which
    // waits for those tails.
    let tails = host.take_teardown();
    leptos::task::spawn_local(async move {
        tails.await;
        (session.unmount)();
    });
    promise
}

/// Run `work` in the live session's root owner; gone, it runs
/// nothing.
fn enter_session(work: &mut dyn FnMut()) {
    let owner = LIVE_SESSION.with(|c| c.borrow().as_ref().map(|live| live.owner.clone()));
    if let Some(owner) = owner {
        owner.with(work);
    }
}

/// An in-session command from the Shell, routed to the active pane.
pub fn command(id: u32, cmd: runtime_contract::boundary::LaunchDocument) {
    let live = SESSION.with(|s| s.borrow().as_ref().filter(|x| x.id == id).map(|_| ()));
    if live.is_none() {
        return;
    }
    let Some(live) = LIVE_SESSION.with(|c| c.borrow().clone()) else {
        return;
    };
    let _ = live.blend_override.try_set(cmd.blend_override);
    // The descriptor, not the path: the Shell resolved it. Inside
    // the session's owner.
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

/// Park `done` as the runtime's disposal-complete callback.
#[cfg(target_arch = "wasm32")]
pub(crate) fn on_dispose_complete(done: js_sys::Function) {
    PENDING_DISPOSE.with(|p| *p.borrow_mut() = Some(done));
}

/// Resolve the Shell's dispose promise; a take, so it resolves once.
pub fn resolve_dispose() {
    PENDING_DISPOSE.with(|p| {
        if let Some(resolve) = p.borrow_mut().take() {
            let _ = resolve.call0(&js_sys::global());
        }
    });
}

/// The digest push cadence (ms), faster than the probe that reads
/// it.
#[cfg(target_arch = "wasm32")]
const DIGEST_BEAT_MS: i32 = 25;

/// The digest cadence (§21): the interval dies with the session.
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
    // The closure is OWNED, not forgotten: cleanup clears the interval,
    // then drops it.
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

/// The browser suite's `window.__mareaderOpenIn` hook: sample paths
/// only, gone with the session.
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
            ("right", Some(of)) => host::OpenTarget::Split {
                of,
                axis: host::tree::SplitAxis::Horizontal,
                side: host::tree::Side::After,
            },
            ("down", Some(of)) => host::OpenTarget::Split {
                of,
                axis: host::tree::SplitAxis::Vertical,
                side: host::tree::Side::After,
            },
            _ => return false,
        };
        let launch = LaunchDocument {
            path,
            ..crate::pane::base::empty_launch()
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

/// The doc-status bridge the Shell's URL policy and probe read.
pub fn report_status(api: &dyn ShellApi, status: &str, error: Option<String>) {
    api.doc_status(&DocStatusReport {
        status: status.to_string(),
        error,
    });
}

/// The unhosted development entry mounts the same independent-pane host.
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

/// The URL launch (`?open=…&blend=1`) the browser suite drives.
fn web_launch() -> LaunchDocument {
    let mut launch = crate::pane::base::empty_launch();
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
