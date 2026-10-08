//! The Vocabulary tab: the level slider and the dataset's lifecycle.

use leptos::prelude::*;

use reader_core::settings::{CefrLevel, MIN_CEFR_BAND};

use crate::services;
use crate::services::cefr::DatasetPhase;
use app_ui::components::primitives::controls::switch::Switch;
use app_ui::components::primitives::form::range_input::RangeInput;
use app_ui::components::primitives::form::row::Row;
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
        <div class="divide-y divide-line rounded-xl border border-line" data-setting="cefr">
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
            <div>
                <Row label="My level">
                    <span
                        class="flex w-40 flex-col gap-1"
                        class=("opacity-45", move || !enabled.get())
                        data-setting="cefr-level"
                    >
                        {move || {
                            // `disabled` is a plain bool on the input, so the
                            // toggle re-creates the control.
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
                        <span class="text-right text-xs tabular-nums text-muted">
                            {move || level.get().label()}
                        </span>
                    </span>
                </Row>
                <p class="px-4 pb-4 text-xs text-muted">{move || level.get().detail()}</p>
            </div>
            <div class="flex items-center justify-between gap-3 px-4 py-3.5">
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
            "The Words-CEFR-Dataset (about 3 MB) and the POS model (about 6.7 MB), \
             downloaded once and rebuilt into a local database. Every download resumes, \
             and nothing leaves the device."
        </p>
    }
}

/// The panel's buttons, shared so a row added later matches the others.
const BUTTON: &str = "rounded-lg border border-line px-3 py-1.5 text-sm text-ink \
                      hover:bg-line/40 focus:outline-none focus-visible:ring-2 \
                      focus-visible:ring-accent";

/// The POS model's own line: an absent or paused model asks for itself.
fn model_note(model: Option<DatasetPhase>) -> Option<AnyView> {
    match model {
        Some(DatasetPhase::Ready) => None,
        Some(DatasetPhase::Downloading) | Some(DatasetPhase::Converting) => Some(
            view! {
                <p class="pt-2 text-xs text-muted">
                    "The POS model is still arriving; click-to-explain waits for it."
                </p>
            }
            .into_any(),
        ),
        Some(DatasetPhase::Paused) => Some(
            view! {
                <div class="flex items-center justify-between gap-3 pt-2">
                    <span class="min-w-0 text-xs text-muted">
                        "The POS model's download paused; click-to-explain waits for it."
                    </span>
                    <button
                        type="button"
                        class=BUTTON
                        on:click=move |_| services::cefr::request_model_download()
                    >
                        "Resume"
                    </button>
                </div>
            }
            .into_any(),
        ),
        _ => Some(
            view! {
                <div class="flex items-center justify-between gap-3 pt-2">
                    <span class="min-w-0 text-xs text-muted">
                        "The POS model is not downloaded; click-to-explain does without it."
                    </span>
                    <button
                        type="button"
                        class=BUTTON
                        on:click=move |_| services::cefr::request_model_download()
                    >
                        "Download"
                    </button>
                </div>
            }
            .into_any(),
        ),
    }
}

/// The dataset's lifecycle: download with progress, cancel, remove, retry.
#[component]
fn DatasetSection(dataset: RwSignal<Option<services::cefr::DatasetMirror>>) -> impl IntoView {
    let desktop = tauri_bridge::has_tauri();
    let button = BUTTON;
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
                match mirror.phase {
                    DatasetPhase::Ready => {
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
                            {model_note(mirror.model)}
                        }
                            .into_any()
                    }
                    DatasetPhase::Downloading => {
                        // A size nobody has declared yet is not a full bar.
                        let percent = mirror.percent();
                        let label = percent
                            .map(|p| format!("{p}%"))
                            .unwrap_or_else(|| "…".into());
                        let width = format!("width:{}%", percent.unwrap_or(0));
                        view! {
                            <div class="flex items-center justify-between gap-3 pb-2">
                                <span class="text-sm text-ink tabular-nums">{label}</span>
                                <span class="flex gap-2">
                                    <button
                                        type="button"
                                        class=button
                                        on:click=move |_| services::cefr::request_pause()
                                    >
                                        "Pause"
                                    </button>
                                    <button
                                        type="button"
                                        class=button
                                        on:click=move |_| services::cefr::request_cancel()
                                    >
                                        "Cancel"
                                    </button>
                                </span>
                            </div>
                            <div class="h-1.5 w-full overflow-hidden rounded-full bg-line">
                                <div
                                    class="h-full rounded-full bg-accent/80 \
                                           transition-[width] duration-100 ease-out"
                                    style=width
                                ></div>
                            </div>
                            <p class="pt-2 text-xs text-muted">
                                "The download resumes where it stopped if it breaks."
                            </p>
                        }
                            .into_any()
                    }
                    DatasetPhase::Paused => {
                        let percent = mirror
                            .percent()
                            .map(|p| format!("{p}%"))
                            .unwrap_or_else(|| "…".into());
                        view! {
                            <div class="flex items-center justify-between gap-3">
                                <span class="text-sm text-ink tabular-nums">
                                    {format!("Paused at {percent}")}
                                </span>
                                <span class="flex gap-2">
                                    <button
                                        type="button"
                                        class=button
                                        on:click=move |_| services::cefr::request_resume()
                                    >
                                        "Resume"
                                    </button>
                                    <button
                                        type="button"
                                        class=button
                                        on:click=move |_| services::cefr::request_cancel()
                                    >
                                        "Cancel"
                                    </button>
                                </span>
                            </div>
                        }
                            .into_any()
                    }
                    DatasetPhase::Converting => view! {
                        <p class="text-sm text-ink">"Building the local dataset…"</p>
                    }
                        .into_any(),
                    DatasetPhase::Failed => {
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
                    DatasetPhase::Absent => view! {
                        <div class="flex items-center justify-between gap-3">
                            <span class="min-w-0">
                                <span class="block text-sm text-ink">"Not downloaded"</span>
                                <span class="block text-xs text-muted">
                                    "Two small files: the word levels and the POS model."
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
