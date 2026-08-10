//! Page-texture section renderer — one section of the consolidated 🎨
//! Appearance menu.
//!
//! Renders the six `TextureMode` rows (none/paper/lined/grid/dotted/cross) with
//! check-on-active. Clicking a row sets `settings.texture`; the page host
//! applies the matching `texture-{name}` class on `.pdf-page`.
//!
//! This is content only — no trigger button, no popover, no open state. The
//! owning `AppearanceMenu` deliberately keeps the popover open on texture
//! selection (only theme selection and outside-click/Escape dismiss it). Until
//! the U7 toolbar rewrite lands, the Phase-2 toolbar still mounts this bare (no
//! popover wrapper) — that transient render is expected to look wrong and is
//! compile-only.

use leptos::prelude::*;

use crate::components::atoms::icon::{Icon, IconName};
use crate::core::settings::TextureMode;
use crate::core::state::AppState;

/// Icon for the old per-feature texture toolbar trigger; the consolidated 🎨
/// Appearance trigger uses a single `Palette` icon instead. Kept alive (allow)
/// so the sprite entry stays for future UI.
#[allow(dead_code)]
fn texture_icon(_mode: TextureMode) -> IconName {
    IconName::Texture
}

#[component]
pub fn TextureMenu(state: AppState) -> impl IntoView {
    let current = move || state.settings.get().texture;

    view! {
        <For
            each=move || TextureMode::all().to_vec()
            key=|m| m.as_str()
            children=move |mode| {
                view! {
                    <button
                        type="button"
                        on:click=move |_| state.settings.update(|s| s.texture = mode)
                        class=move || {
                            if current() == mode {
                                "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-sm font-medium bg-accent-soft text-accent"
                            } else {
                                "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-sm text-ink hover:bg-line"
                            }
                        }
                    >
                        <span class="inline-flex w-4 shrink-0 justify-center text-accent">
                            {move || (current() == mode).then(|| view! { <Icon name=IconName::Check size=14/> })}
                        </span>
                        <span>{mode.label()}</span>
                    </button>
                }
            }
        />
    }
}
