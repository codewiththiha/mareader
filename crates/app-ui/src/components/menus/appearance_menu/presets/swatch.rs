//! One preset thumbnail: a real miniature page, so it cannot drift.

use leptos::prelude::*;

use crate::appearance::{ThemeHandle, ThemeScope, cancel_appearance_commit};
use app_chrome::icon::{Icon, IconName};
use app_state::ChromeState;
use reader_core::appearance::presets::{Preset, is_builtin};

#[component]
pub(super) fn PresetSwatch(
    preset: Preset,
    state: ChromeState,
    theme: ThemeHandle,
) -> impl IntoView {
    let id = preset.id.clone();
    let name = preset.name.clone();
    let appearance = preset.appearance;
    // The highlight means "the edited look IS this preset".
    let active = move || theme.look.with(|a| *a == appearance);
    let active_btn = active;
    let name_title = name.clone();
    let deletable = !is_builtin(&id);
    let id_for_delete = id.clone();

    view! {
        <div class="group relative">
            <button
                type="button"
                title=name_title
                aria-pressed=move || active_btn().to_string()
                on:click=move |_| {
                    // A preset is an explicit look: drop any in-flight commit.
                    cancel_appearance_commit();
                    theme.commit.run((ThemeScope::Colour, Box::new(move |a| {
                        *a = appearance;
                    })));
                }
                class=move || {
                    if active() {
                        "flex w-full flex-col items-center gap-1 rounded-md border-2 border-accent p-1"
                    } else {
                        "flex w-full flex-col items-center gap-1 rounded-md border-2 border-transparent p-1 hover:border-line"
                    }
                }
            >
                // The swatch carries the preset as inline properties.
                <span
                    class="preset-swatch"
                    style=move || {
                        if state.reader.reflowable.get() {
                            appearance.text_preview_style()
                        } else {
                            appearance.preview_style()
                        }
                    }
                    aria-hidden="true"
                >
                    <span class=appearance.preview_class()>
                        // Mirrors a real page: solid colours, no GPU layers.
                        <span class="preset-canvas">
                            <span class="preset-line preset-line-a"></span>
                            <span class="preset-line preset-line-b"></span>
                            <span class="preset-line preset-line-c"></span>
                        </span>
                    </span>
                </span>
                <span class="w-full truncate text-center text-[10px] leading-tight text-ink">
                    {name}
                </span>
            </button>
            {deletable
                .then(|| {
                    view! {
                        <button
                            type="button"
                            title="Delete preset"
                            aria-label="Delete preset"
                            on:click={
                                let id = id_for_delete.clone();
                                move |ev: leptos::ev::MouseEvent| {
                                    ev.stop_propagation();
                                    let id = id.clone();
                                    state
                                        .settings
                                        .update(|s| s.user_presets.retain(|p| p.id != id));
                                }
                            }
                            class="absolute right-0 top-0 hidden h-5 w-5 items-center justify-center rounded-full border border-line bg-surface text-muted hover:text-ink group-hover:flex"
                        >
                            <Icon name=IconName::Close size=10 />
                        </button>
                    }
                })}
        </div>
    }
}
