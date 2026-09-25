//! The shell artifact's entry. The shell is the persistent host: routing,
//! the runtime manager, persistence, diagnostics. Library and reader are
//! separate artifacts it loads and disposes (docs/runtime-split.md).

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
