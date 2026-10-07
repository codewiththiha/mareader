//! The motion preferences, published from the shell onto `<html>`.

use leptos::prelude::*;

use reader_core::settings::Settings;

/// The `<html>` class that freezes every CSS animation and transition.
const ANIMATIONS_OFF_CLASS: &str = "animations-off";

/// Publish `settings.animations` as a class on `<html>`.
pub fn publish_motion(settings: RwSignal<Settings>) {
    Effect::new(move |_| {
        let off = !settings.with(|s| s.animations.enabled);
        if let Some(el) = app_ui::theme_paint::document_element() {
            let class = el.class_list();
            if off {
                let _ = class.add_1(ANIMATIONS_OFF_CLASS);
            } else {
                let _ = class.remove_1(ANIMATIONS_OFF_CLASS);
            }
        }
    });
}
