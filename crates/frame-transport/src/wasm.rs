//! The wasm half of the transport: the `MessagePort` wire and the channel
//! adoption that pairs a hosted boot with its Shell (§7, §8). Compiled only
//! for `wasm32` — the artifacts are the only place this can run.

use wasm_bindgen::{JsCast, JsValue, closure::Closure};

use crate::{CHANNEL_KIND, Wire};

/// `Wire` over the frame port: one posted envelope is one port message.
#[derive(Clone)]
pub struct PortWire {
    port: web_sys::MessagePort,
}

impl PortWire {
    /// The raw port, for the session's command side (the Shell's envelopes
    /// arriving IN). The transport itself only ever posts OUT.
    pub fn port(&self) -> &web_sys::MessagePort {
        &self.port
    }
}

impl Wire for PortWire {
    fn post_json(&self, json: String) {
        // A post can only fail on a closed port — the Shell swapped or
        // removed the frame's other end from under us. Dropping is the
        // answer: nothing on this side of a dead port can leave it alive.
        let _ = self.port.post_message(&JsValue::from_str(&json));
    }
}

/// What the Shell's `postMessage` identifies the frame with. Everything else
/// on the wire is port traffic under the protocol vocabulary; THIS is the
/// one message a hosted runtime ever accepts without the port.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChannelOffer {
    kind: String,
    generation: u64,
    nonce: String,
}

/// Adopt the channel the Shell pairs with this frame: listen for the
/// nonce+generation offer matching the IDENTITY THIS BOOT PARSED FROM ITS
/// OWN URL. A message from any other generation or with another nonce is not
/// the Shell this frame was built for — dropped, never adopted (§8: the init
/// nonce authenticates the channel establishment itself). `on_adopted` runs
/// with the port wire; the runtime boots its session only inside it, so a
/// frame the Shell never claims never becomes a runtime.
///
/// The listener is never unregistered on purpose: it guards the handoff and
/// dies with the frame's document at disposal — one closure per frame
/// lifetime, exactly its own garbage collection (§14's rule that cleanup
/// never depends on the iframe removal is about RUNTIME work; this listener
/// owns no runtime work until the offer arrives).
pub fn adopt_channel(generation: u64, nonce: String, on_adopted: impl FnOnce(PortWire) + 'static) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let mut adopt = Some(on_adopted);
    let listener = Closure::new(move |event: web_sys::MessageEvent| {
        let data: JsValue = event.data();
        let Ok(json) = js_sys::JSON::stringify(&data) else {
            return;
        };
        let Ok(offer) = serde_json::from_str::<ChannelOffer>(&String::from(json)) else {
            return;
        };
        if offer.kind != CHANNEL_KIND || offer.generation != generation || offer.nonce != nonce {
            return;
        }
        let port: web_sys::MessagePort = {
            let ports = event.ports();
            if ports.length() == 0 {
                return;
            }
            ports.get(0).unchecked_into()
        };
        if let Some(adopt) = adopt.take() {
            adopt(PortWire { port });
        }
    });
    let added =
        window.add_event_listener_with_callback("message", listener.as_ref().unchecked_ref());
    if added.is_ok() {
        // Leaks the closure until the frame's document is gone — by design,
        // see the doc comment above.
        listener.forget();
    }
}
