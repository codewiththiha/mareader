//! The reader's hosted frame boot: `reader.html?hosted=1&g=<generation>&n=<nonce>`.
//!
//! Same rules as the library's frame: the URL marker is the whole boot
//! descriptor (§6), the channel offer's nonce authenticates the adoption
//! (§8), and the session is the same `start_session` every other boot uses —
//! a frame is a transport, never a second reader.
//!
//! Two reader-only shapes sit on top:
//!
//! - The launch descriptor rides the Shell's `init`: the frame cannot mount
//!   before it arrives (a reader IS the document it was opened with), so the
//!   session starts inside the init handler, not at adoption.
//! - The one synchronous bridge query, `resolve_launch`, becomes a port
//!   round trip here: an in-session open asks, parks its continuation in
//!   [`PENDING_OPENS`], and runs when the answer lands.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use frame_transport::wasm::PortWire;
use frame_transport::{PendingResolves, PortShellApi};
use runtime_contract::boundary::LaunchDocument;
use runtime_contract::protocol::{BootStage, RuntimeFrame};
#[cfg(target_arch = "wasm32")]
use runtime_contract::protocol::{RuntimeKind, ShellEnvelope, ShellFrame};
use wasm_bindgen::JsCast;

use crate::context::{ApiHandle, ReaderContext};

thread_local! {
    /// The live frame's boundary (set at adoption; it dies with the frame).
    static API: RefCell<Option<PortShellApi<PortWire>>> = const { RefCell::new(None) };
    /// The session the frame started, for the Shell's command traffic.
    static SESSION_ID: Cell<Option<u32>> = const { Cell::new(None) };
    /// The resolve round trips this frame asked, by request id. The Shell
    /// ALWAYS answers a resolve (a "no row" answer answers None), so an offer
    /// without a parked query never ages over a frame swap — the port dies
    /// with the document.
    static RESOLVES: RefCell<Option<Rc<PendingResolves>>> = const { RefCell::new(None) };
    /// The open flows parked on their resolve answers, by request id. A drop
    /// / dialog open arrives sync; its launch resolution is async over the
    /// port, and this is the rendezvous.
    static PENDING_OPENS: RefCell<HashMap<u64, (ReaderContext, String)>> =
        RefCell::new(HashMap::new());
}

/// Boot through the frame when this artifact's URL names one. `true` as soon
/// as the marker stands: a hosted boot never falls back to standalone (§6).
pub fn boot_if_hosted() -> bool {
    let Some((generation, nonce)) = marker() else {
        return false;
    };
    console_error_panic_hook::set_once();
    frame_transport::wasm::adopt_channel(generation, nonce, move |wire| {
        adopt(wire, generation);
    });
    true
}

fn marker() -> Option<(u64, String)> {
    let search = web_sys::window()?.location().search().ok()?;
    frame_transport::parse_hosted_marker(&search)
}

/// Run `f` against the live frame api when there is one.
pub fn with_api<R>(f: impl FnOnce(&PortShellApi<PortWire>) -> R) -> Option<R> {
    API.with(|api| api.borrow().as_ref().map(f))
}

fn emit(body: RuntimeFrame) {
    with_api(|api| api.emit(body));
}

/// The frame-side open flow: ask the Shell to resolve `path` against the
/// persisted library, park the continuation, run it when the answer lands.
/// The frame has no synchronous boundary query — the only honest async form
/// of "open, resumed where the library says" is to wait for the answer.
pub fn open_path_in_frame(ctx: ReaderContext, path: String) {
    with_api(|api| {
        let (request, _ticket) = api.ask_resolve_launch(&path);
        PENDING_OPENS.with(|opens| opens.borrow_mut().insert(request, (ctx, path)));
    });
}

fn adopt(wire: PortWire, generation: u64) {
    // Re-offers for this same boot adopt nothing: the first channel answers
    // first, and a second listener would only double the traffic.
    if API.with(|api| api.borrow().is_some()) {
        return;
    }
    let resolves = Rc::new(PendingResolves::default());
    let api = PortShellApi::new(wire.clone(), generation, resolves.clone());
    RESOLVES.with(|slot| *slot.borrow_mut() = Some(resolves));
    API.with(|slot| *slot.borrow_mut() = Some(api));
    #[cfg(target_arch = "wasm32")]
    install_shell_listener(wire.port().clone(), generation);
    // First contact: the reader cannot mount before the Shell's `init` (a
    // reader IS its document), so its first word on the port is "alive and
    // waiting" — that emission is also how the Shell learns which of its
    // offered channels this boot adopted.
    emit(RuntimeFrame::Status {
        stage: BootStage::Initialized,
    });
    // No session yet: a reader is the document it was opened with, and that
    // descriptor arrives with the Shell's init. The handshake's loading
    // stages are the Shell's clock, not this boot's guess — only a frame the
    // Shell claims ever becomes a reader.
}

/// The Shell's `init`: the frame's one launch. Mounts the runtime root the
/// handshake names (§9) and starts the session; everything after is the
/// boot stages on the port.
fn on_init(launch: Option<Box<LaunchDocument>>, generation: u64) {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let Some(body) = document.body() else {
        return;
    };
    let Ok(root) = document.create_element("div") else {
        return;
    };
    root.set_attribute("id", "runtime-root").ok();
    root.set_attribute("data-mareader-runtime", "reader").ok();
    root.set_attribute("data-mareader-generation", &generation.to_string())
        .ok();
    let _ = body.append_child(&root);

    // An init without a descriptor is a reader with nothing open — the same
    // shape as a standalone boot without launch parameters, so the same
    // default answers it.
    let launch = launch
        .map(|launch| *launch)
        .unwrap_or_else(crate::web_launch);
    let id = crate::start_session(&root, launch, ApiHandle::Frame);
    SESSION_ID.with(|slot| slot.set(Some(id)));
    crate::diagnostics::set_reader_live(true);
    emit(RuntimeFrame::Status {
        stage: BootStage::Mounted,
    });
    emit(RuntimeFrame::Ready);
    report_painted();
}

#[cfg(target_arch = "wasm32")]
fn install_shell_listener(port: web_sys::MessagePort, generation: u64) {
    let listener = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::MessageEvent)>::new(
        move |event: web_sys::MessageEvent| {
            let data: wasm_bindgen::JsValue = event.data();
            let Ok(json) = js_sys::JSON::stringify(&data) else {
                return;
            };
            let Ok(envelope) = serde_json::from_str::<ShellEnvelope>(&String::from(json)) else {
                return;
            };
            if envelope.generation != generation {
                return;
            }
            match envelope.body {
                ShellFrame::Init { runtime, launch } => {
                    if runtime == RuntimeKind::Reader {
                        on_init(launch, generation);
                    }
                }
                ShellFrame::Launch { document } => {
                    if let Some(id) = SESSION_ID.with(|slot| slot.get()) {
                        crate::command(id, *document);
                    }
                }
                ShellFrame::ResolveLaunchAnswer { request, document } => {
                    on_resolve_answer(request, document.map(|document| *document));
                }
                ShellFrame::Dispose => {
                    if let Some(id) = SESSION_ID.with(|slot| slot.get()) {
                        let promise = crate::dispose(id);
                        crate::diagnostics::set_reader_live(false);
                        wasm_bindgen_futures::spawn_local(async move {
                            let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
                            emit(RuntimeFrame::DisposeComplete);
                        });
                    }
                }
                ShellFrame::CoverBaked { .. } => {
                    // The shelf's command on the reader's port: the bake round
                    // trip belongs to the library frame. Dropped by the
                    // protocol, never silently.
                }
            }
        },
    );
    port.set_onmessage(Some(listener.as_ref().unchecked_ref()));
    // One listener per frame lifetime; the iframe removal is its GC.
    listener.forget();
}

/// A resolve round trip landing: the parked open flow continues with the
/// descriptor, or with a bare launch for a path the store never knew (the
/// read record mints the row, same as the same-page bridge's None answer).
fn on_resolve_answer(request: u64, document: Option<LaunchDocument>) {
    if let Some(resolves) = RESOLVES.with(|slot| slot.borrow().clone()) {
        resolves.settle(request, document.clone());
    }
    if let Some((ctx, path)) = PENDING_OPENS.with(|opens| opens.borrow_mut().remove(&request)) {
        let launch =
            document.unwrap_or_else(|| crate::services::document::open::bare_launch(&path));
        crate::services::document::open::open_with_launch(ctx, launch);
    }
}

/// `Painted` after the mount has had its paint opportunity (§9): two frames,
/// so layout and presentation happened before the Shell lifts its cover.
fn report_painted() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let once = wasm_bindgen::closure::Closure::<dyn FnMut(f64)>::new(move |_: f64| {
        let Some(window) = web_sys::window() else {
            return;
        };
        let twice = wasm_bindgen::closure::Closure::<dyn FnMut(f64)>::new(move |_: f64| {
            emit(RuntimeFrame::Painted);
        });
        let _ = window.request_animation_frame(twice.as_ref().unchecked_ref());
        twice.forget();
    });
    let _ = window.request_animation_frame(once.as_ref().unchecked_ref());
    once.forget();
}
