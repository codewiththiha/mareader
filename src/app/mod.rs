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

#[cfg(all(format_runtime, target_arch = "wasm32"))]
pub(crate) use bootstrap::provide_app_contexts as bootstrap_contexts;
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

#[cfg(all(feature = "session-library", target_arch = "wasm32"))]
mod shelf {
    use std::any::Any;
    use std::cell::RefCell;

    use leptos::prelude::*;

    use super::{
        create_app_state, install_library_session, provide_app_contexts, LibrarySession,
    };

    thread_local! {
        static HANDLE: RefCell<Option<Box<dyn Any>>> = const { RefCell::new(None) };
    }

    pub fn mount() {
        console_error_panic_hook::set_once();
        crate::memory::log_heap("library-mount");
        let state = create_app_state();
        provide_context(state);
        let (appearance, typography) = provide_app_contexts(state);
        install_library_session(state, appearance, typography);
        let handle =
            leptos::mount::mount_to_body(move || view! { <LibrarySession state=state /> });
        HANDLE.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(handle));
        });
    }

    pub fn dispose() {
        crate::memory::log_heap("library-drop");
        HANDLE.with(|slot| {
            slot.borrow_mut().take();
        });
    }
}

/// Mount the shelf and nothing else. The library artifact calls this. The
/// reader host is not started on that page, so this heap never holds a book.
#[cfg(all(feature = "session-library", target_arch = "wasm32"))]
pub fn mount_shelf() {
    shelf::mount();
}

/// Drop the shelf view. Idempotent: a second call finds an empty handle.
#[cfg(all(feature = "session-library", target_arch = "wasm32"))]
pub fn dispose_shelf() {
    shelf::dispose();
}

#[cfg(all(feature = "session-host", target_arch = "wasm32"))]
mod reader_host {
    use std::any::Any;
    use std::cell::RefCell;

    use leptos::prelude::*;

    use super::App;

    thread_local! {
        static HANDLE: RefCell<Option<Box<dyn Any>>> = const { RefCell::new(None) };
    }

    pub fn mount() {
        console_error_panic_hook::set_once();
        crate::memory::log_heap("host-mount");
        // The same tree the Trunk bin mounted. `boot.kind` is already reader,
        // so this is the chrome, not a second shelf.
        let handle = leptos::mount::mount_to_body(|| view! { <App /> });
        HANDLE.with(|slot| {
            *slot.borrow_mut() = Some(Box::new(handle));
        });
    }

    pub fn dispose() {
        crate::memory::log_heap("host-drop");
        HANDLE.with(|slot| {
            slot.borrow_mut().take();
        });
    }
}

/// Mount the reader chrome and nothing else. The host artifact calls this.
/// The shelf module is already released, so this heap does not hold the shelf.
#[cfg(all(feature = "session-host", target_arch = "wasm32"))]
pub fn mount_reader() {
    reader_host::mount();
}

/// Drop the reader chrome. The book instance is released separately, first.
#[cfg(all(feature = "session-host", target_arch = "wasm32"))]
pub fn dispose_reader() {
    reader_host::dispose();
}
