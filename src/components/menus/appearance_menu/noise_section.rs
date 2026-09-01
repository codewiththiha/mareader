//! Film grain: Off / Static / Animated, plus intensity.
//!
//! This replaced a boolean toggle. "Animated" is a third mode rather than a
//! second checkbox because "animated but off" is not a meaningful state, and a
//! 3-way choice makes that unrepresentable instead of merely discouraged.

use leptos::prelude::*;

use crate::components::primitives::form::slider::Slider;
use pdf_core::appearance::NoiseMode;
use crate::components::primitives::option_button::OptionButton;
use crate::state::AppState;
use crate::effects::appearance::{preview_appearance, AppearanceScrub};

#[component]
pub fn NoiseSection(state: AppState) -> impl IntoView {
    let seed = state.settings.read_untracked().appearance;
    let (intensity, set_intensity) = signal(seed.noise_intensity as f64);

    Effect::new(move || {
        let a = state.settings.with(|s| s.appearance);
        set_intensity.set(a.noise_intensity as f64);
    });

    let current = move || state.settings.with(|s| s.appearance.noise);

    view! {
        <div class="grid grid-cols-3 gap-1">
            {NoiseMode::all()
                .iter()
                .copied()
                .map(|m| {
                    let selected = Signal::derive(move || current() == m);
                    view! {
                        <OptionButton
                            selected=selected
                            on_click=move || {
                                super::update_appearance(state, move |s| {
                                    s.appearance.noise = m;
                                    // Turning grain on at 0% shows nothing
                                    // and reads as a dead control.
                                    if m.is_on() && s.appearance.noise_intensity == 0 {
                                        s.appearance.noise_intensity = 25;
                                    }
                                })
                            }
                            variant_class="px-2 py-1.5 text-xs"
                        >
                            {m.label()}
                        </OptionButton>
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
                unit="%"
                on_change=move |v| {
                    let v = v.round().clamp(0.0, 100.0);
                    set_intensity.set(v);
                    preview_appearance(state.settings, AppearanceScrub::NoiseIntensity(v as u8));
                }
                label="Grain intensity"
            />
        </div>
    }
}
