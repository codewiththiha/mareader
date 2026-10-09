//! The Vocabulary tab: the level slider and the dataset's lifecycle.

use leptos::prelude::*;

use reader_core::settings::{CefrLevel, MIN_CEFR_BAND};

use crate::services;
use app_ui::components::primitives::controls::switch::Switch;
use app_ui::components::primitives::form::range_input::RangeInput;
use app_ui::components::primitives::menu::section_label::SectionLabel;

/// One action's look, shared by every button on this tab.
const BUTTON: &str = "rounded-lg border border-line px-3 py-1.5 text-sm text-ink \
                      hover:bg-line/40 focus:outline-none focus-visible:ring-2 \
                      focus-visible:ring-accent disabled:cursor-not-allowed disabled:opacity-45";

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
                    disabled=Signal::derive(move || !enabled.get())
                />
                <p class="pt-1 text-xs text-muted">{move || level.get().detail()}</p>
            </div>
            <div class="flex items-center justify-between gap-3 border-t border-line px-4 py-3.5">
                <span class="min-w-0">
                    <span class="block text-sm text-ink">"Click a red word to explain it"</span>
                    <span class="block text-xs text-muted">
                        "Opens the AI word card for that word in its sentence. Off, the ink is \
                         paint alone and a selection runs straight through it."
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
            "Two files from the Words-CEFR-Dataset: the word levels, about 3 MB, and the \
             word-role model a clicked word is read with, about 6 MB. Both are downloaded \
             once, kept on this device, and the levels are rebuilt into a local database."
        </p>
    }
}

/// One labelled action; every button on this tab is this.
fn action(label: &'static str, run: fn()) -> impl IntoView {
    view! {
        <button type="button" class=BUTTON on:click=move |_| run()>
            {label}
        </button>
    }
}

/// The filled share of a download bar.
fn bar(percent: Option<u32>) -> impl IntoView {
    let width = move || match percent {
        Some(percent) => format!("width:{percent}%"),
        None => "width:100%".to_string(),
    };
    view! {
        <div class="h-1.5 w-full overflow-hidden rounded-full bg-line">
            <div class="h-full rounded-full bg-accent" style=width></div>
        </div>
    }
}

/// The readout above a bar: a percent, and the rate it is moving at.
fn readout(percent: Option<u32>, rate: Option<String>) -> impl IntoView {
    view! {
        <span class="flex items-baseline gap-2 text-sm text-ink tabular-nums">
            <span>{move || percent.map(|p| format!("{p}%")).unwrap_or_else(|| "…".into())}</span>
            <span class="text-xs text-muted">{rate.unwrap_or_default()}</span>
        </span>
    }
}

/// The dataset's lifecycle: download with progress, pause, resume, remove.
#[component]
fn DatasetSection(dataset: RwSignal<Option<services::cefr::DatasetMirror>>) -> impl IntoView {
    let desktop = tauri_bridge::has_tauri();
    view! {
        <div class="rounded-xl border border-line px-4 py-4" data-setting="cefr-dataset">
            {move || {
                if !desktop {
                    return view! {
                        <p class="text-xs text-muted">"The word dataset lives in the desktop app."</p>
                    }
                        .into_any();
                }
                let Some(mirror) = dataset.get() else {
                    return view! { <p class="text-xs text-muted">"Checking the dataset…"</p> }
                        .into_any();
                };
                let body = match mirror.phase.as_str() {
                    "ready" => ready_row(mirror.words).into_any(),
                    "downloading" => downloading_row(&mirror).into_any(),
                    "paused" => paused_row(&mirror).into_any(),
                    "converting" => view! {
                        <p class="text-sm text-ink">"Building the local dataset…"</p>
                    }
                        .into_any(),
                    "failed" => failed_row(mirror.message.clone()).into_any(),
                    _ => absent_row().into_any(),
                };
                view! {
                    {body}
                    <TaggerRow phase=mirror.tagger.clone() percent=mirror.tagger_percent />
                }
                    .into_any()
            }}
        </div>
    }
}

fn ready_row(words: Option<u64>) -> impl IntoView {
    let label = words
        .map(|words| format!("Ready — {words} words"))
        .unwrap_or_else(|| "Ready".into());
    view! {
        <div class="flex items-center justify-between gap-3">
            <span class="text-sm text-ink">{label}</span>
            {action("Remove", services::cefr::request_remove)}
        </div>
    }
}

fn downloading_row(mirror: &services::cefr::DatasetMirror) -> impl IntoView {
    let (percent, rate) = (mirror.percent(), mirror.rate());
    view! {
        <div class="pb-2">
            <div class="flex items-center justify-between gap-3 pb-2">
                {readout(percent, rate)}
                <span class="flex gap-2">
                    {action("Pause", services::cefr::request_pause)}
                    {action("Cancel", services::cefr::request_cancel)}
                </span>
            </div>
            {bar(percent)}
        </div>
        <p class="pt-2 text-xs text-muted">
            "The download resumes where it stopped if it breaks, and a blocked host hands \
             the file to the next one."
        </p>
    }
}

fn paused_row(mirror: &services::cefr::DatasetMirror) -> impl IntoView {
    let percent = mirror.percent();
    view! {
        <div class="pb-2">
            <div class="flex items-center justify-between gap-3 pb-2">
                <span class="text-sm text-ink tabular-nums">
                    {move || match percent {
                        Some(percent) => format!("Paused at {percent}%"),
                        None => "Paused".to_string(),
                    }}
                </span>
                <span class="flex gap-2">
                    {action("Resume", services::cefr::request_resume)}
                    {action("Cancel", services::cefr::request_cancel)}
                </span>
            </div>
            {bar(percent)}
        </div>
    }
}

fn failed_row(message: Option<String>) -> impl IntoView {
    view! {
        <div class="flex items-center justify-between gap-3 pb-1">
            <span class="text-sm text-ink">"The dataset failed"</span>
            {action("Retry", services::cefr::request_download)}
        </div>
        <p class="text-xs text-muted">{message.unwrap_or_else(|| "failed".into())}</p>
    }
}

fn absent_row() -> impl IntoView {
    view! {
        <div class="flex items-center justify-between gap-3">
            <span class="min-w-0">
                <span class="block text-sm text-ink">"Not downloaded"</span>
                <span class="block text-xs text-muted">
                    "Word levels for the highlighter, one small file."
                </span>
            </span>
            {action("Download", services::cefr::request_download)}
        </div>
    }
}

/// The word-role model's own line: a click's POS needs it, the ink does not.
#[component]
fn TaggerRow(phase: String, percent: Option<u32>) -> impl IntoView {
    let (label, note) = match phase.as_str() {
        "ready" => (
            "Word roles ready",
            "A clicked red word is read in its sentence.".to_string(),
        ),
        "downloading" => (
            "Word roles downloading",
            percent
                .map(|percent| format!("{percent}% — needed to read a clicked word's role."))
                .unwrap_or_else(|| "Needed to read a clicked word's role.".to_string()),
        ),
        "failed" => (
            "Word roles failed",
            "Levels still work; a clicked word shows no role.".to_string(),
        ),
        _ => (
            "Word roles not downloaded",
            "Comes with the dataset; a clicked word shows no role until it lands.".to_string(),
        ),
    };
    view! {
        <div class="mt-3 flex items-baseline justify-between gap-3 border-t border-line pt-3">
            <span class="min-w-0">
                <span class="block text-xs text-ink">{label}</span>
                <span class="block text-xs text-muted">{note}</span>
            </span>
        </div>
    }
}
