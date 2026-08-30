//! The motion preferences, published from one place so the two ways the reader
//! animates cannot drift apart.
//!
//! See [`Motion`] for the half that reaches the reactive graph; the class on
//! `<html>` is the master's reach into the CSS the reader does not model
//! itself (menu pops, toasts, the theme cross-fade, hover fades), which no
//! individual switch enumerates.

use leptos::prelude::*;

use crate::state::reader::Motion;
use crate::state::AppState;

/// The `<html>` class that freezes every CSS animation and transition. Its
/// rule sits next to the `prefers-reduced-motion` safety net it mirrors, in
/// `styles/components/animations.css`.
const ANIMATIONS_OFF_CLASS: &str = "animations-off";

/// Publish `settings.animations` — as `Motion` for everything the reader
/// animates in Rust, and as a class on `<html>` for the CSS.
///
/// ONE effect writes both, off ONE tracked read of the settings: a settings
/// write lands in the same flush for both halves, so no surface can catch the
/// class and the signal disagreeing. The class write is guarded because a
/// `classList` call on every settings change (an appearance slider tick, for
/// instance) would be a needless attribute mutation.
///
/// Called from the app root, not the reader page: the library is animated by
/// the same CSS, and the master has to freeze it too.
pub fn publish_motion(state: AppState) {
    let vs = state.reader.viewer;
    Effect::new(move |_| {
        let prefs = state.settings.with(|st| st.animations);
        let motion = Motion::from_prefs(&prefs);
        vs.motion.set(motion);
        let Some(el) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.document_element())
        else {
            return;
        };
        let class = el.class_list();
        // Touch the attribute only when the class and the pref disagree.
        let has = class.contains(ANIMATIONS_OFF_CLASS);
        if prefs.enabled != has {
            if prefs.enabled {
                let _ = class.remove_1(ANIMATIONS_OFF_CLASS);
            } else {
                let _ = class.add_1(ANIMATIONS_OFF_CLASS);
            }
        }
    });
}
