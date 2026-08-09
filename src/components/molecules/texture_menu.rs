//! Paper texture menu (none/paper/lined/grid/dotted/cross). OWNED BY branch D
//! (panels/settings).
//!
//! A dropdown listing the six `TextureMode` variants. Clicking an entry sets
//! `settings.texture`; the page host applies the matching `texture-{name}` class
//! on `.pdf-page`. The popover closes on selection and whenever any settings
//! change, so only one menu is ever open at a time.

use leptos::prelude::*;

use crate::components::atoms::icon::{Icon, IconName};
use crate::core::settings::TextureMode;
use crate::core::state::AppState;

#[component]
pub fn TextureMenu(state: AppState) -> impl IntoView {
    let open = RwSignal::new(false);

    let current = move || state.settings.get().texture;

    let trigger_class = move || {
        let base = "inline-flex items-center justify-center gap-1.5 rounded-lg border h-9 px-2.5 text-sm font-medium transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent border-line bg-surface text-ink hover:bg-line";
        if open.get() {
            format!("{base} border-accent text-accent")
        } else {
            base.to_string()
        }
    };

    // Close the popover whenever settings change (a selection here, or any other
    // menu changing theme/texture/noise) so only one menu is open at a time.
    Effect::new(move || {
        let _ = state.settings.get();
        open.set(false);
    });

    view! {
        <div class="relative inline-flex">
            <button
                type="button"
                title="Texture"
                on:click=move |_| open.set(!open.get())
                class=trigger_class
            >
                {move || view! { <Icon name=IconName::Texture size=16/> }}
                <svg class="text-muted" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                    <path d="m6 9 6 6 6-6"/>
                </svg>
            </button>
            <Show when=move || open.get()>
                <div class="absolute right-0 top-full z-50 mt-1 w-48 rounded-lg border border-line bg-surface p-1 shadow-lg">
                    <For
                        each=move || TextureMode::all().to_vec()
                        key=|m| m.as_str()
                        children=move |mode| {
                            view! {
                                <button
                                    type="button"
                                    on:click=move |_| {
                                        state.settings.update(|s| s.texture = mode);
                                        open.set(false);
                                    }
                                    class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-sm text-ink hover:bg-line"
                                >
                                    <span class="inline-flex w-4 shrink-0 justify-center text-accent">
                                        {move || (current() == mode).then(|| view! { <Icon name=IconName::Check size=14/> })}
                                    </span>
                                    <span>{mode.label()}</span>
                                </button>
                            }
                        }
                    />
                </div>
            </Show>
        </div>
    }
}
