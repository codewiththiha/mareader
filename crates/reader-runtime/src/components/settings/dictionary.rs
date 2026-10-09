//! The Dictionary tab: the hover switch and the language packs.

use leptos::prelude::*;

use crate::services;
use app_ui::components::primitives::controls::switch::Switch;
use app_ui::components::primitives::menu::section_label::SectionLabel;

/// One action's look, shared by every button on this tab.
const BUTTON: &str = "rounded-lg border border-line px-3 py-1.5 text-sm text-ink \
                      hover:bg-line/40 focus:outline-none focus-visible:ring-2 \
                      focus-visible:ring-accent disabled:cursor-not-allowed disabled:opacity-45";

#[component]
pub(crate) fn DictionaryTab(state: crate::context::ReaderContext) -> impl IntoView {
    let s = state.settings;
    let hover = Signal::derive(move || s.with(|st| st.dict.hover));
    let packs = services::dict::packs();

    view! {
        <SectionLabel text="Dictionary" />
        <div class="rounded-xl border border-line" data-setting="dict">
            <div class="flex items-center justify-between gap-3 px-4 py-3.5">
                <span class="min-w-0">
                    <span class="block text-sm text-ink">"Show meanings on hover"</span>
                    <span class="block text-xs text-muted">
                        "A red word's senses pop up beside it. Clicking still explains."
                    </span>
                </span>
                <Switch
                    checked=hover
                    on_change=Callback::new(move |on: bool| {
                        s.update(move |st| st.dict.hover = on);
                    })
                    title="Dictionary hover"
                />
            </div>
        </div>

        <SectionLabel text="Language packs" />
        <div class="rounded-xl border border-line" data-setting="dict-packs">
            <For
                each=move || packs.get()
                key=|pack: &services::dict::PackMirror| pack.id.clone()
                children=move |pack: services::dict::PackMirror| {
                    let id = pack.id.clone();
                    let include = Signal::derive(move || {
                        let langs = s.with(|st| st.dict.langs.clone());
                        langs.is_empty() || langs.contains(&id)
                    });
                    let id_for_toggle = pack.id.clone();
                    let id_download = pack.id.clone();
                    let id_status = pack.id.clone();
                    let id_remove = pack.id.clone();
                    let phase = pack.phase();
                    let built = pack.built;
                    view! {
                        <div class="flex items-center gap-3 border-t border-line px-4 py-3 first:border-t-0">
                            <input
                                type="checkbox"
                                class="size-4 accent-[var(--color-accent)]"
                                attr:aria-label=format!("Include {} in the dictionary", pack.label)
                                prop:checked=move || include.get()
                                disabled=!built
                                on:change=move |ev: web_sys::Event| {
                                    use wasm_bindgen::JsCast;
                                    let on = ev
                                        .target()
                                        .and_then(|t| {
                                            t.dyn_ref::<web_sys::HtmlInputElement>()
                                                .map(|input| input.checked())
                                        })
                                        .unwrap_or(false);
                                    let id = id_for_toggle.clone();
                                    s.update(move |st| {
                                        let mut langs = st.dict.langs.clone();
                                        if langs.is_empty() {
                                            // Empty means all: materialize the
                                            // set before changing one seat.
                                            langs = services::dict::packs()
                                                .get()
                                                .iter()
                                                .filter(|p| p.built)
                                                .map(|p| p.id.clone())
                                                .collect();
                                        }
                                        langs.retain(|entry| entry != &id);
                                        if on {
                                            langs.push(id);
                                        }
                                        st.dict.langs = langs;
                                    });
                                }
                            />
                            <span class="min-w-0 flex-1">
                                <span class="block truncate text-sm text-ink">{pack.label.clone()}</span>
                                <span class="block text-xs text-muted">
                                    {move || {
                                        let row = packs
                                            .get()
                                            .into_iter()
                                            .find(|p| p.id == id_status)
                                            .unwrap_or(pack.clone());
                                        status_line(&row)
                                    }}
                                </span>
                            </span>
                            {if built {
                                view! {
                                    <button
                                        type="button"
                                        class=BUTTON
                                        on:click=move |_| {
                                            services::dict::request_pack(&id_remove, "remove");
                                            let id = id_remove.clone();
                                            s.update(move |st| {
                                                st.dict.langs.retain(|entry| entry != &id);
                                            });
                                        }
                                    >
                                        "Remove"
                                    </button>
                                }
                                .into_any()
                            } else {
                                view! {
                                    <PackActions
                                        id=id_download.clone()
                                        phase=phase
                                    />
                                }
                                .into_any()
                            }}
                        </div>
                    }
                }
            />
        </div>
        <p class="pt-2 text-xs text-muted">
            "Packs convert to a local database. With two packs, words can cross \
             through English when none joins the pair directly."
        </p>
    }
}

/// A pack's lifecycle buttons: download, or pause/resume/cancel.
#[component]
fn PackActions(id: String, phase: &'static str) -> impl IntoView {
    let download_id = id.clone();
    let pause_id = id.clone();
    let resume_id = id.clone();
    let cancel_id = id.clone();
    view! {
        {match phase {
            "downloading" => view! {
                <button
                    type="button"
                    class=BUTTON
                    on:click=move |_| services::dict::request_pack(&pause_id, "pause")
                >
                    "Pause"
                </button>
                <button
                    type="button"
                    class=BUTTON
                    on:click=move |_| services::dict::request_pack(&cancel_id, "cancel")
                >
                    "Cancel"
                </button>
            }
            .into_any(),
            "paused" => view! {
                <button
                    type="button"
                    class=BUTTON
                    on:click=move |_| services::dict::request_pack(&resume_id, "resume")
                >
                    "Resume"
                </button>
                <button
                    type="button"
                    class=BUTTON
                    on:click=move |_| services::dict::request_pack(&cancel_id, "cancel")
                >
                    "Cancel"
                </button>
            }
            .into_any(),
            _ => view! {
                <button
                    type="button"
                    class=BUTTON
                    on:click=move |_| services::dict::request_pack(&download_id, "download")
                >
                    "Download"
                </button>
            }
            .into_any(),
        }}
    }
}

/// The readout under a pack's name.
fn status_line(pack: &services::dict::PackMirror) -> String {
    match pack.phase() {
        "ready" => format!("{} words — ready", pack.rows),
        "downloading" => match pack.percent() {
            Some(percent) => format!("downloading — {percent}%"),
            None => "downloading".to_string(),
        },
        "paused" => "paused".to_string(),
        "failed" => "download failed".to_string(),
        _ => format!("{} words — not downloaded", pack.rows),
    }
}
