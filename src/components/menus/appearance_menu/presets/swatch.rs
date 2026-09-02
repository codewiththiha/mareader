//! One preset thumbnail: a miniature page with a couple of text-ish rules on
//! it, so tint and texture both have something to act on.
//!
//! Each thumbnail is a REAL miniature page — same `--canvas-filter`, same
//! texture classes, same grain — rendered by pinning that preset's appearance
//! as inline custom properties on the swatch (`Appearance::preview_style`).
//! Nothing here re-implements the look in miniature, so a swatch cannot drift
//! from what selecting it actually does.

use leptos::prelude::*;

use app_chrome::icon::{Icon, IconName};
use crate::effects::appearance::cancel_appearance_commit;
use crate::state::AppState;
use pdf_core::presets::{is_builtin, Preset};

#[component]
pub(super) fn PresetSwatch(preset: Preset, state: AppState) -> impl IntoView {
    let id = preset.id.clone();
    let id_for_click = id.clone();
    let name = preset.name.clone();
    let appearance = preset.appearance;
    let active = {
        let id = id.clone();
        move || state.settings.with(|s| s.active_preset.as_deref() == Some(id.as_str()))
    };
    let active_btn = active.clone();
    let name_title = name.clone();
    let deletable = !is_builtin(&id);
    let id_for_delete = id.clone();

    view! {
        <div class="group relative">
            <button
                type="button"
                title=name_title
                aria-pressed=move || active_btn().to_string()
                on:click={
                    let id = id_for_click.clone();
                    move |_| {
                        let id = id.clone();
                        // A preset is an explicit look: drop any in-flight
                        // slider commit so it cannot overwrite this a beat later.
                        cancel_appearance_commit();
                        state.settings.update(|s| s.apply_preset(&id));
                    }
                }
                class=move || {
                    if active() {
                        "flex w-full flex-col items-center gap-1 rounded-md border-2 border-accent p-1"
                    } else {
                        "flex w-full flex-col items-center gap-1 rounded-md border-2 border-transparent p-1 hover:border-line"
                    }
                }
            >
                // The swatch root carries the preset's whole appearance as
                // inline custom properties, so everything inside resolves
                // against THAT look rather than the applied one.
                <span
                    class="preset-swatch"
                    style=appearance.preview_style()
                    aria-hidden="true"
                >
                    <span class=appearance.preview_class()>
                        // Mirrors the real page structure: an unfiltered
                        // themed backdrop with a "canvas" on top. No inline
                        // filter/blend — the swatch uses solid colours
                        // (--ps-color-paper / --ps-color-ink) instead of
                        // GPU filter layers, so it is immune to compositor
                        // bugs during slider drags.
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
                                        .update(|s| {
                                            s.user_presets.retain(|p| p.id != id);
                                            if s.active_preset.as_deref() == Some(id.as_str()) {
                                                s.active_preset = None;
                                            }
                                        });
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

