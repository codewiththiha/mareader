//! Centered reader settings modal shell: the tab strip, the Escape handler and
//! the body that hosts one tab at a time. The tabs live in `layout`, `theme`
//! and `animations`, and the SET of them is not fixed — see `shown`.
//!
//! The `open` signal belongs to the page (two things open this modal: the gear
//! button and the reader menu's item), so the page's signal is registered as
//! [`OverlayPolicy::MODAL`] here. That is what makes opening a menu close the
//! modal and vice versa, without either component knowing about the other —
//! see [`lanes`](crate::components::primitives::overlay::lanes).

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use crate::components::settings::animations::AnimationsTab;
use crate::components::settings::common::{Tab, TabButton};
use crate::components::settings::layout::LayoutTab;
use crate::components::settings::theme::ThemeTab;
use crate::components::primitives::icon::IconName;
use crate::components::primitives::icon_button::IconButton;
use crate::components::primitives::overlay::lanes::{use_overlay_lane, OverlayPolicy};
use crate::state::AppState;

#[component]
pub fn SettingsModal(
    state: AppState,
    open: RwSignal<bool>,
    #[prop(default = "min(92vw, 620px)")] width: &'static str,
    #[prop(default = "min(76vh, 640px)")] height: &'static str,
) -> impl IntoView {
    // Mutual exclusion with the anchored menus, on the page's signal.
    use_overlay_lane(open, OverlayPolicy::MODAL);
    let tab = RwSignal::new(Tab::Layout);
    // The Animations tab is offered only while the master switch in the Layout
    // tab is on — an animations panel that cannot animate anything is worse
    // than no panel. `shown` is the tab the strip displays and the body renders
    // (rather than an effect writing `tab` back): turning the master off while
    // that tab happens to be open falls back to Layout for as long as it is
    // off, and the reader's own selection survives to be returned to.
    let animations_on = Signal::derive(move || state.settings.with(|st| st.animations.enabled));
    let shown = Signal::derive(move || match tab.get() {
        Tab::Animations if !animations_on.get() => Tab::Layout,
        other => other,
    });
    Effect::new(move |_| {
        if !open.get() {
            return;
        }
        let h = window_event_listener_untyped("keydown", move |ev: web_sys::Event| {
            if let Ok(kev) = ev.dyn_into::<web_sys::KeyboardEvent>() {
                if kev.key() == "Escape" {
                    // A dropdown (or any dismissable surface) opened inside
                    // the modal owns this press: its own handler peels it,
                    // and closing the modal underneath it in the same
                    // keydown would take both layers down at once.
                    if crate::components::primitives::floating::dismiss::has_open_dismissable() {
                        return;
                    }
                    open.set(false);
                }
            }
        });
        on_cleanup(move || h.remove());
    });
    view! {
        <Show when=move || open.get()>
            <div
                class="fixed inset-0 z-[var(--z-popover)] flex items-center justify-center bg-black/45 p-4"
                on:click=move |_| open.set(false)
            >
                <div
                    class="flex flex-col rounded-2xl border border-line bg-surface shadow-2xl"
                    style=format!("width:{width};height:{height}")
                    on:click=move |ev| ev.stop_propagation()
                >
                    <div class="flex shrink-0 items-center gap-1 px-4 pb-2 pt-4">
                        <TabButton
                            tab=tab
                            active=shown
                            t=Tab::Layout
                            icon=IconName::Layout
                            label="Layout"
                        />
                        <TabButton
                            tab=tab
                            active=shown
                            t=Tab::Theme
                            icon=IconName::Palette
                            label="Theme"
                        />
                        <Show when=move || animations_on.get()>
                            <TabButton
                                tab=tab
                                active=shown
                                t=Tab::Animations
                                icon=IconName::Motion
                                label="Animations"
                            />
                        </Show>
                        <div class="ml-auto">
                            <IconButton
                                icon=IconName::Close
                                title="Close"
                                class="rounded-full bg-line/60 hover:bg-line".to_string()
                                on_click=move || open.set(false)
                            />
                        </div>
                    </div>
                    <div class="min-h-0 flex-1 overflow-y-auto px-4 pb-5">
                        {move || match shown.get() {
                            Tab::Layout => view! { <LayoutTab state=state /> }.into_any(),
                            Tab::Theme => view! { <ThemeTab state=state /> }.into_any(),
                            Tab::Animations => {
                                view! { <AnimationsTab state=state /> }.into_any()
                            }
                        }}
                    </div>
                </div>
            </div>
        </Show>
    }
}
