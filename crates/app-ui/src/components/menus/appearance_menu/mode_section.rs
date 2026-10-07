//! Base mode and colour tint: the three-way base and hue plus strength.

use leptos::prelude::*;

use crate::appearance::{AppearanceScrub, ThemeHandle, ThemeScope};
use crate::components::menus::appearance_menu::hue_picker::HuePicker;
use crate::components::primitives::controls::toggle_button::ToggleButton;
use crate::components::primitives::form::slider::Slider;

/// Text tuning goes through the settings signal; the typography effect
/// paints it immediately.
fn update_text(state: ChromeState, f: impl FnOnce(&mut reader_core::settings::TextSettings)) {
    state.settings.update(|s| f(&mut s.text));
}
use app_chrome::icon::{Icon, IconName};
use app_state::ChromeState;
use reader_core::appearance::BaseMode;

fn base_icon(b: BaseMode) -> IconName {
    match b {
        BaseMode::Light => IconName::Sun,
        BaseMode::Dark => IconName::Moon,
        BaseMode::Dim => IconName::Dim,
    }
}

#[component]
pub fn BaseSection(state: ChromeState, theme: ThemeHandle) -> impl IntoView {
    // The dials show the HANDLE's look: the active pane's, or the window's.
    let seed = theme.look.read_untracked();
    let (hue, set_hue) = signal(seed.tint_hue as f64);
    let (strength, set_strength) = signal(seed.tint_strength as f64);

    // Mirror external writes back into the local signals.
    Effect::new(move || {
        let a = theme.look.get();
        set_hue.set(a.tint_hue as f64);
        set_strength.set(a.tint_strength as f64);
    });

    let current_base = move || theme.look.with(|a| a.base);

    view! {
        <div class="grid grid-cols-3 gap-1">
            {BaseMode::all()
                .iter()
                .copied()
                .map(|b| {
                    let selected = Signal::derive(move || current_base() == b);
                    view! {
                        <ToggleButton
                            active=selected
                            on_click=move || {
                                // Keep the hue the reader was just dialling,
                                // then switch family.
                                theme
                                    .commit
                                    .run((ThemeScope::Colour, Box::new(move |a| {
                                        a.base = b;
                                    })));
                            }
                            title=b.label()
                            variant_class="flex flex-col items-center gap-1 px-2 py-2 text-xs"
                        >
                            <Icon name=base_icon(b) size=16 />
                            <span>{b.label()}</span>
                        </ToggleButton>
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
                    // Hue at zero strength shows nothing; default it gently.
                    let mut st = strength.get_untracked().round().clamp(0.0, 100.0) as u8;
                    if st == 0 {
                        st = 18;
                        set_strength.set(18.0);
                    }
                    theme.scrub.run((
                        ThemeScope::Colour,
                        AppearanceScrub::Tint { hue: v as u16, strength: st },
                    ));
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
                    theme.scrub.run((
                        ThemeScope::Colour,
                        AppearanceScrub::Tint { hue, strength: v as u8 },
                    ));
                }
                label="Strength"
            />
        </div>

        // The reflowable contrast dial, under the tint; only text uses it.
        <Show when=move || state.reader.reflowable.get()>
            <div class="mt-3">
                <TextInkSlider state=state />
            </div>
        </Show>
    }
}

/// The ink-intensity slider, written through the shared typography path.
#[component]
fn TextInkSlider(state: ChromeState) -> impl IntoView {
    let (ink, set_ink) = signal(state.settings.with_untracked(|s| s.text.ink_contrast));
    // Mirror external writes back into the local signal.
    Effect::new(move || {
        set_ink.set(state.settings.with(|s| s.text.ink_contrast));
    });
    view! {
        <Slider
            value=ink
            min=10.0
            max=100.0
            step=1.0
            unit="%"
            on_change=move |v| {
                let v = v.round().clamp(10.0, 100.0);
                set_ink.set(v);
                update_text(state, move |t| t.ink_contrast = v);
            }
            label="Text ink intensity"
        />
    }
}
