//! The artifact half of the frame protocol: the boot both runtime frames share.

use std::cell::{Cell, RefCell};

use wasm_bindgen::JsCast;

use crate::PortShellApi;
use crate::wasm::PortWire;
#[cfg(target_arch = "wasm32")]
use runtime_contract::protocol::ShellEnvelope;
use runtime_contract::protocol::{BootStage, RuntimeFrame, ShellFrame};

thread_local! {
    /// The live frame's boundary, set when the channel is adopted.
    static API: RefCell<Option<PortShellApi<PortWire>>> = const { RefCell::new(None) };
    /// The session the frame started, for the Shell's command traffic.
    static SESSION_ID: Cell<Option<u32>> = const { Cell::new(None) };
}

/// Boot through the frame when this artifact's URL names one.
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

/// Run `f` against the live frame api when there is one.
pub fn with_api<R>(f: impl FnOnce(&PortShellApi<PortWire>) -> R) -> Option<R> {
    API.with(|api| api.borrow().as_ref().map(f))
}

/// One outgoing message, for the boot's own stage reporting.
pub fn emit(body: RuntimeFrame) {
    with_api(|api| api.emit(body));
}

/// The session this frame started: what the Shell's traffic is addressed to.
pub fn frame_session() -> Option<u32> {
    SESSION_ID.with(Cell::get)
}

pub fn set_frame_session(id: u32) {
    SESSION_ID.with(|slot| slot.set(Some(id)));
}

/// Take the session id, leaving the frame session-less: the disposal guard.
pub fn take_frame_session() -> Option<u32> {
    SESSION_ID.with(|slot| slot.take())
}

/// Adopt the Shell's channel for this boot and answer with the first stage.
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

/// The Shell's envelopes arriving over the port, behind the generation guard.
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

/// Mount the runtime root the handshake names; `h-full w-full` is load-bearing.
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

/// `Painted` after the mount has had two frames to lay out and present.
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
