//! The motion preferences, published from the shell so the class on `<html>`
//! survives every runtime transition. This is the shell document's CSS half
//! only: each runtime frame paints its own class (`install_frame_theme`), and
//! the reader projects its own settings copy onto `viewer.motion`.

use leptos::prelude::*;

use reader_core::settings::Settings;

/// The `<html>` class that freezes every CSS animation and transition.
const ANIMATIONS_OFF_CLASS: &str = "animations-off";

/// Publish `settings.animations` as a class on `<html>` (the master's reach
/// into the CSS the runtimes do not model themselves).
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
