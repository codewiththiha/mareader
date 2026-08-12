//! Applies the persisted appearance to the DOM whenever it changes.
//!
//! Base mode -> `<html data-base="light|dark|dim">` (+ `.dark` class).
//! Tint      -> computed `--canvas-filter` / `--canvas-blend` + seven UI token
//!              overrides, all written as inline custom properties on `<html>`.
//! Texture   -> `--texture-opacity` / `--texture-scale-user`.
//! Noise     -> `body.noise-enabled` (+ `.noise-animated`) and `--noise-opacity`.
//!
//! WHY INLINE PROPERTIES RATHER THAN CSS BLOCKS. The old design had one
//! `:root[data-theme=...]` block per theme, so every look needed hand-written
//! CSS and only the six that existed were reachable. The tint is now continuous
//! — any hue, any strength — which cannot be enumerated in a stylesheet. The
//! stylesheet keeps the STRUCTURE (which var drives what) and the base
//! palettes; the computed values are pushed here. Setting a property to the
//! empty string removes the override and lets the stylesheet's own value win
//! again, which is how a tint is cleanly un-applied.

use leptos::prelude::*;
use web_sys::wasm_bindgen::JsCast;

use crate::core::state::AppState;
use crate::util::storage::save_settings;

/// The seven tokens the tint may override. Listed once so they can be cleared
/// as a set — a stale override left behind when the tint is removed would keep
/// tinting the UI with no way for the user to see why.
const UI_TOKENS: [&str; 7] = [
    "--color-paper",
    "--color-surface",
    "--color-line",
    "--color-ink",
    "--color-muted",
    "--color-accent",
    "--color-accent-soft",
];

fn document_element() -> Option<web_sys::Element> {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.document_element())
}

fn html_style() -> Option<web_sys::CssStyleDeclaration> {
    document_element()
        .and_then(|e| e.dyn_into::<web_sys::HtmlElement>().ok())
        .map(|h| h.style())
}

pub fn theme_applier(state: AppState) {
    // --- base mode + computed tint -------------------------------------------
    Effect::new(move || {
        let a = state.settings.get().appearance;
        let Some(el) = document_element() else { return };

        let _ = el.set_attribute("data-base", a.base.as_str());
        let class = el.class_list();
        if a.base.is_dark() {
            let _ = class.add_1("dark");
        } else {
            let _ = class.remove_1("dark");
        }

        let Some(style) = html_style() else { return };
        let _ = style.set_property("--canvas-filter", &a.canvas_filter());
        let _ = style.set_property("--canvas-blend", a.canvas_blend());

        // Clear first, then re-apply: switching from a tinted preset to an
        // untinted one has to actually remove the overrides.
        for tok in UI_TOKENS {
            let _ = style.remove_property(tok);
        }
        for (name, value) in a.ui_overrides() {
            let _ = style.set_property(name, &value);
        }
    });

    // --- texture opacity + scale ---------------------------------------------
    Effect::new(move || {
        let a = state.settings.get().appearance;
        let Some(style) = html_style() else { return };
        let _ = style.set_property(
            "--texture-opacity",
            &format!("{:.3}", a.texture_opacity as f64 / 100.0),
        );
        // A multiplier on the page's own scale, NOT an absolute pitch: the
        // texture must still track zoom, so the user
        // control scales the natural pitch rather than replacing it.
        let _ = style.set_property(
            "--texture-scale-user",
            &format!("{:.3}", a.texture_scale as f64 / 100.0),
        );
    });

    // --- noise ---------------------------------------------------------------
    Effect::new(move || {
        let a = state.settings.get().appearance;
        let opacity = (a.noise_intensity.min(100) as f64) / 100.0;
        let Some(body) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.body())
        else {
            return;
        };
        let class = body.class_list();
        if a.noise.is_on() {
            let _ = class.add_1("noise-enabled");
        } else {
            let _ = class.remove_1("noise-enabled");
        }
        // The animation is a separate class so toggling intensity does not
        // restart it, and so a static grain costs no compositing work at all.
        if matches!(a.noise, crate::core::appearance::NoiseMode::Animated) {
            let _ = class.add_1("noise-animated");
        } else {
            let _ = class.remove_1("noise-animated");
        }
        if let Some(style) = body.dyn_ref::<web_sys::HtmlElement>().map(|h| h.style()) {
            let _ = style.set_property("--noise-opacity", &format!("{opacity}"));
        }
    });

    // Persist on change.
    Effect::new(move || {
        let settings = state.settings.get();
        save_settings(&settings);
    });
}
