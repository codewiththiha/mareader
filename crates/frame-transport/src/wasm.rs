//! The wasm half of the transport: the `MessagePort` wire (§7, §8). Compiled only
//! for `wasm32` — the artifacts are the only place this can run.

use crate::Wire;
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsValue;

/// `Wire` over the frame port: one posted envelope is one port message.
#[cfg(target_arch = "wasm32")]
#[derive(Clone)]
pub struct PortWire {
    port: web_sys::MessagePort,
}

/// The host-compile shape: the port is a wasm artifact, so off wasm there is
/// only the name (a runtime's frame context compiles on the host test lanes,
/// where no adoption ever happens and so no wire ever exists).
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone)]
pub struct PortWire;

#[cfg(target_arch = "wasm32")]
impl PortWire {
    /// A wire over a port the host handed over directly: a runtime mounted
    /// in the Shell's own document is given its end of the channel instead
    /// of adopting it from a window message.
    pub fn new(port: web_sys::MessagePort) -> Self {
        Self { port }
    }

    /// The raw port, for the session's command side (the Shell's envelopes
    /// arriving IN). The transport itself only ever posts OUT.
    pub fn port(&self) -> &web_sys::MessagePort {
        &self.port
    }
}

#[cfg(target_arch = "wasm32")]
impl Wire for PortWire {
    fn post_json(&self, json: String) {
        // A post can only fail on a closed port — the Shell swapped or
        // removed the frame's other end from under us. Dropping is the
        // answer: nothing on this side of a dead port can leave it alive.
        let _ = self.port.post_message(&JsValue::from_str(&json));
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Wire for PortWire {
    /// Nothing to send over on the host: no port ever adopted this boot.
    fn post_json(&self, _json: String) {}
}
