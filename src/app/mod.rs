//! Application root: boots the persisted state, provides the app contexts,
//! installs the effects for this page, and mounts that page.
//!
//! Which page is `src/boot.rs`. The shelf and the reader are not routes of
//! one tree; each is a document, and leaving one drops its wasm instance.

mod bootstrap;
mod effects;
mod shell;

use leptos::prelude::*;

use bootstrap::{create_app_state, provide_app_contexts};
use effects::{install_library_session, install_reader_session};
use shell::{LibrarySession, ReaderSession};

#[component]
pub fn App() -> impl IntoView {
    let state = create_app_state();
    provide_context(state);
    let (appearance, typography) = provide_app_contexts(state);

    if crate::boot::is_reader() {
        install_reader_session(state, appearance, typography);
        view! { <ReaderSession state=state /> }.into_any()
    } else {
        install_library_session(state, appearance, typography);
        view! { <LibrarySession state=state /> }.into_any()
    }
}
