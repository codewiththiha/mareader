mod app;
mod components;
mod dom_contract;
mod effects;
mod events;
mod features;
mod services;
mod state;
mod storage;
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
