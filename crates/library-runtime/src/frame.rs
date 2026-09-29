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
use runtime_contract::boundary::DocumentDragDescriptor;
#[cfg(target_arch = "wasm32")]
use runtime_contract::protocol::BootStage;
use runtime_contract::protocol::{DragPointerPhase, RuntimeFrame};
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
    /// When the shelf last told the Shell to expect a reader (`expect_reader`).
    static LAST_EXPECT_READER_MS: Cell<Option<u64>> = const { Cell::new(None) };
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

/// Hand a held book to the Shell: the pointer entered the shelf's "Open in
/// Reader" zone at client `(x, y)`. From here the Shell owns the drag; the
/// library only forwards the pointer it still receives
/// ([`reader_drag_pointer`]). The descriptor is data — no runtime object of
/// either side crosses.
pub fn begin_reader_drag(source: DocumentDragDescriptor, x: f64, y: f64) {
    emit(RuntimeFrame::BeginDocumentDrag {
        source: Box::new(source),
        x,
        y,
    });
}

/// The pointer of a drag the Shell owns, as this frame still receives it
/// (the press began here, so the browser keeps routing it here), in this
/// frame's client coordinates.
pub fn reader_drag_pointer(x: f64, y: f64, phase: DragPointerPhase) {
    emit(RuntimeFrame::DocumentDragPointer { x, y, phase });
}

/// How often the shelf repeats its intent hint at most. A pointer crossing
/// the grid raises `pointerover` on every card edge; the Shell only needs to
/// hear "still here" about once a second to keep its reader booted.
const EXPECT_READER_THROTTLE_MS: u64 = 1_000;

/// Tell the Shell a book is about to be opened (the pointer is over the
/// shelf, a card has focus or is pressed). The Shell boots its reader
/// behind the shelf on this word alone — it no longer warms one on every
/// shelf paint — and every repeat keeps that reader from being evicted as
/// idle, so the open that follows is still a reveal while a shelf nobody is
/// touching keeps no reader resident. Standalone (no Shell) there is nobody
/// to tell, and the call is a no-op.
pub fn expect_reader() {
    let now = runtime_contract::time::now_ms();
    let due = LAST_EXPECT_READER_MS.with(|last| {
        let due = last
            .get()
            .is_none_or(|sent| now.saturating_sub(sent) >= EXPECT_READER_THROTTLE_MS);
        if due {
            last.set(Some(now));
        }
        due
    });
    if due {
        emit(RuntimeFrame::ExpectReader);
    }
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
    // First contact: the shelf no longer mounts on adoption either. The
    // Shell's `init` carries `warm`, and a boot has to know it is warm
    // BEFORE its first effect runs — the shelf's startup passes write
    // durable state a live reader session is meanwhile editing. So this
    // frame's first word is "alive and waiting", exactly as the reader's is.
    emit(RuntimeFrame::Status {
        stage: BootStage::Initialized,
    });
}

/// The Shell's `init`: mount the runtime root (§9) and start the session.
/// `warm` is the Shell's statement that this boot runs ahead of the
/// navigation that will use it, so the shelf renders but does not work.
fn on_init(warm: bool, generation: u64) {
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

    // A warm shelf is hidden behind the reader until `Refresh` reveals it;
    // the document is told so (the animated grain pauses while nobody can
    // see it).
    app_ui::frame_theme::mark_frame_hidden(warm);
    let id = crate::start_session(&root, ApiHandle::Frame, warm);
    SESSION_ID.with(|slot| slot.set(Some(id)));
    emit(RuntimeFrame::Status {
        stage: BootStage::Mounted,
    });
    emit(RuntimeFrame::Ready);
    report_painted();
}

/// The Shell recycling this frame after a completed disposal: a fresh warm
/// shelf mounts in the document that already paid for the wasm instance.
/// Only ever sent after `DisposeComplete`, so the previous session is gone.
#[cfg(target_arch = "wasm32")]
fn on_rearm(generation: u64) {
    if SESSION_ID.with(|slot| slot.get()).is_some() {
        return;
    }
    if let Some(document) = web_sys::window().and_then(|w| w.document()) {
        while let Some(root) = document.get_element_by_id("runtime-root") {
            root.remove();
        }
    }
    on_init(true, generation);
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
                    warm,
                } => {
                    // The kind check is the only acknowledgement a
                    // wrong-frame boot could fake; `warm` is the payload that
                    // matters — it decides whether this shelf works now or
                    // when it is revealed.
                    debug_assert_eq!(runtime, RuntimeKind::Library);
                    on_init(warm, generation);
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
                    // Promoted from warm: the shelf it seeded at boot is a
                    // snapshot from before the reader session ran, and a
                    // reader session moves read points and adds books. Re-read
                    // the store rather than booting a new runtime — that read
                    // is the whole reason warming is worth its memory.
                    app_ui::frame_theme::mark_frame_hidden(false);
                    if let Some(id) = SESSION_ID.with(|slot| slot.get()) {
                        crate::command(id, crate::LibraryCommand::Refresh);
                    }
                }
                ShellFrame::Rearm => {
                    on_rearm(generation);
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
                ShellFrame::Launch { .. }
                | ShellFrame::ResolveLaunchAnswer { .. }
                | ShellFrame::DocumentDrag { .. } => {
                    // None means anything to the shelf: a document launch
                    // and a carried drag's steps are the reader's, and the
                    // library never asks the Shell to resolve a launch.
                    // Dropped by the protocol, not by accident.
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
