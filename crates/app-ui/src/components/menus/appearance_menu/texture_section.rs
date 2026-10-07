//! Texture picker + its two new controls: opacity and scale.
//!
//! The mode list is a compact grid rather than the old full-width rows — with
//! two sliders underneath, six stacked rows pushed everything else off-screen.
//! Opacity and scale are disabled (not hidden) when the texture is None, so
//! the controls stay in place and the panel does not resize as you click
//! around the list.
//!
//! The whole section is scoped `Texture` (see [`crate::appearance`]): while a
//! per-pane mode is in effect it edits the focused pane's own texture, and the
//! modes are the reader's choice — independent themes carry the texture family
//! with the colour, and `independent_textures` carries the family on its own
//! for a reader who wants a pattern per pane and a colour for all.

use leptos::prelude::*;

use crate::appearance::{AppearanceScrub, ThemeHandle, ThemeScope};
use crate::components::primitives::controls::toggle_button::ToggleButton;
use crate::components::primitives::form::slider::Slider;
use app_chrome::icon::{Icon, IconName};
use reader_core::appearance::TextureMode;

#[component]
pub fn TextureSection(theme: ThemeHandle) -> impl IntoView {
    let seed = theme.look.read_untracked();
    let (opacity, set_opacity) = signal(seed.texture_opacity as f64);
    let (tscale, set_tscale) = signal(seed.texture_scale as f64);

    Effect::new(move || {
        let a = theme.look.get();
        set_opacity.set(a.texture_opacity as f64);
        set_tscale.set(a.texture_scale as f64);
    });

    let current = move || theme.look.with(|a| a.texture);
    let has_texture = move || current() != TextureMode::None;

    view! {
        <div class="grid grid-cols-3 gap-1">
            {TextureMode::all()
                .iter()
                .copied()
                .map(|mode| {
                    let selected = Signal::derive(move || current() == mode);
                    view! {
                        <ToggleButton
                            active=selected
                            on_click=move || {
                                theme
                                    .commit
                                    .run((ThemeScope::Texture, Box::new(move |a| {
                                        a.texture = mode;
                                    })));
                            }
                            variant_class="flex items-center justify-center gap-1 px-1.5 py-1.5 text-[11px]"
                        >
                            {move || {
                                (current() == mode)
                                    .then(|| view! { <Icon name=IconName::Check size=11 /> })
                            }}
                            <span class="truncate">{mode.label()}</span>
                        </ToggleButton>
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
                unit="%"
                on_change=move |v| {
                    if current() == TextureMode::None {
                        return;
                    }
                    let v = v.round().clamp(0.0, 100.0);
                    set_opacity.set(v);
                    theme
                        .scrub
                        .run((ThemeScope::Texture, AppearanceScrub::TextureOpacity(v as u8)));
                }
                label="Texture opacity"
            />
            <Slider
                value=tscale
                min=25.0
                max=400.0
                step=5.0
                unit="%"
                on_change=move |v| {
                    if current() == TextureMode::None {
                        return;
                    }
                    let v = v.round().clamp(25.0, 400.0);
                    set_tscale.set(v);
                    theme
                        .scrub
                        .run((ThemeScope::Texture, AppearanceScrub::TextureScale(v as u16)));
                }
                label="Texture scale"
            />
        </div>
    }
}

