//! Paints the reflowable formats' typography onto `<html>` whenever it
//! changes — the text counterpart of `apply_theme`.
//!
//! The contract comes from `reflow_core::typography::css_variables`: every
//! knob the settings own is written as a SCALE-1 custom property
//! (`--tx-font-size`, `--tx-line-height`, …). The page hosts never read the
//! settings for type — they set their own `--ts` multiplier and let the
//! stylesheet resolve `calc(var(--tx-…) * var(--ts))`. That split is why a
//! zoom never repaints typography (only `--ts` moves) and why a settings
//! change repaints everywhere at once, pages and the measure column alike.

use leptos::prelude::*;

use crate::state::reader::TypographySignal;
use crate::effects::app::theme::html_style;
use crate::state::AppState;

/// Install the typography painter. Runs once at boot (the persisted
/// typography must be live before the first text document renders) and on
/// every change afterwards.
pub fn apply_typography(_state: AppState, typography: TypographySignal) {
    Effect::new(move |_| {
        let t = typography.get();
        let Some(style) = html_style() else {
            return;
        };
        for (name, value) in reflow_core::typography::css_variables(&t) {
            let _ = style.set_property(name, &value);
        }
    });
}
