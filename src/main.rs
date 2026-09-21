mod app;
mod components;
mod effects;
mod events;
mod features;
mod memory;
mod services;
mod state;
mod storage;
mod time;

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
