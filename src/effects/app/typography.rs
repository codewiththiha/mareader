//! Paints the reflowable formats' typography onto `<html>`.

use leptos::prelude::*;

use app_ui::theme_paint::html_style;

/// Install the typography painter: at boot, and on every change.
pub fn apply_typography(settings: RwSignal<reader_core::settings::Settings>) {
    // The reflowable typography narrowed out of the settings blob.

    Effect::new(move |_| {
        let t = settings.with(|s| s.text.clone());
        let Some(style) = html_style() else {
            return;
        };
        for (name, value) in reader_core::settings::typography::css_variables(&t) {
            let _ = style.set_property(name, &value);
        }
    });
}
