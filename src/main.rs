//! The shell artifact's entry: the persistent host of routing, managers,
//! persistence and diagnostics.

mod app;
mod diagnostics;
mod effects;
mod services;
mod state;

use leptos::prelude::*;

fn main() {
    console_error_panic_hook::set_once();
    mount_to_body(|| {
        view! {
            <app::Shell/>
        }
    })
}
