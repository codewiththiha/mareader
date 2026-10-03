//! The artifact half of the frame protocol: the boot both disposable
//! runtime frames share (the Reader host and the shelf), so the two cannot
//! drift apart.
//!
//! Every hosted artifact does the same five things: parse the marker in its
//! own URL, adopt the nonce/generation authenticated channel offer, hold the
//! boundary, answer the adoption with its first stage, then mount the
//! runtime root the handshake names and report the paint. What differs is
//! only what an envelope means to it and what it starts inside that root —
//! those stay in the artifact's own `frame` module, which passes them in.
//!
//! The state is thread-local and per wasm instance: one frame boot per
//! artifact document, and the artifact's own frame module is the only
//! caller. Off `wasm32` the same code compiles for the host test lanes,
//! where no boot ever happens.

use std::cell::{Cell, RefCell};

use wasm_bindgen::JsCast;

use crate::PortShellApi;
use crate::wasm::PortWire;
#[cfg(target_arch = "wasm32")]
use runtime_contract::protocol::ShellEnvelope;
use runtime_contract::protocol::{BootStage, RuntimeFrame, ShellFrame};

thread_local! {
    /// The live frame's boundary. Set when the channel is adopted, cleared
    /// never — the frame's document dying is the clearing.
    static API: RefCell<Option<PortShellApi<PortWire>>> = const { RefCell::new(None) };
    /// The session the frame started, for the Shell's command traffic.
    static SESSION_ID: Cell<Option<u32>> = const { Cell::new(None) };
}

/// Boot through the frame when this artifact's URL names one, handing the
/// adopted port to `start` once the Shell's offer matches this boot.
///
/// Returns `true` as soon as the marker stands — before any offer arrives —
/// because §6 forbids the fallback: a hosted boot that never hears from its
/// Shell stays a claimless frame, never a standalone page.
pub fn boot_if_hosted(start: impl FnOnce(PortWire, u64) + 'static) -> bool {
    let search = web_sys::window()
        .and_then(|window| window.location().search().ok())
        .unwrap_or_default();
    match crate::parse_boot_marker(&search) {
        crate::BootMarker::Standalone => false,
        crate::BootMarker::InvalidHosted => true,
        crate::BootMarker::Hosted { generation, nonce } => {
            console_error_panic_hook::set_once();
            crate::wasm::adopt_channel(generation, nonce, move |wire| start(wire, generation));
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

/// One outgoing message, for the boot's own stage reporting.
pub fn emit(body: RuntimeFrame) {
    with_api(|api| api.emit(body));
}

/// The session this frame started: what the Shell's command traffic is
/// addressed to, and how the frame knows its own session is live.
pub fn session() -> Option<u32> {
    SESSION_ID.with(Cell::get)
}

pub fn set_session(id: u32) {
    SESSION_ID.with(|slot| slot.set(Some(id)));
}

/// Take the session id, leaving the frame session-less. The disposal path
/// uses it both as the once-guard and as the id to dispose.
pub fn take_session() -> Option<u32> {
    SESSION_ID.with(|slot| slot.take())
}

/// Adopt the Shell's channel for this boot: hold the boundary, install the
/// generation-guarded listener that hands every envelope to `on_frame`, and
/// answer the adoption with the boot's first stage. `false` when this boot
/// already adopted one — the Shell re-offers until it hears back, and a
/// second channel would only double the traffic.
pub fn adopt(wire: PortWire, generation: u64, on_frame: impl FnMut(ShellFrame) + 'static) -> bool {
    if API.with(|api| api.borrow().is_some()) {
        return false;
    }
    let api = PortShellApi::new(wire.clone(), generation);
    API.with(|slot| *slot.borrow_mut() = Some(api));
    #[cfg(target_arch = "wasm32")]
    install_listener(wire.port().clone(), generation, on_frame);
    #[cfg(not(target_arch = "wasm32"))]
    let _ = on_frame;
    emit(RuntimeFrame::Status {
        stage: BootStage::Initialized,
    });
    true
}

/// The Shell's envelopes arriving over the port. The generation guard is the
/// second wall (after the init nonce): a message addressed to another
/// generation is not this frame's business, whatever it says (§8, §35). One
/// listener per frame lifetime; the iframe's removal is its garbage
/// collector.
#[cfg(target_arch = "wasm32")]
fn install_listener(
    port: web_sys::MessagePort,
    generation: u64,
    mut on_frame: impl FnMut(ShellFrame) + 'static,
) {
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
            on_frame(envelope.body);
        },
    );
    port.set_onmessage(Some(listener.as_ref().unchecked_ref()));
    listener.forget();
}

/// Mount the runtime root the handshake names (§9) and hand it back.
///
/// `h-full w-full` is load-bearing: every runtime root is `h-full`, and a
/// mount point with `height: auto` hands a full-height child an indefinite
/// measure instead of the frame's real one (the reader's virtualizer then
/// measures the whole column as visible and mounts every page).
pub fn mount_root(kind: &str, generation: u64) -> Option<web_sys::Element> {
    let document = web_sys::window()?.document()?;
    let body = document.body()?;
    let root = document.create_element("div").ok()?;
    let generation = generation.to_string();
    let _ = root.set_attribute("id", "runtime-root");
    let _ = root.set_attribute("data-mareader-runtime", kind);
    let _ = root.set_attribute("data-mareader-generation", &generation);
    let _ = root.set_attribute("class", "h-full w-full");
    let _ = body.append_child(&root);
    Some(root)
}

/// `Painted` after the mount has had its paint opportunity (§9): two frames
/// so the browser has laid out and presented the first surface before the
/// Shell is told it may lift the loading cover.
pub fn report_painted() {
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
