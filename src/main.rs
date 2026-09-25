mod app;
mod components;
mod diagnostics;
mod dom_contract;
mod effects;
mod epoch;
mod events;
mod features;
mod memory;
mod runtime;
mod services;
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
