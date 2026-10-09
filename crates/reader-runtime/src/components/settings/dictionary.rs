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
                        "A red word's senses pop up under it, pointing at the word. \
                     Clicking still explains."
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
                    let pair = format!("{}-{}", pack.source, pack.target);
                    let label = pack.label.clone();
                    let fallback = pack.clone();
                    let id_for_row = id.clone();
                    // The row's truth, read again on every redraw.
                    let current = move || {
                        packs
                            .get()
                            .into_iter()
                            .find(|p| p.id == id_for_row)
                            .unwrap_or_else(|| fallback.clone())
                    };
                    let current_for_status = current.clone();
                    let current_for_actions = current.clone();
                    let id_for_actions = id.clone();
                    let pair_for_actions = pair.clone();
                    view! {
                        <div class="flex items-center gap-3 border-t border-line px-4 py-3 first:border-t-0">
                            <span class="min-w-0 flex-1">
                                <span class="block truncate text-sm text-ink">{label}</span>
                                <span class="block text-xs text-muted">
                                    {move || status_line(&current_for_status())}
                                </span>
                            </span>
                            {move || {
                                let row = current_for_actions();
                                match row.phase() {
                                    "ready" => {
                                        let id = id_for_actions.clone();
                                        let pair = pair_for_actions.clone();
                                        view! {
                                            <button
                                                type="button"
                                                class=BUTTON
                                                on:click=move |_| {
                                                    services::dict::request_pack(&id, "remove");
                                                    let id = id.clone();
                                                    let pair = pair.clone();
                                                    s.update(move |st| {
                                                        st.dict.langs
                                                            .retain(|entry| entry != &id && entry != &pair);
                                                    });
                                                }
                                            >
                                                "Remove"
                                            </button>
                                        }
                                        .into_any()
                                    }
                                    "downloading" => {
                                        let pause_id = id_for_actions.clone();
                                        let cancel_id = id_for_actions.clone();
                                        view! {
                                            <button
                                                type="button"
                                                class=BUTTON
                                                on:click=move |_| {
                                                    services::dict::request_pack(&pause_id, "pause")
                                                }
                                            >
                                                "Pause"
                                            </button>
                                            <button
                                                type="button"
                                                class=BUTTON
                                                on:click=move |_| {
                                                    services::dict::request_pack(&cancel_id, "cancel")
                                                }
                                            >
                                                "Cancel"
                                            </button>
                                        }
                                        .into_any()
                                    }
                                    "paused" => {
                                        let resume_id = id_for_actions.clone();
                                        let cancel_id = id_for_actions.clone();
                                        view! {
                                            <button
                                                type="button"
                                                class=BUTTON
                                                on:click=move |_| {
                                                    services::dict::request_pack(&resume_id, "resume")
                                                }
                                            >
                                                "Resume"
                                            </button>
                                            <button
                                                type="button"
                                                class=BUTTON
                                                on:click=move |_| {
                                                    services::dict::request_pack(&cancel_id, "cancel")
                                                }
                                            >
                                                "Cancel"
                                            </button>
                                        }
                                        .into_any()
                                    }
                                    "converting" => view! {
                                        <button type="button" class=BUTTON disabled=true>
                                            "Converting…"
                                        </button>
                                    }
                                    .into_any(),
                                    _ => {
                                        let download_id = id_for_actions.clone();
                                        let why = row.message.clone().unwrap_or_default();
                                        view! {
                                            <button
                                                type="button"
                                                class=BUTTON
                                                title=why
                                                on:click=move |_| {
                                                    services::dict::request_pack(&download_id, "download")
                                                }
                                            >
                                                "Download"
                                            </button>
                                        }
                                        .into_any()
                                    }
                                }
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

/// The readout under a pack's name.
fn status_line(pack: &services::dict::PackMirror) -> String {
    match pack.phase() {
        "ready" => format!("{} words — ready", pack.rows),
        "converting" => "converting to database…".to_string(),
        "downloading" => match pack.percent() {
            Some(percent) => format!("downloading — {percent}%"),
            None => "downloading".to_string(),
        },
        "paused" => "paused".to_string(),
        "failed" => pack
            .message
            .clone()
            .unwrap_or_else(|| "download failed".to_string()),
        _ => format!("{} words — not downloaded", pack.rows),
    }
}
