//! Film-grain section renderer — one section of the consolidated 🎨
//! Appearance menu.
//!
//! Renders a `Toggle` (on/off) and a `Slider` (0..=100 intensity). Both write
//! straight to `settings`; the foundation `theme_applier` effect reflects them
//! on the DOM (`body.noise-enabled` + `--noise-opacity`).
//!
//! The atoms expect `ReadSignal<bool>` / `ReadSignal<f64>` props, so we keep two
//! local signals seeded from settings and mirror every change back into
//! `settings.noise_enabled` / `settings.noise_intensity`.
//!
//! This is content only — no trigger button, no popover, no open/close state.
//! The owning `AppearanceMenu` owns dismissal and deliberately does NOT close on
//! noise changes so the slider stays usable mid-drag. Until the U7 toolbar
//! rewrite lands, the Phase-2 toolbar still mounts this bare (no popover
//! wrapper) — that transient render is expected to look wrong and is
//! compile-only.

use leptos::prelude::*;

use crate::components::atoms::icon::IconName;
use crate::components::atoms::slider::Slider;
use crate::components::atoms::toggle::Toggle;
use crate::core::state::AppState;

/// Icon for the old per-feature noise toolbar trigger; the consolidated 🎨
/// Appearance trigger uses a single `Palette` icon instead. Kept alive (allow)
/// so the sprite entry stays for future UI.
#[allow(dead_code)]
fn noise_icon() -> IconName {
    IconName::Noise
}

#[component]
pub fn NoiseToggle(state: AppState) -> impl IntoView {
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

    view! {
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
    }
}
