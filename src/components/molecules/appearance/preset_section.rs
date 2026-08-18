//! Preset gallery: grouped rows of look thumbnails, plus save/delete.
//!
//! Each thumbnail is a REAL miniature page — same `--canvas-filter`, same
//! texture classes, same grain — rendered by pinning that preset's appearance
//! as inline custom properties on the swatch (`Appearance::preview_style`).
//! Nothing here re-implements the look in miniature, so a swatch cannot drift
//! from what selecting it actually does.

use leptos::prelude::*;

use pdf_viewer::components::atoms::icon::{Icon, IconName};
use pdf_core::presets::{group_presets, is_builtin, make_preset_id, user_group_names, Preset};
use crate::core::state::AppState;
use crate::effects::theme_applier::{cancel_appearance_commit, flush_appearance_commit};

/// One preset thumbnail: a miniature page with a couple of text-ish rules on
/// it, so tint and texture both have something to act on.
#[component]
fn PresetSwatch(preset: Preset, state: AppState) -> impl IntoView {
    let id = preset.id.clone();
    let id_for_click = id.clone();
    let name = preset.name.clone();
    let appearance = preset.appearance;
    let active = {
        let id = id.clone();
        move || state.settings.get().active_preset.as_deref() == Some(id.as_str())
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

#[component]
pub fn PresetSection(state: AppState) -> impl IntoView {
    let saving = RwSignal::new(false);
    let new_name = RwSignal::new(String::new());
    let new_group = RwSignal::new(String::new());

    let groups = move || group_presets(&state.settings.get().all_presets());
    let existing_groups = move || user_group_names(&state.settings.get().user_presets);

    let commit = move || {
        let name = new_name.get_untracked().trim().to_string();
        if name.is_empty() {
            return;
        }
        let group = new_group.get_untracked().trim().to_string();
        // Persist any in-flight slider so we save what the reader sees.
        flush_appearance_commit();
        state.settings.update(|s| {
            let id = make_preset_id(&name, &s.user_presets);
            s.user_presets.push(Preset {
                id: id.clone(),
                name,
                group,
                appearance: s.appearance,
            });
            // Saving selects what you just saved — otherwise the gallery would
            // show the new preset as inactive while you are literally looking
            // at its look.
            s.active_preset = Some(id);
        });
        new_name.set(String::new());
        new_group.set(String::new());
        saving.set(false);
    };

    view! {
        <For
            each=groups
            key=|g| format!("{}-{}", g.name, g.presets.len())
            children=move |g| {
                view! {
                    <div class="mb-2">
                        <div class="mb-1 text-[10px] font-semibold uppercase tracking-wide text-muted">
                            {g.name.clone()}
                        </div>
                        <div class="grid grid-cols-4 gap-1">
                            {g
                                .presets
                                .iter()
                                .map(|p| view! { <PresetSwatch preset=p.clone() state=state /> })
                                .collect_view()}
                        </div>
                    </div>
                }
            }
        />

        {move || {
            if saving.get() {
                view! {
                    <div class="mt-2 space-y-1.5 rounded-md border border-line p-2">
                        <input
                            type="text"
                            placeholder="Preset name"
                            aria-label="Preset name"
                            autofocus
                            prop:value=move || new_name.get()
                            on:input=move |ev| new_name.set(event_target_value(&ev))
                            on:keydown=move |ev| {
                                if ev.key() == "Enter" {
                                    commit();
                                } else if ev.key() == "Escape" {
                                    saving.set(false);
                                }
                            }
                            class="w-full rounded border border-line bg-paper px-2 py-1 text-xs text-ink focus:border-accent focus:outline-none"
                        />
                        // Free text WITH a datalist: users can type a brand new
                        // section or pick one they already made, without two
                        // separate controls for "new" and "existing".
                        <input
                            type="text"
                            list="preset-groups"
                            placeholder="Section (optional)"
                            aria-label="Preset section"
                            prop:value=move || new_group.get()
                            on:input=move |ev| new_group.set(event_target_value(&ev))
                            on:keydown=move |ev| {
                                if ev.key() == "Enter" {
                                    commit();
                                } else if ev.key() == "Escape" {
                                    saving.set(false);
                                }
                            }
                            class="w-full rounded border border-line bg-paper px-2 py-1 text-xs text-ink focus:border-accent focus:outline-none"
                        />
                        <datalist id="preset-groups">
                            {move || {
                                existing_groups()
                                    .into_iter()
                                    .map(|g| view! { <option value=g></option> })
                                    .collect_view()
                            }}
                        </datalist>
                        <div class="flex gap-1">
                            <button
                                type="button"
                                on:click=move |_| commit()
                                class="flex-1 rounded border border-accent bg-accent-soft px-2 py-1 text-xs font-medium text-accent"
                            >
                                "Save"
                            </button>
                            <button
                                type="button"
                                on:click=move |_| saving.set(false)
                                class="rounded border border-line px-2 py-1 text-xs text-muted hover:text-ink"
                            >
                                "Cancel"
                            </button>
                        </div>
                    </div>
                }
                    .into_any()
            } else {
                view! {
                    <button
                        type="button"
                        on:click=move |_| saving.set(true)
                        class="mt-1 flex w-full items-center justify-center gap-1.5 rounded-md border border-dashed border-line px-2 py-1.5 text-xs text-muted hover:border-accent hover:text-accent"
                    >
                        <Icon name=IconName::Plus size=12 />
                        "Save current look"
                    </button>
                }
                    .into_any()
            }
        }}
    }
}
