//! The host's plain-data raster leases; frame removal reclaims its
//! keys.

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::{JsCast, JsValue};

#[cfg(target_arch = "wasm32")]
fn call(method: &str, argument: Option<&str>) -> Option<JsValue> {
    let window = web_sys::window()?;
    let lane = js_sys::Reflect::get(&window, &"__mareaderRasterLane".into()).ok()?;
    let function = js_sys::Reflect::get(&lane, &method.into())
        .ok()?
        .dyn_into::<js_sys::Function>()
        .ok()?;
    match argument {
        Some(value) => function.call1(&lane, &value.into()).ok(),
        None => function.call0(&lane).ok(),
    }
}

pub(super) fn retire(nonce: &str) {
    #[cfg(target_arch = "wasm32")]
    {
        let scope = web_sys::window().and_then(|window| {
            js_sys::Reflect::get(&window, &"__mareaderRasterScope".into())
                .ok()?
                .as_string()
        });
        if let Some(scope) = scope {
            let owner = format!("{scope}/{nonce}");
            let _ = call("retire", Some(&owner));
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    let _ = nonce;
}

pub(crate) fn snapshot() -> Option<serde_json::Value> {
    #[cfg(target_arch = "wasm32")]
    {
        let value = call("snapshot", None)?;
        let json = js_sys::JSON::stringify(&value).ok()?.as_string()?;
        serde_json::from_str(&json).ok()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        None
    }
}
