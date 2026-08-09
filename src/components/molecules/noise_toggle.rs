//! Film-grain noise toggle + intensity. OWNED BY branch D (panels/settings).
//!
//! The popover holds a `Toggle` (on/off) and a `Slider` (0..=100 intensity).
//! Both write straight to `settings`; the foundation `theme_applier` effect
//! reflects them on the DOM (`body.noise-enabled` + `--noise-opacity`).
//!
//! The atoms expect `ReadSignal<bool>` / `ReadSignal<f64>` props, so we keep two
//! local signals seeded from settings and mirror every change back into
//! `settings.noise_enabled` / `settings.noise_intensity`.

use std::cell::RefCell;
use std::rc::Rc;

use leptos::prelude::*;

use crate::components::atoms::icon::{Icon, IconName};
use crate::components::atoms::slider::Slider;
use crate::components::atoms::toggle::Toggle;
use crate::core::settings::TextureMode;
use crate::core::state::AppState;

#[component]
pub fn NoiseToggle(state: AppState) -> impl IntoView {
    let open = RwSignal::new(false);

    let settings_now = move || state.settings.get();
    // Seed the local mirrors from the persisted settings. read_untracked avoids
    // the "signal read outside a tracking context" warning: seeding at setup is
    // intentionally one-shot; the Effect below keeps the mirrors in sync.
    let seed = state.settings.read_untracked();
    let (enabled, set_enabled) = signal(seed.noise_enabled);
    let (intensity, set_intensity) = signal(seed.noise_intensity as f64);

    // Keep the local mirrors in sync if settings are written from elsewhere.
    Effect::new(move || {
        let s = settings_now();
        set_enabled.set(s.noise_enabled);
        set_intensity.set(s.noise_intensity as f64);
    });

    // Close the popover when another menu (theme/texture) changes its setting,
    // but NOT when noise settings change — the slider must stay usable mid-drag.
    let prev = Rc::new(RefCell::new((String::new(), TextureMode::None)));
    Effect::new(move || {
        let s = settings_now();
        let mut p = prev.borrow_mut();
        if p.0 != s.theme_id || p.1 != s.texture {
            p.0 = s.theme_id.clone();
            p.1 = s.texture;
            open.set(false);
        }
    });

    let trigger_class = move || {
        let base = "inline-flex items-center justify-center gap-1.5 rounded-lg border h-9 px-2.5 text-sm font-medium transition-colors focus:outline-none focus-visible:ring-2 focus-visible:ring-accent border-line bg-surface text-ink hover:bg-line";
        let active = if settings_now().noise_enabled || open.get() {
            " border-accent text-accent"
        } else {
            ""
        };
        format!("{base}{active}")
    };

    view! {
        <div class="relative inline-flex">
            <button
                type="button"
                title="Noise"
                on:click=move |_| open.set(!open.get())
                class=trigger_class
            >
                {move || view! { <Icon name=IconName::Noise size=16/> }}
                <svg class="text-muted" width="12" height="12" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
                    <path d="m6 9 6 6 6-6"/>
                </svg>
            </button>
            <Show when=move || open.get()>
                <div class="menu-popover absolute right-0 top-full z-50 mt-1 w-56 rounded-lg border border-line bg-surface p-3 shadow-lg">
                    <div class="flex items-center justify-between gap-3">
                        <span class="text-sm text-ink">"Film grain"</span>
                        <Toggle
                            checked=enabled
                            on_change=move |v| {
                                set_enabled.set(v);
                                state.settings.update(|s| s.noise_enabled = v);
                            }
                        />
                    </div>
                    <div class="mt-3">
                        <Slider
                            value=intensity
                            min=0.0
                            max=100.0
                            step=1.0
                            on_change=move |v| {
                                let v = v.round().clamp(0.0, 100.0);
                                set_intensity.set(v);
                                state.settings.update(|s| s.noise_intensity = v as u8);
                            }
                            label="Intensity".to_string()
                        />
                    </div>
                </div>
            </Show>
        </div>
    }
}
