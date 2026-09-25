//! The Shell: the persistent host the runtimes mount into. It owns routing
//! (one navigation authority — the runtime manager), the runtime loader and
//! manager, the durable settings/persistence writes, the bridge the runtimes
//! call, and the diagnostics surface that merges the manager's facts with
//! the active runtime's digest.

mod bootstrap;
mod bridge;
mod loader;
pub(crate) mod manager;

use leptos::html;
use leptos::prelude::*;

#[component]
pub fn Shell() -> impl IntoView {
    let state = bootstrap::create_shell_state();
    provide_context(state.clone());

    // The bridge: the boundary runtimes call back through (§14). Installed
    // before any runtime loads.
    bridge::install(state.clone());

    // The OS open handoff: the shell owns the file-event surface (§2) —
    // a drop/dialog lands here and becomes a reader launch.
    crate::services::install_os_open_handling(state.clone());

    // The diagnostics surface: the manager's facts merged with the active
    // runtime's last digest.
    crate::diagnostics::install(state.clone());

    // The shell's durable paints: theme/typography/motion on <html>. They
    // survive every runtime transition (§17).
    bootstrap::install_shell_effects(state.clone());

    // The one mount target. The shell controls it; exactly one runtime mounts
    // inside at a time (§4). The manager's starts mount into it — and they
    // take the element, so it is handed over once the view is in the DOM:
    // a lookup made while this component renders cannot see its own child.
    let host = NodeRef::<html::Div>::new();
    Effect::new({
        let state = state.clone();
        move |_| {
            let host = host.get().expect("runtime host element");
            state.manager.set_host(host.into());
            // Boot: whichever runtime the URL names.
            manager::RuntimeManager::boot(state.clone());
        }
    });

    view! {
        // `h-full w-full` is load-bearing: every runtime's root is `h-full`,
        // and a mount target with `height: auto` hands the reader's scroll
        // area an indefinite height — the virtualizer then measures the whole
        // column as visible and mounts every page. The size the app root used
        // to take from `<body>` (html/body are 100% in the shell stylesheet)
        // lives here now (§4: the Shell owns the one target).
        <div id="runtime-host" class="h-full w-full" node_ref=host></div>
        <div class="noise-overlay"></div>
    }
}
