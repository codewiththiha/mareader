//! The library runtime: the shelf — its state, services, effects and UI —
//! compiled as its own WASM artifact. It has no reader state to hold: the
//! reader is a different artifact this one can only ask the Shell for.

pub mod context;
pub mod effects_library;
pub mod features;
#[cfg(target_arch = "wasm32")]
pub mod frame;

/// The frame boot is a wasm-artifact path: on the host lanes there is no
/// iframe, no port, nothing to boot — the same rules as the memory probe's
/// host shape. The stub keeps the frame's call sites compiling (`context`'s
/// `ApiHandle::Frame` dispatch, the bin's boot gate) with the frame's own
/// signatures: an api that never exists and a boot that is never hosted.
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
}
pub mod services;
pub mod state;

use std::cell::{Cell, RefCell};

use app_ui::components::primitives::overlay::lanes::OverlayBoard;
use leptos::prelude::*;
use reader_core::settings::Settings;
use wasm_bindgen::JsCast;

pub use context::LibraryContext;

/// One live library session: the unmount handle. The manager starts and
/// disposes it like the reader's; the library keeps no cross-session state —
/// the durable copy in storage is what the next session seeds from
/// (persist data ≠ retain live object).
pub struct Session {
    pub id: u32,
    unmount: Box<dyn FnOnce()>,
}

thread_local! {
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
    static NEXT_ID: Cell<u32> = const { Cell::new(1) };
    static PENDING_DISPOSE: RefCell<Option<js_sys::Function>> = const { RefCell::new(None) };
    /// The live session's context, for the Shell → runtime commands that
    /// land outside any page event (a bake answer; an open handoff).
    static LIVE_CTX: RefCell<Option<LibraryContext>> = const { RefCell::new(None) };
}

/// Mount a library session into `host`.
pub fn start_session(host: &web_sys::Element, api: context::ApiHandle) -> u32 {
    let id = NEXT_ID.with(|n| {
        let id = n.get();
        n.set(id + 1);
        id
    });
    SESSION.with(|s| {
        assert!(
            s.borrow().is_none(),
            "a library session is already live — the manager disposes before it starts"
        );
    });

    let host: web_sys::HtmlElement = host.clone().unchecked_into();
    let state = context::LibraryContext::new(api);
    LIVE_CTX.with(|c| *c.borrow_mut() = Some(state));
    let handle = mount_to(host, move || {
        // Scoped to THIS session: the state seeds from storage, the effects
        // (grid gestures, dnd, import flows) install, the UI mounts.
        provide_context(state.library.covers);
        // One overlay registry for this session: the shelf's menus and modals
        // arbitrate through it, and it dies with the unmount.
        provide_context(OverlayBoard::default());
        // The library's session effects install INSIDE this scope: the
        // listeners and timers die with the unmount (§5, §17).
        effects_library::library_effects(state);
        view! { <features::library::LibraryPage state /> }
    });
    let unmount: Box<dyn FnOnce()> = Box::new(move || drop(handle));
    SESSION.with(|s| {
        *s.borrow_mut() = Some(Session { id, unmount });
    });
    id
}

/// The command envelope the Shell delivers to a LIVE library session.
#[derive(Debug, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum LibraryCommand {
    /// A cover-bake request answered: the engine's raster, or `None` when
    /// the bake failed (the queue's one-retry policy decides from there).
    #[serde(rename_all = "camelCase")]
    CoverBaked {
        path: String,
        /// Arc-free on the wire; the filing queue re-wraps it.
        image: Option<Box<runtime_contract::covers::CoverImage>>,
    },
}

/// Run one command against the live session. Commands for a session id that
/// is no longer live are dropped, not answered — the Shell's generation
/// guard and this check are the two walls a stale frame's traffic hits.
pub fn command(id: u32, cmd: LibraryCommand) {
    let live = SESSION.with(|s| s.borrow().as_ref().is_some_and(|x| x.id == id));
    if !live {
        return;
    }
    if let Some(ctx) = LIVE_CTX.with(|c| *c.borrow()) {
        match cmd {
            LibraryCommand::CoverBaked { path, image } => {
                services::covers::on_baked(ctx, path, image.map(|image| *image));
            }
        }
    }
}

/// Dispose the library session (the manager replaces runtimes; the reader is
/// no different except in direction).
pub fn dispose(id: u32) -> js_sys::Promise {
    let mut resolve_fn: Option<js_sys::Function> = None;
    let mut executor = |resolve: js_sys::Function, _reject: js_sys::Function| {
        resolve_fn = Some(resolve);
    };
    let promise = js_sys::Promise::new(&mut executor);
    let live = SESSION.with(|s| s.borrow().as_ref().map(|x| x.id) == Some(id));
    if !live {
        return js_sys::Promise::resolve(&wasm_bindgen::JsValue::from_bool(true));
    }
    SESSION.with(|s| {
        if let Some(session) = s.borrow_mut().take() {
            LIVE_CTX.with(|c| *c.borrow_mut() = None);
            // The library session owns no async engine tails: the unmount's
            // cleanups are synchronous (listeners, observers, timers), so the
            // promise resolves on the next microtask via a plain resolve.
            (session.unmount)();
        }
    });
    if let Some(resolve) = resolve_fn {
        let _ = resolve.call0(&js_sys::global());
    }
    promise
}

/// Standalone boot (`library.html`): no Shell — a storage-backed API.
pub fn run_standalone() {
    console_error_panic_hook::set_once();
    let api = context::ApiHandle::Standalone;
    let host = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.body())
        .map(web_sys::Element::from)
        .expect("document body for the standalone library");
    start_session(&host, api);
}

/// The settings blob a standalone library session starts from (the hosted
/// path's seed comes through the manager's bridge launch payload).
pub fn standalone_settings() -> Settings {
    storage::load_settings()
}
