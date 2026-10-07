//! The Shell: routing, the runtime frames, durable persistence, diagnostics.

pub(crate) mod bake;
mod boot;
mod bootstrap;
pub(crate) mod frame;
pub(crate) mod manager;

use leptos::html;
use leptos::prelude::*;

#[component]
pub fn Shell() -> impl IntoView {
    let state = bootstrap::create_shell_state();
    state.manager.attach_state(state.clone());
    provide_context(state.clone());

    // The OS file-event surface (§2): a drop or dialog becomes a reader launch.
    crate::services::install_os_open_handling(state.clone());

    // OS drag-drop imports reach the library, and only while it is on screen.
    let drop_hint = crate::services::install_import_drop(state.clone());

    // The diagnostics surface: the manager's facts merged with the active
    // runtime's last digest.
    crate::diagnostics::install(state.clone());

    // The shell's durable paints: theme/typography/motion on <html>. They
    // survive every runtime transition (§17).
    bootstrap::install_shell_effects(state.clone());
    bootstrap::install_history(state.clone());

    // The one mount target; exactly one runtime mounts inside at a time (§4).
    let host = NodeRef::<html::Div>::new();
    Effect::new({
        let state = state.clone();
        move |_| {
            // An unresolved ref shows the placeholder, never a blank window.
            let Some(element) = host.get() else {
                return;
            };
            state.manager.set_host(element.into());
            // Boot: whichever runtime the URL names.
            manager::RuntimeManager::boot(state.clone());
        }
    });

    // `#shell-boot` stays until boot.rs's coverage watch removes it: one owner.

    view! {
        // `h-full w-full` is load-bearing: auto-height mounts every page.
        <div id="runtime-host" class="h-full w-full" node_ref=host></div>
        // Over the frames, never taking a pointer: the drop is the window's.
        <Show when=move || drop_hint.get()>
            <div
                aria-hidden="true"
                class="pointer-events-none fixed inset-0 z-50 p-3"
            >
                <div class="flex h-full w-full items-center justify-center rounded-2xl border-2 border-dashed border-accent bg-accent/10">
                    <div class="rounded-xl bg-surface px-4 py-2.5 text-sm font-medium text-ink shadow-lg">
                        "Drop to add to your library"
                    </div>
                </div>
            </div>
        </Show>
    }
}
