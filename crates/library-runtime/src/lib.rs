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
///
/// `warm` is the Shell's statement that this session boots ahead of the
/// navigation that will use it: the shelf renders so the reveal is free, but
/// it holds its startup passes until [`refresh`] runs them.
pub fn start_session(host: &web_sys::Element, api: context::ApiHandle, warm: bool) -> u32 {
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

    // A fresh shelf owes nothing to the one before it in this frame.
    services::covers::reset_ledger();
    let host: web_sys::HtmlElement = host.clone().unchecked_into();
    let state = context::LibraryContext::new(api);
    // The baseline a later Refresh compares against: what this seed read.
    SEEN_STAMPS.with(|slot| slot.set(StoreStamps::read()));
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
        effects_library::library_effects(state, warm);
        // This frame's own `<html>`: the chrome pipeline only — the shelf has
        // no raster and no reflowable page, so it writes neither token set
        // and never addresses an engine. Edits go to the Shell to persist.
        {
            use runtime_contract::boundary::ShellApi;
            let api = state.api;
            app_ui::frame_theme::install_frame_theme(
                state.settings,
                app_ui::frame_theme::FramePipeline::Library,
                move |s| api.save_settings(s),
                |_| {},
            );
        }
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
    /// Promoted from warm to visible: re-read the durable state this session
    /// seeded at boot and has not looked at since.
    Refresh,
}

/// Re-read the store into the live session's signals.
///
/// A warm library boots while the reader is still on screen, so the blob it
/// seeded from is the one that existed before that reading session: the row
/// the reader was in has moved, and a book imported two opens ago may not be
/// in it. This is the cheap half of a cold boot — the wasm instance, the
/// Leptos mount and the grid's first layout are all already paid for — and it
/// is what makes a warm handback correct instead of merely fast.
///
/// Only the persisted slices are replaced. Everything the user was doing in
/// the shelf (the query, the open shelf, the selection) is session state, not
/// durable state, and overwriting it here would be a bug.
fn refresh(ctx: LibraryContext) {
    // Only what moved while the shelf waited. A reading session moves read
    // points (the library blob) and rarely anything else; re-parsing the
    // cover map — megabytes of data URLs — and re-setting it would re-render
    // every cover on the shelf at the exact moment it is revealed.
    let now = StoreStamps::read();
    let seen = SEEN_STAMPS.with(|slot| slot.replace(now));
    if now.library.is_none() || now.library != seen.library {
        let blob = storage::load_library();
        ctx.library.books.set(blob.books);
        ctx.library.shelves.set(blob.shelves);
        ctx.library.folders.set(blob.folders);
        ctx.library.view.set(blob.view);
    }
    if now.covers.is_none() || now.covers != seen.covers {
        ctx.library.covers.set(storage::load_covers());
    }
    if now.settings.is_none() || now.settings != seen.settings {
        ctx.settings.set(storage::load_settings());
    }
    // And the work a warm boot parked: the shelf is on screen now, so its
    // migration, its measurement pass and its cover bakes are owed — just
    // not inside the reveal's own frame. They start once it has painted.
    after_reveal(effects_library::run_deferred_startup);
}

/// Stamps of the stores a shelf seeds from, as of its last read.
#[derive(Clone, Copy, Default)]
struct StoreStamps {
    library: Option<u64>,
    covers: Option<u64>,
    settings: Option<u64>,
}

impl StoreStamps {
    fn read() -> Self {
        Self {
            library: storage::library_stamp(),
            covers: storage::covers_stamp(),
            settings: storage::settings_stamp(),
        }
    }
}

thread_local! {
    /// What the live session last read from the store (seeded at session
    /// start, refreshed on every Refresh).
    static SEEN_STAMPS: Cell<StoreStamps> = Cell::new(StoreStamps::default());
}

/// Run `f` shortly after the current frame has been presented.
fn after_reveal(f: fn()) {
    #[cfg(target_arch = "wasm32")]
    {
        let Some(window) = web_sys::window() else {
            f();
            return;
        };
        let run = wasm_bindgen::closure::Closure::once_into_js(f);
        if window
            .set_timeout_with_callback_and_timeout_and_arguments_0(
                run.unchecked_ref(),
                REVEAL_SETTLE_MS,
            )
            .is_err()
        {
            f();
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    f();
}

/// How long a revealed shelf gets to paint before its parked passes start.
#[cfg(target_arch = "wasm32")]
const REVEAL_SETTLE_MS: i32 = 160;

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
            LibraryCommand::Refresh => refresh(ctx),
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
    start_session(&host, api, false);
}

/// The settings blob a standalone library session starts from (the hosted
/// path's seed comes through the manager's bridge launch payload).
pub fn standalone_settings() -> Settings {
    storage::load_settings()
}
