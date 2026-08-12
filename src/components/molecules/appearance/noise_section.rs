//! Film grain: Off / Static / Animated, plus intensity.
//!
//! This replaced a boolean toggle. "Animated" is a third mode rather than a
//! second checkbox because "animated but off" is not a meaningful state, and a
//! 3-way choice makes that unrepresentable instead of merely discouraged.

use leptos::prelude::*;

use crate::components::atoms::slider::Slider;
use crate::core::appearance::NoiseMode;
use crate::core::state::AppState;
use crate::effects::theme_applier::{
    flush_appearance_commit, preview_appearance, AppearanceScrub,
};

#[component]
pub fn NoiseSection(state: AppState) -> impl IntoView {
    let seed = state.settings.read_untracked().appearance;
    let (intensity, set_intensity) = signal(seed.noise_intensity as f64);

    Effect::new(move || {
        let a = state.settings.get().appearance;
        set_intensity.set(a.noise_intensity as f64);
    });

    let current = move || state.settings.get().appearance.noise;

    view! {
        <div class="grid grid-cols-3 gap-1">
            {NoiseMode::all()
                .into_iter()
                .map(|m| {
                    view! {
                        <button
                            type="button"
                            aria-pressed=move || (current() == m).to_string()
                            on:click=move |_| {
                                flush_appearance_commit();
                                state
                                    .settings
                                    .update(|s| {
                                        s.appearance.noise = m;
                                        // Turning grain on at 0% shows nothing
                                        // and reads as a dead control.
                                        if m.is_on() && s.appearance.noise_intensity == 0 {
                                            s.appearance.noise_intensity = 25;
                                        }
                                        s.touch_appearance();
                                    })
                            }
                            class=move || {
                                if current() == m {
                                    "rounded-md border border-accent bg-accent-soft px-2 py-1.5 text-xs font-medium text-accent"
                                } else {
                                    "rounded-md border border-line px-2 py-1.5 text-xs text-ink hover:bg-line"
                                }
                            }
                        >
                            {m.label()}
                        </button>
                    }
                })
                .collect_view()}
        </div>
        <div
            class=move || {
                if current().is_on() { "mt-3" } else { "mt-3 opacity-40" }
            }
        >
            <Slider
                value=intensity
                min=0.0
                max=100.0
                step=1.0
                unit="%".to_string()
                on_change=move |v| {
                    let v = v.round().clamp(0.0, 100.0);
                    set_intensity.set(v);
                    preview_appearance(state, AppearanceScrub::NoiseIntensity(v as u8));
                }
                label="Grain intensity".to_string()
            />
        </div>
    }
}
