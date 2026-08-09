//! Applies the persisted theme + noise settings to the DOM whenever they change.
//!
//! Theme -> `<html data-theme="...">` (+ `.dark` class when the theme is dark).
//! Noise -> `<body.noise-enabled>` + `--noise-opacity` on body.
//! Settings -> persisted to localStorage on every change.

use leptos::prelude::*;
use web_sys::wasm_bindgen::JsCast;

use crate::core::state::AppState;
use crate::core::themes::theme_by_id;
use crate::util::storage::save_settings;

fn document_element() -> Option<web_sys::Element> {
    web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.document_element())
}

pub fn theme_applier(state: AppState) {
    // Theme attribute + dark class.
    Effect::new(move || {
        let settings = state.settings.get();
        let theme = theme_by_id(&settings.theme_id);
        if let Some(el) = document_element() {
            let _ = el.set_attribute("data-theme", theme.id);
            let class = el.class_list();
            if theme.is_dark {
                let _ = class.add_1("dark");
            } else {
                let _ = class.remove_1("dark");
            }
        }
    });

    // Noise overlay + intensity.
    Effect::new(move || {
        let settings = state.settings.get();
        let enabled = settings.noise_enabled;
        let opacity = (settings.noise_intensity.min(100) as f64) / 100.0;
        let body = web_sys::window().and_then(|w| w.document()).and_then(|d| d.body());
        if let Some(body) = body {
            let class = body.class_list();
            if enabled {
                let _ = class.add_1("noise-enabled");
            } else {
                let _ = class.remove_1("noise-enabled");
            }
            if let Some(style) = body.dyn_ref::<web_sys::HtmlElement>().map(|h| h.style()) {
                let _ = style.set_property("--noise-opacity", &format!("{opacity}"));
            }
        }
    });

    // Persist on change.
    Effect::new(move || {
        let settings = state.settings.get();
        save_settings(&settings);
    });
}
