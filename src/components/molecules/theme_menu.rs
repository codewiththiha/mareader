//! Theme section renderer — one section of the consolidated 🎨 Appearance menu.
//!
//! Renders the theme rows (Light/Dark/Sepia/Green/Night/Dim from
//! `crate::core::themes::THEMES`) with check-on-active, a color dot, and the
//! "dark" badge. Clicking a row sets `settings.theme_id`; the foundation
//! `theme_applier` effect pushes it to the DOM (`<html data-theme=...>` + `.dark`).
//!
//! This is content only — no trigger button, no popover, no open state. The
//! owning `AppearanceMenu` watches `theme_id` and closes itself on selection.
//! Until the U7 toolbar rewrite lands, the Phase-2 toolbar still mounts this
//! bare (no popover wrapper) — that transient render is expected to look wrong
//! and is compile-only.

use leptos::prelude::*;

use crate::components::atoms::icon::{Icon, IconName};
use crate::core::state::AppState;
use crate::core::themes::THEMES;

/// Maps a theme id to its toolbar icon (matches the ids in `THEMES` / CSS).
///
/// Used by the old per-theme toolbar trigger; the consolidated 🎨 Appearance
/// trigger uses a single `Palette` icon instead. Kept alive (allow) so the
/// theme icon set stays in the sprite and the mapping is documented.
#[allow(dead_code)]
fn theme_icon(id: &str) -> IconName {
    match id {
        "dark" => IconName::Moon,
        "sepia" => IconName::Sepia,
        "green" => IconName::Green,
        "night" => IconName::Night,
        "dim" => IconName::Dim,
        _ => IconName::Sun,
    }
}

fn theme_rows() -> Vec<&'static crate::core::themes::ThemeDefinition> {
    THEMES.iter().collect()
}

/// Small color-dot class: dark themes get an ink-colored dot, light ones paper.
fn dot_class(is_dark: bool) -> &'static str {
    if is_dark {
        "inline-block h-3 w-3 shrink-0 rounded-full border border-line bg-ink"
    } else {
        "inline-block h-3 w-3 shrink-0 rounded-full border border-line bg-paper"
    }
}

#[component]
pub fn ThemeMenu(state: AppState) -> impl IntoView {
    let current = move || state.settings.get().theme_id;

    view! {
        <For
            each=move || theme_rows()
            key=|t| t.id
            children=move |t| {
                let t = *t;
                view! {
                    <button
                        type="button"
                        on:click=move |_| state.settings.update(|s| s.theme_id = t.id.to_string())
                        class=move || {
                            if current() == t.id {
                                "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-sm font-medium bg-accent-soft text-accent"
                            } else {
                                "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-sm text-ink hover:bg-line"
                            }
                        }
                    >
                        <span class="inline-flex w-4 shrink-0 justify-center text-accent">
                            {move || (current() == t.id).then(|| view! { <Icon name=IconName::Check size=14/> })}
                        </span>
                        <span class=dot_class(t.is_dark) />
                        <span>{t.label}</span>
                        {t.is_dark.then(|| view! {
                            <span class="ml-auto rounded bg-line px-1 text-[10px] text-muted">"dark"</span>
                        })}
                    </button>
                }
            }
        />
    }
}
