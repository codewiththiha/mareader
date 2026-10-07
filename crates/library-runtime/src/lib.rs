//! The library runtime: the shelf as its own WASM artifact.

pub mod context;
pub mod effects_library;
pub mod features;
#[cfg(target_arch = "wasm32")]
pub mod frame;

/// The off-wasm stub, so the frame's call sites still compile.
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

/// One live library session: the unmount handle.
struct Session {
    id: u32,
    unmount: Box<dyn FnOnce()>,
}

thread_local! {
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
    static NEXT_ID: Cell<u32> = const { Cell::new(1) };
    /// The live session's context, for commands outside any page event.
    static LIVE_CTX: RefCell<Option<LibraryContext>> = const { RefCell::new(None) };
}

/// Mount a library session into `host`; the frame defers startup writes.
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
        // Scoped to THIS session: state, effects and UI mount and die here.
        provide_context(state.library.covers);
        // The shelf's menus and modals arbitrate through one overlay board.
        provide_context(OverlayBoard::default());
        // Session effects install inside this scope and die with the unmount.
        effects_library::library_effects(state, defer_startup);
        // This frame's `<html>`: chrome only; edits go to the Shell to persist.
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
    /// An answered bake: the engine's raster, or `None` when it failed.
    #[serde(rename_all = "camelCase")]
    CoverBaked {
        path: String,
        /// Arc-free on the wire; the filing queue re-wraps it.
        image: Option<Box<runtime_contract::covers::CoverImage>>,
    },
    /// Newly revealed: reconcile late durable writes and start deferred work.
    Refresh,
    /// Files dropped on the window, imported as the picker would.
    ImportFiles { paths: Vec<String> },
}

/// Re-read the store into the live session's signals.
fn refresh(ctx: LibraryContext) {
    // Only what changed since the mount: covers left as they are.
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
    // Start deferred passes after paint; the timer dies with the session.
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
    /// What the live session last read from the store.
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

/// How long a revealed shelf gets to paint.
#[cfg(target_arch = "wasm32")]
const REVEAL_SETTLE_MS: i32 = 160;

/// Run one command; a stale session id is dropped, not answered.
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

/// Dispose the library session; the manager replaces runtimes.
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
            // No async tails: the promise resolves on the next microtask.
            (session.unmount)();
        }
    });
    if let Some(resolve) = resolve_fn {
        let _ = resolve.call0(&js_sys::global());
    }
    promise
}

/// The unhosted development entry, on the production session.
pub fn run_standalone() {
    console_error_panic_hook::set_once();
    let host = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.body())
        .map(web_sys::Element::from)
        .expect("document body for the standalone library");
    start_session(&host, context::ApiHandle::Standalone, false);
}
