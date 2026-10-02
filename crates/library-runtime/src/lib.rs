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
    /// The frame api never exists off-wasm: nothing to run `f` against.
    pub fn with_api<R>(
        _f: impl FnOnce(&frame_transport::PortShellApi<frame_transport::wasm::PortWire>) -> R,
    ) -> Option<R> {
        None
    }

    /// Native lanes have no hosted frame marker.
    pub fn boot_if_hosted() -> bool {
        false
    }
}
pub mod services;
pub mod state;

use std::cell::{Cell, RefCell};

use app_ui::components::primitives::overlay::lanes::OverlayBoard;
use leptos::prelude::*;
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
    /// The live session's context, for the Shell → runtime commands that
    /// land outside any page event (a bake answer; an open handoff).
    static LIVE_CTX: RefCell<Option<LibraryContext>> = const { RefCell::new(None) };
}

/// Mount a library session into `host`.
///
/// An incoming hosted frame defers startup writes until visible paint;
/// standalone starts them immediately. Every session owns a fresh realm.
pub fn start_session(host: &web_sys::Element, api: context::ApiHandle, defer_startup: bool) -> u32 {
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
        effects_library::library_effects(state, defer_startup);
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
    /// Newly revealed: reconcile late durable writes and start deferred work.
    Refresh,
    /// Files dropped on the window from the OS: imported onto the shelf on
    /// screen, exactly as the Add menu's picker would.
    ImportFiles { paths: Vec<String> },
}

/// Re-read the store into the live session's signals.
///
/// The outgoing Reader may flush a read point after this fresh Library
/// seeded its store. Reconcile changed persisted slices without replacing
/// this session's query/selection or unnecessarily rebuilding its cover map.
fn refresh(ctx: LibraryContext) {
    // Only what changed between incoming mount and reveal. Re-parsing the
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
    // Start incoming-frame passes after visible paint, not during the
    // handoff's own frame. The callback is cancelled with this session.
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
    #[cfg(target_arch = "wasm32")]
    static REVEAL_TIMER: Cell<Option<i32>> = const { Cell::new(None) };
}

/// Run `f` shortly after the current frame has been presented.
fn after_reveal(f: fn()) {
    #[cfg(target_arch = "wasm32")]
    {
        let Some(window) = web_sys::window() else {
            f();
            return;
        };
        cancel_reveal();
        let run = wasm_bindgen::closure::Closure::once_into_js(move || {
            REVEAL_TIMER.with(|slot| slot.set(None));
            if LIVE_CTX.with(|ctx| ctx.borrow().is_some()) {
                f();
            }
        });
        match window.set_timeout_with_callback_and_timeout_and_arguments_0(
            run.unchecked_ref(),
            REVEAL_SETTLE_MS,
        ) {
            Ok(id) => REVEAL_TIMER.with(|slot| slot.set(Some(id))),
            Err(_) => f(),
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    f();
}

/// Cancel the session-owned reveal timer before unmount/removal.
fn cancel_reveal() {
    #[cfg(target_arch = "wasm32")]
    if let Some(id) = REVEAL_TIMER.with(Cell::take)
        && let Some(window) = web_sys::window()
    {
        window.clear_timeout_with_handle(id);
    }
}

/// How long a revealed shelf gets to paint before its incoming-frame passes start.
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
            LibraryCommand::ImportFiles { paths } => {
                let shelf = ctx.library.shelf.get_untracked();
                let target = (shelf != library_core::shelf::ALL_SHELF).then_some(shelf);
                services::import_files(ctx, paths, target);
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
    cancel_reveal();
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

/// The unhosted development entry uses the same production session as a
/// hosted frame. Its standalone API writes durable data without a Shell.
pub fn run_standalone() {
    console_error_panic_hook::set_once();
    let host = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.body())
        .map(web_sys::Element::from)
        .expect("document body for the standalone library");
    start_session(&host, context::ApiHandle::Standalone, false);
}
