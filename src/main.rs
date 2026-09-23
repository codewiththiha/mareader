mod app;
mod boot;
mod components;
#[cfg(all(format_runtime, target_arch = "wasm32"))]
mod dom_contract;
mod effects;
#[cfg(all(format_runtime, target_arch = "wasm32"))]
mod epoch;
mod events;
mod features;
mod memory;
mod services;
mod slot;
mod state;
mod storage;
mod time;
mod zoom;

use app::*;
use leptos::prelude::*;

fn main() {
    console_error_panic_hook::set_once();
    mount_to_body(|| {
        view! {
            <App/>
        }
    })
}
