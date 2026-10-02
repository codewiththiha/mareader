//! The Library artifact boots in its own disposable iframe. The URL
//! authenticates the Shell's MessageChannel offer; every message is stamped
//! with that frame's generation,
//! under the protocol vocabulary (`runtime-contract::protocol`).
//!
//! The session itself is the one `start_session`: the boot is a transport,
//! never a second implementation of the shelf.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use frame_transport::wasm::PortWire;
use frame_transport::{PendingResolves, PortShellApi};
#[cfg(target_arch = "wasm32")]
use runtime_contract::protocol::BootStage;
use runtime_contract::protocol::RuntimeFrame;
#[cfg(target_arch = "wasm32")]
use runtime_contract::protocol::{RuntimeKind, ShellEnvelope, ShellFrame};
use wasm_bindgen::JsCast;

use crate::context::ApiHandle;

thread_local! {
    /// The live frame's boundary. Set when the channel is adopted, cleared
    /// never — the frame's document dying is the clearing.
    static API: RefCell<Option<PortShellApi<PortWire>>> = const { RefCell::new(None) };
    /// The session the frame started, for the Shell's command traffic.
    static SESSION_ID: Cell<Option<u32>> = const { Cell::new(None) };
}

/// Boot through the frame when this artifact's URL names one.
///
/// Returns `true` as soon as the marker stands — before any offer arrives —
/// because §6 forbids the fallback: a hosted boot that never hears from its
/// Shell stays a claimless frame, never a standalone page.
pub fn boot_if_hosted() -> bool {
    let search = web_sys::window()
        .and_then(|window| window.location().search().ok())
        .unwrap_or_default();
    match frame_transport::parse_boot_marker(&search) {
        frame_transport::BootMarker::Standalone => false,
        frame_transport::BootMarker::InvalidHosted => true,
        frame_transport::BootMarker::Hosted { generation, nonce } => {
            console_error_panic_hook::set_once();
            frame_transport::wasm::adopt_channel(generation, nonce, move |wire| {
                start_frame(wire, generation);
            });
            true
        }
    }
}

/// Run `f` against the live frame api when there is one. Command traffic
/// before the adoption (impossible in practice: the session starts after it)
/// simply has no channel to speak over.
pub fn with_api<R>(f: impl FnOnce(&PortShellApi<PortWire>) -> R) -> Option<R> {
    API.with(|api| api.borrow().as_ref().map(f))
}

/// One outgoing message, for the boot's own stage reporting below.
fn emit(body: RuntimeFrame) {
    with_api(|api| api.emit(body));
}

fn start_frame(wire: PortWire, generation: u64) {
    // The Shell re-offers the channel until it hears back; a second offer for
    // this same boot adopts nothing (the first channel's answer is how the
    // Shell picked its lane, and a re-mount here would rerun the session).
    if API.with(|api| api.borrow().is_some()) {
        return;
    }
    let resolves = Rc::new(PendingResolves::default());
    let api = PortShellApi::new(wire.clone(), generation, resolves);
    API.with(|slot| *slot.borrow_mut() = Some(api));
    #[cfg(target_arch = "wasm32")]
    install_shell_listener(wire.port().clone(), generation);
    // The init's visibility flag defers startup writes until reveal. No
    // route may write another route's store before its incoming paint.
    emit(RuntimeFrame::Status {
        stage: BootStage::Initialized,
    });
}

/// The Shell's `init`: mount the runtime root (§9) and start the session.
/// Hidden means an incoming route: paint first, start passes on reveal.
fn on_init(hidden: bool, generation: u64) {
    // A re-init for a boot that already mounted is not a second session: the
    // Shell mints one identity per frame and never reuses one.
    if SESSION_ID.with(|slot| slot.get()).is_some() {
        return;
    }
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
    root.set_attribute("data-mareader-runtime", "library").ok();
    root.set_attribute("data-mareader-generation", &generation.to_string())
        .ok();
    // `h-full w-full` is load-bearing: the same rule the Shell's target obeys
    // — a mount point with `height: auto` hands every full-height child an
    // indefinite measure instead of the frame's real one.
    root.set_attribute("class", "h-full w-full").ok();
    let _ = body.append_child(&root);
    app_ui::frame_theme::mark_frame_hidden(hidden);
    let id = crate::start_session(&root, ApiHandle::Frame, hidden);
    SESSION_ID.with(|slot| slot.set(Some(id)));
    emit(RuntimeFrame::Status {
        stage: BootStage::Mounted,
    });
    emit(RuntimeFrame::Ready);
    report_painted();
}

/// A Shell envelope arriving over the port. The generation guard is the
/// second wall (after the init nonce): a message addressed to another
/// generation is not this frame's business, whatever it says (§8, §35).
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
                ShellFrame::Init {
                    runtime,
                    launch: _,
                    hidden,
                } => {
                    debug_assert_eq!(runtime, RuntimeKind::Library);
                    on_init(hidden, generation);
                }
                ShellFrame::CoverBaked { path, image } => {
                    if let Some(id) = SESSION_ID.with(|slot| slot.get()) {
                        crate::command(
                            id,
                            crate::LibraryCommand::CoverBaked {
                                path,
                                image: image.map(Box::new),
                            },
                        );
                    }
                }
                ShellFrame::Refresh => {
                    // Fresh Library has painted. Reconcile late durable
                    // writes and run its previously deferred startup once.
                    app_ui::frame_theme::mark_frame_hidden(false);
                    if let Some(id) = SESSION_ID.with(|slot| slot.get()) {
                        crate::command(id, crate::LibraryCommand::Refresh);
                    }
                }
                ShellFrame::ImportFiles { paths } => {
                    if let Some(id) = SESSION_ID.with(|slot| slot.get()) {
                        crate::command(id, crate::LibraryCommand::ImportFiles { paths });
                    }
                }
                ShellFrame::Dispose => {
                    if let Some(id) = SESSION_ID.with(|slot| slot.take()) {
                        let promise = crate::dispose(id);
                        wasm_bindgen_futures::spawn_local(async move {
                            let _ = wasm_bindgen_futures::JsFuture::from(promise).await;
                            emit(RuntimeFrame::DisposeComplete);
                        });
                    } else {
                        emit(RuntimeFrame::DisposeComplete);
                    }
                }
                ShellFrame::Launch { .. } | ShellFrame::ResolveLaunchAnswer { .. } => {
                    // Neither means anything to the shelf: a document launch
                    // is the reader's command, and the library never asks
                    // the Shell to resolve a launch. Dropped by the
                    // protocol, not by accident.
                }
            }
        },
    );
    port.set_onmessage(Some(listener.as_ref().unchecked_ref()));
    // Dies with the frame's document — one listener per frame lifetime, and
    // the iframe's removal (after dispose-complete) is its garbage collector.
    listener.forget();
}

/// `Painted` after the mount has had its paint opportunity (§9): two frames
/// so the browser has laid out and presented the first surface before the
/// Shell is told it may lift the loading cover.
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
