//! The "save current look" form: name + optional section, with a datalist of
//! existing sections. Local editing state is a `signal()` pair — nothing
//! outside this component needs the `RwSignal` API.

use std::sync::Arc;

use leptos::prelude::*;

use crate::components::primitives::icon::{Icon, IconName};
use crate::effects::appearance::flush_appearance_commit;
use crate::state::AppState;
use pdf_core::presets::{make_preset_id, Preset};

#[component]
pub(super) fn PresetEditor(
    state: AppState,
    existing_groups: impl Fn() -> Vec<String> + Clone + Send + Sync + 'static,
) -> impl IntoView {
    let (saving, set_saving) = signal(false);
    let (new_name, set_new_name) = signal(String::new());
    let (new_group, set_new_group) = signal(String::new());

    // The provider closure rides in an Arc so the outer view closure stays
    // `Fn` (it only captures the Arc) while the datalist closure below can
    // clone it for its own reactive call.
    let existing_groups = Arc::new(existing_groups);

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
        set_new_name.set(String::new());
        set_new_group.set(String::new());
        set_saving.set(false);
    };

    view! {
        <Show
            when=move || saving.get()
            fallback=move || {
                view! {
                    <button
                        type="button"
                        on:click=move |_| set_saving.set(true)
                        class="mt-1 flex w-full items-center justify-center gap-1.5 rounded-md border border-dashed border-line px-2 py-1.5 text-xs text-muted hover:border-accent hover:text-accent"
                    >
                        <Icon name=IconName::Plus size=12 />
                        "Save current look"
                    </button>
                }
            }
        >
            <div class="mt-2 space-y-1.5 rounded-md border border-line p-2">
                <input
                    type="text"
                    placeholder="Preset name"
                    aria-label="Preset name"
                    autofocus
                    prop:value=move || new_name.get()
                    on:input=move |ev| set_new_name.set(event_target_value(&ev))
                    on:keydown=move |ev| {
                        if ev.key() == "Enter" {
                            commit();
                        } else if ev.key() == "Escape" {
                            set_saving.set(false);
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
                    on:input=move |ev| set_new_group.set(event_target_value(&ev))
                    on:keydown=move |ev| {
                        if ev.key() == "Enter" {
                            commit();
                        } else if ev.key() == "Escape" {
                            set_saving.set(false);
                        }
                    }
                    class="w-full rounded border border-line bg-paper px-2 py-1 text-xs text-ink focus:border-accent focus:outline-none"
                />
                <datalist id="preset-groups">
                    {{
                        let eg = existing_groups.clone();
                        move || {
                            eg()
                                .into_iter()
                                .map(|g| view! { <option value=g></option> })
                                .collect_view()
                        }
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
                        on:click=move |_| set_saving.set(false)
                        class="rounded border border-line px-2 py-1 text-xs text-muted hover:text-ink"
                    >
                        "Cancel"
                    </button>
                </div>
            </div>
        </Show>
    }
}
