//! The library's hosted frame boot: `library.html?hosted=1&g=<generation>&n=<nonce>`.
//!
//! The marker is the WHOLE difference between a standalone page and a frame
//! (§6): no window-object sniffing, no fallback to standalone — a boot that
//! sees the marker becomes a frame or nothing. The identity in the URL is
//! what the Shell echoes in its channel offer, so the adoption below is the
//! first and last unauthenticated message this boot ever accepts (§8);
//! everything after it flows over the frame's own port, stamped with the
//! generation, under the protocol vocabulary (`runtime-contract::protocol`).
//!
//! The session itself is the same `start_session` the standalone and the
//! legacy hosted boots use: the frame is a transport and a boot, never a
//! second implementation of the shelf (§5's "no second UI" rule).

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use frame_transport::wasm::PortWire;
use frame_transport::{PendingResolves, PortShellApi};
use runtime_contract::protocol::RuntimeFrame;
#[cfg(target_arch = "wasm32")]
use runtime_contract::protocol::{ShellEnvelope, ShellFrame};
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
    let Some((generation, nonce)) = marker() else {
        return false;
    };
    console_error_panic_hook::set_once();
    frame_transport::wasm::adopt_channel(generation, nonce, move |wire| {
        start_frame(wire, generation);
    });
    true
}

fn marker() -> Option<(u64, String)> {
    let search = web_sys::window()?.location().search().ok()?;
    frame_transport::parse_hosted_marker(&search)
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

    // The explicit runtime root (§9): the shell's painted-versus-mounted
    // handshake, and the DOM identity the lifecycle tests read.
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

    let id = crate::start_session(&root, ApiHandle::Frame);
    SESSION_ID.with(|slot| slot.set(Some(id)));
    emit(RuntimeFrame::Status {
        stage: runtime_contract::protocol::BootStage::Mounted,
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
                ShellFrame::Init { runtime, .. } => {
                    // The library's init carries no payload; the kind check is
                    // the only acknowledgement a wrong-frame boot could fake.
                    debug_assert_eq!(runtime, runtime_contract::protocol::RuntimeKind::Library);
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
                ShellFrame::Dispose => {
                    if let Some(id) = SESSION_ID.with(|slot| slot.get()) {
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
                    // Neither means anything to the shelf: a document launch is
                    // the reader's command, and the library never asks the Shell
                    // to resolve one. Dropped by the protocol, not by accident.
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
