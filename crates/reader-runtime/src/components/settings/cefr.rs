//! The Vocabulary tab: the level slider and the dataset's lifecycle.

use leptos::prelude::*;

use reader_core::settings::{CefrLevel, MIN_CEFR_BAND};

use crate::services;
use app_ui::components::primitives::controls::switch::Switch;
use app_ui::components::primitives::form::range_input::RangeInput;
use app_ui::components::primitives::menu::section_label::SectionLabel;

#[component]
pub(crate) fn CefrTab(state: crate::context::ReaderContext) -> impl IntoView {
    let s = state.settings;
    let enabled = Signal::derive(move || s.with(|st| st.cefr_enabled));
    let level = Signal::derive(move || s.with(|st| st.cefr_level));
    let click_explain = Signal::derive(move || s.with(|st| st.cefr_click_explain));
    let dataset = services::cefr::dataset();

    view! {
        <SectionLabel text="Vocabulary highlighter" />
        <div class="rounded-xl border border-line" data-setting="cefr">
            <div class="flex items-center justify-between gap-3 px-4 py-3.5">
                <span class="min-w-0">
                    <span class="block text-sm text-ink">"Mark words above my level"</span>
                    <span class="block text-xs text-muted">
                        "Red ink over every word harder than the band below, in PDF and text."
                    </span>
                </span>
                <Switch
                    checked=enabled
                    on_change=Callback::new(move |on: bool| {
                        s.update(move |st| st.cefr_enabled = on);
                    })
                    title="Vocabulary highlighter"
                />
            </div>
            <div class="px-4 pb-4" class=("opacity-45", move || !enabled.get())>
                <span class="flex items-baseline justify-between text-xs text-muted">
                    <span>"My level"</span>
                    <span class="text-sm font-medium tabular-nums text-ink">
                        {move || level.get().label()}
                    </span>
                </span>
                {move || {
                    // The input is a plain-bool prop; re-creating it on the
                    // toggle is the reactivity.
                    let off = !enabled.get();
                    view! {
                        <RangeInput
                            value=Signal::derive(move || level.get().band() as f64)
                            min=Signal::derive(|| MIN_CEFR_BAND as f64)
                            max=Signal::derive(|| 6.0)
                            step=Signal::derive(|| 1.0)
                            on_input=move |v: f64| {
                                s.update(move |st| {
                                    st.cefr_level = CefrLevel::from_band(v.round() as u8);
                                });
                            }
                            aria_label="Reading level"
                            disabled=off
                        />
                    }
                }}
                <p class="pt-1 text-xs text-muted">{move || level.get().detail()}</p>
            </div>
            <div class="flex items-center justify-between gap-3 border-t border-line px-4 py-3.5">
                <span class="min-w-0">
                    <span class="block text-sm text-ink">"Click a red word to explain it"</span>
                    <span class="block text-xs text-muted">
                        "Opens the AI word card for that word in its sentence."
                    </span>
                </span>
                <Switch
                    checked=click_explain
                    on_change=Callback::new(move |on: bool| {
                        s.update(move |st| st.cefr_click_explain = on);
                    })
                    disabled=Signal::derive(move || !enabled.get())
                    title="Click to explain"
                />
            </div>
        </div>

        <SectionLabel text="Word dataset" />
        <DatasetSection dataset=dataset />
        <p class="px-1 pt-2 text-xs text-muted">
            "The dataset is the Words-CEFR-Dataset, downloaded once (about 3 MB) and \
             rebuilt as a local database. Nothing leaves the device."
        </p>
    }
}

/// The dataset's lifecycle: download with progress, cancel, remove, retry.
#[component]
fn DatasetSection(dataset: RwSignal<Option<services::cefr::DatasetMirror>>) -> impl IntoView {
    let desktop = tauri_bridge::has_tauri();
    let button = "rounded-lg border border-line px-3 py-1.5 text-sm text-ink \
                  hover:bg-line/40 focus:outline-none focus-visible:ring-2 focus-visible:ring-accent";
    view! {
        <div class="rounded-xl border border-line px-4 py-4" data-setting="cefr-dataset">
            {move || {
                if !desktop {
                    return view! {
                        <p class="text-xs text-muted">
                            "The word dataset lives in the desktop app."
                        </p>
                    }
                        .into_any();
                }
                let mirror = dataset.get();
                let Some(mirror) = mirror else {
                    return view! { <p class="text-xs text-muted">"Checking the dataset…"</p> }
                        .into_any();
                };
                match mirror.phase.as_str() {
                    "ready" => {
                        let words = mirror
                            .words
                            .map(|w| format!("Ready — {w} words"))
                            .unwrap_or_else(|| "Ready".into());
                        view! {
                            <div class="flex items-center justify-between gap-3">
                                <span class="text-sm text-ink">{words}</span>
                                <button
                                    type="button"
                                    class=button
                                    on:click=move |_| services::cefr::request_remove()
                                >
                                    "Remove"
                                </button>
                            </div>
                        }
                            .into_any()
                    }
                    "downloading" => {
                        let percent = mirror
                            .percent()
                            .map(|p| format!("{p}%"))
                            .unwrap_or_else(|| "…".into());
                        let width = mirror
                            .percent()
                            .map(|p| format!("width:{p}%"))
                            .unwrap_or_else(|| "width:100%".into());
                        view! {
                            <div class="flex items-center justify-between gap-3 pb-2">
                                <span class="text-sm text-ink tabular-nums">{percent}</span>
                                <button
                                    type="button"
                                    class=button
                                    on:click=move |_| services::cefr::request_cancel()
                                >
                                    "Cancel"
                                </button>
                            </div>
                            <div class="h-1.5 w-full overflow-hidden rounded-full bg-line">
                                <div class="h-full rounded-full bg-accent" style=width></div>
                            </div>
                            <p class="pt-2 text-xs text-muted">
                                "The download resumes where it stopped if it breaks."
                            </p>
                        }
                            .into_any()
                    }
                    "converting" => view! {
                        <p class="text-sm text-ink">"Building the local dataset…"</p>
                    }
                        .into_any(),
                    "failed" => {
                        let message = mirror.message.clone().unwrap_or_else(|| "failed".into());
                        view! {
                            <div class="flex items-center justify-between gap-3 pb-1">
                                <span class="text-sm text-ink">"The dataset failed"</span>
                                <button
                                    type="button"
                                    class=button
                                    on:click=move |_| services::cefr::request_download()
                                >
                                    "Retry"
                                </button>
                            </div>
                            <p class="text-xs text-muted">{message}</p>
                        }
                            .into_any()
                    }
                    _ => view! {
                        <div class="flex items-center justify-between gap-3">
                            <span class="min-w-0">
                                <span class="block text-sm text-ink">"Not downloaded"</span>
                                <span class="block text-xs text-muted">
                                    "Word levels for the highlighter, one small file."
                                </span>
                            </span>
                            <button
                                type="button"
                                class=button
                                on:click=move |_| services::cefr::request_download()
                            >
                                "Download"
                            </button>
                        </div>
                    }
                        .into_any(),
                }
            }}
        </div>
    }
}
