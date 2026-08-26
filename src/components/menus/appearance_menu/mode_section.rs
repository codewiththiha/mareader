//! Base mode + colour tint — the two controls that replaced the six fixed
//! themes.
//!
//! The base is a 3-way segmented choice (Light / Dark / Dim) because they are
//! mutually exclusive structural options, not a list that will grow. The tint
//! below it is hue + strength, and it is deliberately shown even at strength 0
//! rather than hidden behind a "enable tint" toggle: a control that appears
//! and disappears is harder to find than one that is simply at zero.

use leptos::prelude::*;

use crate::components::menus::appearance_menu::hue_picker::HuePicker;
use crate::components::primitives::icon::{Icon, IconName};
use crate::components::primitives::form::slider::Slider;
use pdf_core::appearance::BaseMode;
use crate::components::primitives::option_button::OptionButton;
use crate::state::AppState;
use crate::effects::appearance::{preview_appearance, AppearanceScrub};

fn base_icon(b: BaseMode) -> IconName {
    match b {
        BaseMode::Light => IconName::Sun,
        BaseMode::Dark => IconName::Moon,
        BaseMode::Dim => IconName::Dim,
    }
}

#[component]
pub fn BaseSection(state: AppState) -> impl IntoView {
    let seed = state.settings.read_untracked().appearance;
    let (hue, set_hue) = signal(seed.tint_hue as f64);
    let (strength, set_strength) = signal(seed.tint_strength as f64);

    // Mirror external writes (applying a preset) back into the local signals,
    // or the sliders would keep showing the old look's numbers.
    Effect::new(move || {
        let a = state.settings.with(|s| s.appearance);
        set_hue.set(a.tint_hue as f64);
        set_strength.set(a.tint_strength as f64);
    });

    let current_base = move || state.settings.with(|s| s.appearance.base);

    view! {
        <div class="grid grid-cols-3 gap-1">
            {BaseMode::all()
                .into_iter()
                .map(|b| {
                    let selected = Signal::derive(move || current_base() == b);
                    view! {
                        <OptionButton
                            selected=selected
                            on_click=move || {
                                // Keep the hue the reader was just dialling,
                                // then switch family.
                                super::update_appearance(state, move |s| {
                                    s.appearance.base = b;
                                })
                            }
                            title=b.label()
                            variant_class="flex flex-col items-center gap-1 px-2 py-2 text-xs"
                        >
                            <Icon name=base_icon(b) size=16 />
                            <span>{b.label()}</span>
                        </OptionButton>
                    }
                })
                .collect_view()}
        </div>

        <div class="mt-3">
            <HuePicker
                hue=hue
                on_change=move |v| {
                    let v = v.round().clamp(0.0, 359.0);
                    set_hue.set(v);
                    // Dragging the hue with no strength shows nothing, which
                    // reads as a broken control. Give it a visible-but-gentle
                    // default so the choice lands — locally AND in the scrub,
                    // because Settings is not written until the drag pauses.
                    let mut st = strength.get_untracked().round().clamp(0.0, 100.0) as u8;
                    if st == 0 {
                        st = 35;
                        set_strength.set(35.0);
                    }
                    preview_appearance(
                        state.settings,
                        AppearanceScrub::Tint { hue: v as u16, strength: st },
                    );
                }
            />
        </div>

        <div class="mt-3">
            <Slider
                value=strength
                min=0.0
                max=100.0
                step=1.0
                unit="%"
                on_change=move |v| {
                    let v = v.round().clamp(0.0, 100.0);
                    set_strength.set(v);
                    // Live hue signal, not Settings: a hue drag may not have
                    // committed yet.
                    let hue = hue.get_untracked().round().clamp(0.0, 359.0) as u16;
                    preview_appearance(
                        state.settings,
                        AppearanceScrub::Tint { hue, strength: v as u8 },
                    );
                }
                label="Tint strength"
            />
        </div>
    }
}
