//! Eye-protection theme menu (light/dark/sepia/green/night/dim). OWNED BY branch D
//! (panels/settings).
//!
//! A dropdown listing every theme from `crate::core::themes::THEMES`. Clicking an
//! entry sets `settings.theme_id`; the foundation `theme_applier` effect pushes it
//! to the DOM (`<html data-theme=...>` + `.dark`). The popover closes on selection
//! and whenever any settings change, so only one menu is ever open at a time.

use leptos::prelude::*;

use crate::components::atoms::icon::{Icon, IconName};
use crate::core::state::AppState;
use crate::core::themes::{theme_by_id, THEMES};

/// Maps a theme id to its toolbar icon (matches the ids in `THEMES` / CSS).
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
    let open = RwSignal::new(false);

    let current = move || state.settings.get().theme_id;
    let current_icon = move || theme_icon(theme_by_id(&current()).id);

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
                title="Theme"
                on:click=move |_| open.set(!open.get())
                class=trigger_class
            >
                {move || view! { <Icon name=current_icon() size=16/> }}
                <svg class="text-muted" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                    <path d="m6 9 6 6 6-6"/>
                </svg>
            </button>
            <Show when=move || open.get()>
                <div class="absolute right-0 top-full z-50 mt-1 w-48 rounded-lg border border-line bg-surface p-1 shadow-lg">
                    <For
                        each=move || theme_rows()
                        key=|t| t.id
                        children=move |t| {
                            let t = *t;
                            view! {
                                <button
                                    type="button"
                                    on:click=move |_| {
                                        state.settings.update(|s| s.theme_id = t.id.to_string());
                                        open.set(false);
                                    }
                                    class="flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-sm text-ink hover:bg-line"
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
                </div>
            </Show>
        </div>
    }
}
