//! Texture picker + its two new controls: opacity and scale.
//!
//! The mode list is a compact grid rather than the old full-width rows — with
//! two sliders underneath, six stacked rows pushed everything else off-screen.
//! Opacity and scale are disabled (not hidden) when the texture is None, so
//! the controls stay in place and the panel does not resize as you click
//! around the list.

use leptos::prelude::*;

use crate::components::atoms::icon::{Icon, IconName};
use crate::components::atoms::slider::Slider;
use crate::core::appearance::TextureMode;
use crate::core::state::AppState;

#[component]
pub fn TextureSection(state: AppState) -> impl IntoView {
    let seed = state.settings.read_untracked().appearance;
    let (opacity, set_opacity) = signal(seed.texture_opacity as f64);
    let (tscale, set_tscale) = signal(seed.texture_scale as f64);

    Effect::new(move || {
        let a = state.settings.get().appearance;
        set_opacity.set(a.texture_opacity as f64);
        set_tscale.set(a.texture_scale as f64);
    });

    let current = move || state.settings.get().appearance.texture;
    let has_texture = move || current() != TextureMode::None;

    view! {
        <div class="grid grid-cols-3 gap-1">
            {TextureMode::all()
                .into_iter()
                .map(|mode| {
                    view! {
                        <button
                            type="button"
                            aria-pressed=move || (current() == mode).to_string()
                            on:click=move |_| {
                                state
                                    .settings
                                    .update(|s| {
                                        s.appearance.texture = mode;
                                        s.touch_appearance();
                                    })
                            }
                            class=move || {
                                if current() == mode {
                                    "flex items-center justify-center gap-1 rounded-md border border-accent bg-accent-soft px-1.5 py-1.5 text-[11px] font-medium text-accent"
                                } else {
                                    "flex items-center justify-center gap-1 rounded-md border border-line px-1.5 py-1.5 text-[11px] text-ink hover:bg-line"
                                }
                            }
                        >
                            {move || {
                                (current() == mode)
                                    .then(|| view! { <Icon name=IconName::Check size=11 /> })
                            }}
                            <span class="truncate">{mode.label()}</span>
                        </button>
                    }
                })
                .collect_view()}
        </div>

        // Sliders stay mounted but inert without a texture: hiding them would
        // make the popover jump in height every time the texture is toggled.
        <div
            class=move || {
                if has_texture() { "mt-3 space-y-3" } else { "mt-3 space-y-3 opacity-40" }
            }
            aria-disabled=move || (!has_texture()).to_string()
        >
            <Slider
                value=opacity
                min=0.0
                max=100.0
                step=1.0
                unit="%".to_string()
                on_change=move |v| {
                    let v = v.round().clamp(0.0, 100.0);
                    set_opacity.set(v);
                    state
                        .settings
                        .update(|s| {
                            s.appearance.texture_opacity = v as u8;
                            s.touch_appearance();
                        });
                }
                label="Texture opacity".to_string()
            />
            <Slider
                value=tscale
                min=25.0
                max=400.0
                step=5.0
                unit="%".to_string()
                on_change=move |v| {
                    let v = v.round().clamp(25.0, 400.0);
                    set_tscale.set(v);
                    state
                        .settings
                        .update(|s| {
                            s.appearance.texture_scale = v as u16;
                            s.touch_appearance();
                        });
                }
                label="Texture scale".to_string()
            />
        </div>
    }
}
