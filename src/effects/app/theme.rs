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
//!
//! SLIDER RAM. Writing `settings` on every `input` event made
//! WKWebView allocate a fresh filter intermediate for every visible page,
//! every tick — that is the 1.2GB spike while dragging Colour / Tint
//! strength. Sliders now live-paint CSS at most once per animation frame and
//! commit the Settings signal (and localStorage) only after the gesture
//! pauses. The filter STRING is unchanged, so the look is byte-identical.
//!
//! The scrub/commit scheduler for the sliders lives in the sibling
//! `appearance` module; this file keeps the painting itself and the
//! two app effects.

use leptos::prelude::*;
use web_sys::wasm_bindgen::JsCast;

use pdf_core::appearance::Appearance;
use crate::state::AppState;

use crate::effects::appearance::schedule_save;

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

fn body_el() -> Option<web_sys::HtmlElement> {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.body())
        .and_then(|b| b.dyn_into::<web_sys::HtmlElement>().ok())
}

/// Write every appearance CSS custom property / class from `a`. Synchronous.
/// The filter string is the same one `Appearance::canvas_filter` already
/// produces — this does not invent a second pipeline.
pub fn paint_appearance_now(a: Appearance) {
    let Some(el) = document_element() else { return };

    let prev_base = el.get_attribute("data-base");
    // Only a Light/Dark/Dim swap needs the glass layer rebuilt. Slider
    // ticks must not (appendix 19), and a same-base tint is already live
    // on `--color-*` — `.toolbar-glass:has(.menu-popover)` drops the
    // stale backdrop while the picker is open.
    let kick = prev_base.as_deref() != Some(a.base.as_str());

    _ = el.set_attribute("data-base", a.base.as_str());
    // Publish the texture too: in blend mode the backdrop bleeds the same
    // paper texture past the page body (styles/components/shell.css), and
    // the CSS keys off this attribute.
    let _ = el.set_attribute("data-texture", a.texture.as_str());
    let class = el.class_list();
    if a.base.is_dark() {
        _ = class.add_1("dark");
    } else {
        _ = class.remove_1("dark");
    }
    if kick {
        // Kill color transitions for this frame so toolbar buttons cannot
        // linger at a mid-mix of the old and new tokens.
        _ = class.add_1("theme-switching");
    }

    if let Some(style) = html_style() {
        let _ = style.set_property(
            "color-scheme",
            if a.base.is_dark() { "dark" } else { "light" },
        );
        _ = style.set_property("--canvas-filter", &a.canvas_filter());
        _ = style.set_property("--canvas-blend", a.canvas_blend());
        for tok in UI_TOKENS {
            _ = style.remove_property(tok);
        }
        for (name, value) in a.ui_overrides() {
            _ = style.set_property(name, &value);
        }
        let _ = style.set_property(
            "--texture-opacity",
            &format!("{:.3}", a.texture_opacity as f64 / 100.0),
        );
        let _ = style.set_property(
            "--texture-scale-user",
            &format!("{:.3}", a.texture_scale as f64 / 100.0),
        );
    }

    if kick {
        request_animation_frame(move || {
            if let Some(el) = document_element() {
                _ = el.class_list().remove_1("theme-switching");
            }
        });
    }

    let Some(body) = body_el() else { return };
    let class = body.class_list();
    if a.noise.is_on() {
        _ = class.add_1("noise-enabled");
    } else {
        _ = class.remove_1("noise-enabled");
    }
    if matches!(a.noise, pdf_core::appearance::NoiseMode::Animated) {
        _ = class.add_1("noise-animated");
    } else {
        _ = class.remove_1("noise-animated");
    }
    let _ = body.style().set_property(
        "--noise-opacity",
        &format!("{}", a.noise_intensity.min(100) as f64 / 100.0),
    );
}

pub fn apply_theme(state: AppState) {
    // One effect, one paint: hue / texture / grain all live on Appearance,
    // and the live-preview path writes the same properties, so splitting
    // them into three effects just tripled the work on every settings write.
    // The blend backdrop needs nothing from here: it is pure CSS over the
    // same variables this effect paints (--canvas-filter / --canvas-blend)
    // plus --pdf-paper, which the engine publishes on the first render of
    // each document.
    Effect::new(move || {
        let a = state.settings.with(|s| s.appearance);
        paint_appearance_now(a);
        // The engine bakes the theme into its rasters (pages + thumbnails);
        // re-bake them at the freshly painted variables. A no-op while a
        // scrub is in flight (scrub mode owns the canvases then) and before
        // the first document opens.
        pdf_engine::api::refresh_theme();
    });

    Effect::new(move || {
        let (color, custom, opacity) = state.settings.with(|st| {
            (st.gloss_color, st.gloss_custom.clone(), st.gloss_opacity)
        });
        let Some(el) = document_element() else {
            return;
        };
        let Some(style) = el.dyn_into::<web_sys::HtmlElement>().ok().map(|h| h.style()) else {
            return;
        };
        match color.resolve(&custom) {
            Some(hex) => {
                let _ = style.set_property("--gloss-color", &hex);
            }
            None => {
                let _ = style.remove_property("--gloss-color");
            }
        }
        let _ = style.set_property("--gloss-opacity", &format!("{:.2}", opacity));
    });

    Effect::new(move || {
        let settings = state.settings.with(|s| s.clone());
        schedule_save(settings);
    });
}
