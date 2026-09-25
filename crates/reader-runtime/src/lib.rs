//! The reader runtime: one reader session's state, resources, effects and
//! UI, compiled as its own WASM artifact and mounted by the Shell's runtime
//! manager.
//!
//! A session is an explicit instance: [`start_session`] creates the reactive
//! ownership root, the [`ReaderContext`] (reader state + the Phase 1
//! `ReaderRuntime` lifecycle owner + the session's settings/UI slices + the
//! [`ShellApi`](app_state::boundary::ShellApi) boundary), installs the
//! reader effects INSIDE that scope, and mounts the reader host. [`dispose`]
//! unmounts the root — whose first-registered cleanup is the Phase 1
//! disposal chain — and resolves only when the runtime reports its own
//! disposal complete. The compiled module stays cached between sessions;
//! nothing live does.

pub mod components;
pub mod context;
pub mod diagnostics;
pub mod effects;
pub mod features;
pub mod runtime;
pub mod services;
pub mod zoom;

use std::cell::{Cell, RefCell};

use app_state::boundary::{DocStatusReport, LaunchDocument, ShellApi};
use app_state::state::{ReaderState, UiState};
use app_ui::components::primitives::overlay::lanes::OverlayBoard;
use leptos::prelude::*;
use wasm_bindgen::JsCast;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::wasm_bindgen;

pub use context::ReaderContext;

/// One live reader session: the unmount handle plus the launch it was
/// started with. The manager holds this; dropping it after `dispose`
/// releases the last Shell-side reference to the session.
pub struct Session {
    pub id: u32,
    unmount: Box<dyn FnOnce()>,
    pub launch: LaunchDocument,
}

thread_local! {
    /// The live session's context: in-session commands (`drop`-to-open) run
    /// against it while the session exists, and dispose clears it — the last
    /// runtime reference to the session's state (§6: the recorded lifetime
    /// object, released with the instance).
    static LIVE_CTX: RefCell<Option<crate::context::ReaderContext>> =
        const { RefCell::new(None) };
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
            // Everything below is scoped to THIS session's owner: the state,
            // the runtime slot, the effects and listeners all die with the
            // unmount, and the first-registered cleanup is the disposal.
            // The session's settings copy: seeded from the durable blob the
            // Shell writes through the boundary (§15 — persisted data, not the
            // Shell's live signals), so a reader boots with the saved look and
            // type.
            let settings = RwSignal::new(storage::load_settings());
            if launch.blend_override {
                use leptos::prelude::Update;
                settings.update(|s| s.layout.blend_mode = true);
            }
            let reader = ReaderState::default();
            let runtime = crate::runtime::ReaderRuntime::new();
            let launch = RwSignal::new(launch);
            let ui = UiState {
                sidebar: RwSignal::new(app_state::SidebarMode::None),
                toast: RwSignal::new(None),
                window_maximized: RwSignal::new(false),
            };
            let reflowable = Signal::derive(move || reader.document.format.get().is_reflowable());
            let search_visible = reader.search.visible;
            let sidebar_slide = reader.viewer.motion;
            let ctx = ReaderContext {
                reader,
                runtime,
                settings,
                ui,
                api,
                launch,
                id,
                chrome: app_state::ChromeState {
                    settings,
                    ui,
                    reader: app_state::ReaderSurface {
                        reflowable,
                        search_visible,
                        sidebar_slide,
                    },
                },
            };

            // What the reader's own pages and effects consume (§17: the
            // runtime provides the contexts its session reads; the Shell keeps
            // the <html> paints). The look narrows once and the page hosts
            // subscribe to the texture slice of it, so a tint nudge cannot
            // re-run their `texture-*` class.
            let appearance: app_state::AppearanceSignal =
                Memo::new(move |_| settings.with(|s| s.appearance));
            let texture: app_state::state::TextureSignal =
                Memo::new(move |_| appearance.get().texture);
            let typography: app_state::state::reader::TypographySignal =
                Memo::new(move |_| settings.with(|s| s.text.clone()));
            provide_context(texture);
            provide_context(typography);

            // One overlay registry for this session: the reader's menus and
            // modals arbitrate through it, and it dies with the unmount.
            provide_context(OverlayBoard::default());

            // THE SESSION IS THE LIFECYCLE BOUNDARY (the route was, in the
            // unified app): begin the mount in this scope, register the
            // disposal cleanup FIRST so it runs LAST.
            runtime.begin_mount();
            provide_context(runtime);
            on_cleanup(move || {
                ctx.runtime.dispose(ctx);
            });

            LIVE_CTX.with(|c| *c.borrow_mut() = Some(ctx));

            // The launch the Shell handed over (§13): the minimal descriptor,
            // opened by the runtime that owns the document. Nothing outside
            // this scope holds the path — an in-session open re-resolves what
            // it is handed, but a launch is already resolved. Opened BEFORE
            // the status report below, so the session's first word to the
            // Shell is `Opening`: an Idle report would send a live session's
            // Shell straight back to the shelf.
            // The paper session's blend switch and detection area, sent
            // BEFORE the first open: the first book's first frame publishes
            // only if the session already knows `blend_on` (the same
            // before-the-first-open contract the deleted app root held).
            crate::effects::reader::blend_backdrop::paper_settings(ctx);

            let launch = ctx.launch.get_untracked();
            if !launch.path.is_empty() {
                services::document::open::open_with_launch(ctx, launch);
            }

            // The two facts the Shell's probe serves from this runtime (§21):
            // the document status its Idle policy answers from, and the
            // snapshot itself — engine counters, gauges, the baseline verdict.
            // Both are PUSHED across the boundary: the Shell holds no reader
            // state, and this session's scope is what ends the pushing.
            install_status_report(ctx);
            install_page_report(ctx);
            // The digest cadence runs in the web build only: the browser
            // suite's probe samples it through scrolls and jumps (a blink of
            // look-ahead activity has to be catchable), and the packaged app
            // pays nothing for a dev instrument (§21).
            if !tauri_bridge::has_tauri() {
                start_digest_beat(ctx.api);
            }

            view! { <features::page::ReaderPage state=ctx /> }

            // Every reader effect and resource is installed: the runtime is
            // live (features::page::ReaderPage ends with mark_ready once the
            // document work is admitted).
        }
    });
    let unmount: Box<dyn FnOnce()> = Box::new(move || {
        LIVE_CTX.with(|c| *c.borrow_mut() = None);
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

/// Dispose the session: unmount (which runs the Phase 1 disposal chain) and
/// resolve when the runtime reports completion. The manager awaits this
/// before it starts the next runtime (§5).
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
    // What a session ending owes while it is still ALIVE: the read point the
    // progress effect may still be debouncing, and the paper session's
    // document close. Both touch the session's own state, and this is the
    // last moment that state exists — after the unmount below its owner is
    // disposed, and a reactive read then is illegal (§15: the durable write
    // precedes the disposal, it does not chase it).
    if let Some(ctx) = LIVE_CTX.with(|c| *c.borrow()) {
        crate::services::document::flush::flush_read_point(&ctx);
        pdf_engine::backdrop::document_close();
    }
    SESSION.with(|s| {
        if let Some(session) = s.borrow_mut().take() {
            (session.unmount)();
        }
    });
    promise
}

/// An in-session command from the Shell (only drops arrive this way today):
/// open another document inside the live session.
pub fn command(id: u32, cmd: app_state::boundary::LaunchDocument) {
    let live = SESSION.with(|s| s.borrow().as_ref().filter(|x| x.id == id).map(|_| ()));
    if live.is_some() {
        let ctx = LIVE_CTX.with(|c| *c.borrow());
        if let Some(ctx) = ctx {
            services::document::open::open_path(ctx, cmd.path);
        }
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

/// Called by the diagnostics surface when the runtime's disposal tail
/// completed: resolves the Shell's dispose promise.
pub fn resolve_dispose() {
    PENDING_DISPOSE.with(|p| {
        if let Some(resolve) = p.borrow_mut().take() {
            let _ = resolve.call0(&js_sys::global());
        }
    });
}

/// The doc-status report (§21), installed in the session's own scope: the
/// Shell's URL policy and its probe read the session's document status, and
/// the session is the only place that status exists. Pushed on every change.
fn install_status_report(ctx: crate::context::ReaderContext) {
    let api = ctx.api;
    let reader = ctx.reader;
    Effect::new(move |_| {
        // try_: the disposal flush can wake this effect after its owner is
        // gone, and a bare read panics there — the report is worth nothing
        // once the session is over, so a dead read is simply the end.
        let Some(status) = reader.document.status.try_get() else {
            return;
        };
        let error = reader.document.error.try_get().flatten();
        let word = format!("{status:?}");
        report_status(&api, &word, error);
    });
}

/// The viewer's page, published for the digest (§21): the Shell's probe
/// answers "which page is this session on" from the same push the status
/// rides, and the page lives here. Read tracked, so every move republishes.
fn install_page_report(ctx: crate::context::ReaderContext) {
    let page = ctx.reader.viewer.page;
    Effect::new(move |_| {
        // try_: same disposal-flush guard as the status report.
        if let Some(page) = page.try_get() {
            diagnostics::set_reader_page(page);
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

/// The doc-status bridge: the session's document state is what the Shell's
/// URL policy and probe read. Called by the status effect in the entry view.
pub fn report_status(api: &dyn ShellApi, status: &str, error: Option<String>) {
    api.doc_status(&DocStatusReport {
        status: status.to_string(),
        error,
    });
}

/// Whether a Shell hosts this artifact. The Shell installs its bridge before
/// it loads any runtime, and the artifact's own page has no such bridge:
/// this is the gate that keeps a dynamically imported artifact from booting
/// a second, invisible session beside the Shell's (§12 — the runtime mounts
/// only inside the Shell's target, and only when the Shell says so).
pub fn shell_hosted() -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        use wasm_bindgen::JsCast;
        let Some(window) = web_sys::window() else {
            return false;
        };
        let target: js_sys::Object = window.unchecked_into();
        let Ok(bridge) =
            js_sys::Reflect::get(&target, &wasm_bindgen::JsValue::from_str("__mareaderShell"))
        else {
            return false;
        };
        !bridge.is_undefined()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        false
    }
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

// ---------------------------------------------------------------------------
// The wasm exports the Shell's manager calls. The payloads are JSON strings
// of the boundary types; the session ids are this artifact's own.
// ---------------------------------------------------------------------------

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(js_name = mareaderReaderStart)]
pub fn mareader_reader_start(host: wasm_bindgen::JsValue, launch_json: String) -> u32 {
    console_error_panic_hook::set_once();
    let host: web_sys::Element = host.unchecked_into();
    let launch: app_state::boundary::LaunchDocument =
        serde_json::from_str(&launch_json).expect("launch descriptor");
    // Session-scoped bridge wiring happens inside start_session's scope.
    let id = start_session(&host, launch, context::ApiHandle::Js);
    diagnostics::set_reader_live(true);
    id
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(js_name = mareaderReaderDispose)]
pub fn mareader_reader_dispose(id: u32) -> js_sys::Promise {
    let promise = dispose(id);
    // The live-state flag drops with the session; the digest's final push
    // (drained) happens in the disposal tail via publish_digest.
    diagnostics::set_reader_live(false);
    promise
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen(js_name = mareaderReaderCommand)]
pub fn mareader_reader_command(id: u32, cmd_json: String) {
    if let Ok(cmd) = serde_json::from_str::<app_state::boundary::LaunchDocument>(&cmd_json) {
        command(id, cmd);
    }
}
