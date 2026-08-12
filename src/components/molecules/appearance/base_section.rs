//! Base mode + colour tint — the two controls that replaced the six fixed
//! themes.
//!
//! The base is a 3-way segmented choice (Light / Dark / Dim) because they are
//! mutually exclusive structural options, not a list that will grow. The tint
//! below it is hue + strength, and it is deliberately shown even at strength 0
//! rather than hidden behind a "enable tint" toggle: a control that appears
//! and disappears is harder to find than one that is simply at zero.

use leptos::prelude::*;

use crate::components::atoms::hue_picker::HuePicker;
use crate::components::atoms::icon::{Icon, IconName};
use crate::components::atoms::slider::Slider;
use crate::core::appearance::BaseMode;
use crate::core::state::AppState;
use crate::effects::theme_applier::{
    flush_appearance_commit, preview_appearance, AppearanceScrub,
};

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
        let a = state.settings.get().appearance;
        set_hue.set(a.tint_hue as f64);
        set_strength.set(a.tint_strength as f64);
    });

    let current_base = move || state.settings.get().appearance.base;

    view! {
        <div class="grid grid-cols-3 gap-1">
            {BaseMode::all()
                .into_iter()
                .map(|b| {
                    view! {
                        <button
                            type="button"
                            title=b.label()
                            aria-pressed=move || (current_base() == b).to_string()
                            on:click=move |_| {
                                // Keep the hue the reader was just dialling,
                                // then switch family.
                                flush_appearance_commit();
                                state
                                    .settings
                                    .update(|s| {
                                        s.appearance.base = b;
                                        s.touch_appearance();
                                    })
                            }
                            class=move || {
                                if current_base() == b {
                                    "flex flex-col items-center gap-1 rounded-md border border-accent bg-accent-soft px-2 py-2 text-xs font-medium text-accent"
                                } else {
                                    "flex flex-col items-center gap-1 rounded-md border border-line px-2 py-2 text-xs text-ink hover:bg-line"
                                }
                            }
                        >
                            <Icon name=base_icon(b) size=16 />
                            <span>{b.label()}</span>
                        </button>
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
                        state,
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
                unit="%".to_string()
                on_change=move |v| {
                    let v = v.round().clamp(0.0, 100.0);
                    set_strength.set(v);
                    // Live hue signal, not Settings: a hue drag may not have
                    // committed yet.
                    let hue = hue.get_untracked().round().clamp(0.0, 359.0) as u16;
                    preview_appearance(
                        state,
                        AppearanceScrub::Tint { hue, strength: v as u8 },
                    );
                }
                label="Tint strength".to_string()
            />
        </div>
    }
}
