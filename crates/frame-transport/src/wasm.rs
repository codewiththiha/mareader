//! The wasm half of the transport: the port wire and the channel adoption.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::{JsCast, JsValue, closure::Closure};

#[cfg(target_arch = "wasm32")]
use crate::CHANNEL_KIND;
use crate::Wire;

/// `Wire` over the frame port: one posted envelope is one port message.
#[cfg(target_arch = "wasm32")]
#[derive(Clone)]
pub struct PortWire {
    port: web_sys::MessagePort,
}

/// The host-compile shape: off wasm there is only the port's name.
pub struct PortWire;

#[cfg(target_arch = "wasm32")]
impl PortWire {
    /// The raw port, for the session's command side.
    pub fn port(&self) -> &web_sys::MessagePort {
        &self.port
    }
}

#[cfg(target_arch = "wasm32")]
impl Wire for PortWire {
    fn post_json(&self, json: String) {
        // A post can only fail on a closed port; dropping is the answer.
        let _ = self.port.post_message(&JsValue::from_str(&json));
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Wire for PortWire {
    /// Nothing to send over on the host: no port ever adopted this boot.
    fn post_json(&self, _json: String) {}
}

/// The one message a hosted runtime accepts without the port.
struct ChannelOffer {
    kind: String,
    generation: u64,
    nonce: String,
}

/// Adopt the channel whose nonce and generation match this boot's URL.
pub fn adopt_channel(generation: u64, nonce: String, on_adopted: impl FnOnce(PortWire) + 'static) {
    let Some(window) = web_sys::window() else {
        return;
    };
    let parent = window.parent().ok().flatten();
    let origin = window.location().origin().ok();
    let mut adopt = Some(on_adopted);
    let listener = Closure::new(move |event: web_sys::MessageEvent| {
        if parent.as_ref().is_none_or(|parent| {
            event
                .source()
                .is_none_or(|source| !js_sys::Object::is(source.as_ref(), parent.as_ref()))
        }) || origin
            .as_ref()
            .is_none_or(|origin| event.origin() != *origin)
        {
            return;
        }
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
        // Leaks the closure until the document is gone — by design.
        listener.forget();
    }
}

/// Off wasm there is no channel to adopt; the function never calls `f`.
pub fn adopt_channel(
    _generation: u64,
    _nonce: String,
    _on_adopted: impl FnOnce(PortWire) + 'static,
) {
}
