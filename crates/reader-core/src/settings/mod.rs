//! Persisted user settings.
//!
//! CONTRACT: field names below are the serde schema persisted to localStorage
//! under `pdfreader.settings.v1`. Do not rename fields.
//!
//! SCHEMA EVOLUTION. The storage key outlives every schema change on purpose:
//! bumping it would silently reset everyone's last-opened file and zoom too.
//! Fields this model no longer knows are simply ignored on the way in, and
//! every field carries a default, so a blob written by any older build loads
//! cleanly — unknown keys fall away on the first write-back.

use serde::{Deserialize, Serialize};

use crate::appearance::Appearance;
use crate::appearance::presets::{builtin_presets, Preset};

mod animation;
mod gloss;
mod layout;

// The reflowable formats' typography SCHEMA is kept with the rest of the
// persisted settings, because the field names are the storage contract. The CSS
// it resolves into is `reflow_core::typography`, which re-exports these names
// so a component can read a knob and paint it from one import — hence `pub`.
pub mod typography;

/// The layout tab's policy (indicator, floating label, page frame, blend) and
/// the animations tab's switches are schemas that live in their own files;
/// re-exported so every persisted knob is still reached as
/// `reader_core::settings::<Type>`.
pub use animation::AnimationSettings;
pub use layout::{
    DEFAULT_COLUMN_WIDTH_PCT, FloatingLabelStyle, LayoutSettings, MAX_COLUMN_WIDTH_PCT,
    MIN_COLUMN_WIDTH_PCT, PageIndicatorStyle, RenderPipeline,
};
pub use typography::TextSettings;

/// The AI word card's knobs are part of the persisted schema — the flat
/// `gloss_*` field names below are storage, so the types live here rather
/// than in `ai-core`, which stays free of anything the settings model owns.
pub use gloss::{default_custom_gloss, default_gloss_opacity, is_hex6, GlossColor, GlossDensity};

/// Which pixels of a page carry the paper colour. Owned by `pdf-paper` (the
/// detector and the paint both speak it); re-exported here because the
/// settings model is the one place a reader's persisted knobs live.
pub use pdf_paper::PaperArea;

pub const SETTINGS_KEY: &str = "pdfreader.settings.v1";

/// `serde(default)` for the flags that were on before they were a switch.
pub(crate) fn on_true() -> bool { true }
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The live look. Edited directly by the appearance controls.
    pub appearance: Appearance,
    /// Id of the preset currently selected, if the live look still matches it.
    /// Cleared as soon as the user nudges any slider, which is what lets the
    /// menu show "Custom" honestly instead of claiming a preset is active when
    /// it has been modified.
    pub active_preset: Option<String>,
    /// User-saved presets (built-ins are code, not storage).
    pub user_presets: Vec<Preset>,
    pub default_zoom: f64,
    pub last_path: Option<String>,
    /// Pin the titlebar open (no auto-hide). Persisted; `serde(default)`
    /// migrates pre-pin blobs to unpinned.
    #[serde(default)]
    pub titlebar_pinned: bool,
    #[serde(default)]
    pub layout: LayoutSettings,
    #[serde(default)]
    pub animations: AnimationSettings,
    #[serde(default)]
    pub gloss_color: GlossColor,
    #[serde(default = "default_gloss_opacity")]
    pub gloss_opacity: f64,
    #[serde(default = "default_custom_gloss")]
    pub gloss_custom: String,
    /// The AI word card's spacing. Blobs saved before the field existed
    /// deserialize as Compact — the card had grown visibly airy and the
    /// denser layout is the better default even for readers who never open
    /// Settings.
    #[serde(default)]
    pub gloss_density: GlossDensity,
    /// Live compositor pipeline vs baked rasters. Blobs saved before the
    /// field existed load as `Live`, which is the behaviour they had.
    #[serde(default)]
    pub render_pipeline: RenderPipeline,
    /// Typography of the reflowable formats (plain text and Markdown):
    /// fonts, spacing, justification, the book layout. PDFs never read
    /// this — their type is baked into the page. Blobs saved before the
    /// text formats existed load the defaults.
    #[serde(default)]
    pub text: TextSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            appearance: Appearance::default(),
            // No preset matches a fresh install's plain Light look: the bases
            // are the Mode section's buttons, not presets, so "custom" is the
            // honest reading of the default state.
            active_preset: None,
            user_presets: Vec::new(),
            default_zoom: 1.0,
            last_path: None,
            titlebar_pinned: false,
            layout: LayoutSettings::default(),
            animations: AnimationSettings::default(),
            gloss_color: GlossColor::default(),
            gloss_opacity: default_gloss_opacity(),
            gloss_custom: default_custom_gloss(),
            gloss_density: GlossDensity::default(),
            render_pipeline: RenderPipeline::default(),
            text: TextSettings::default(),
        }
    }
}

impl Settings {
    /// Built-ins first, then the user's own — the order the menu renders in.
    pub fn all_presets(&self) -> Vec<Preset> {
        let mut v = builtin_presets();
        v.extend(self.user_presets.iter().cloned());
        v
    }

    fn find_preset(&self, id: &str) -> Option<Preset> {
        self.all_presets().into_iter().find(|p| p.id == id)
    }

    /// Apply a preset: copy its look and remember which one is active.
    pub fn apply_preset(&mut self, id: &str) {
        if let Some(p) = self.find_preset(id) {
            self.appearance = p.appearance;
            self.active_preset = Some(p.id);
        }
    }

    /// Record a manual appearance edit. Any hand edit detaches from the
    /// preset UNLESS it happens to land back exactly on it.
    pub fn touch_appearance(&mut self) {
        self.appearance.sanitize();
        let still = self
            .active_preset
            .as_ref()
            .and_then(|id| self.find_preset(id))
            .map(|p| p.appearance == self.appearance)
            .unwrap_or(false);
        if !still {
            self.active_preset = self
                .all_presets()
                .into_iter()
                .find(|p| p.appearance == self.appearance)
                .map(|p| p.id);
        }
    }
}

/// Ensures a persisted `Settings` is internally valid.
pub fn sanitize(settings: &mut Settings) {
    // --- validation ----------------------------------------------------------
    settings.appearance.sanitize();
    typography::sanitize(&mut settings.text);
    settings.default_zoom = settings.default_zoom.clamp(0.25, 5.0);
    settings.gloss_opacity = settings.gloss_opacity.clamp(0.1, 1.0);
    settings.layout.page_margin = settings.layout.page_margin.clamp(0.0, 64.0);
    settings.layout.column_width_pct = settings
        .layout
        .column_width_pct
        .clamp(layout::MIN_COLUMN_WIDTH_PCT, layout::MAX_COLUMN_WIDTH_PCT);
    // A startup fit of `None` is meaningless (the reader would not know how to
    // size the first page); retry to the default `FitMode::Page`.
    if settings.layout.default_fit == crate::zoom_math::FitMode::None {
        settings.layout.default_fit = layout::default_startup_fit();
    }
    settings.layout.floating_label_max_pct =
        settings.layout.floating_label_max_pct.clamp(10.0, 100.0);
    if !is_hex6(&settings.gloss_custom) {
        settings.gloss_custom = default_custom_gloss();
    }

    // Drop user presets with empty ids/names or ids that shadow a built-in;
    // both would make rows unselectable in the menu.
    let builtin_ids: Vec<String> = builtin_presets().into_iter().map(|p| p.id).collect();
    settings.user_presets.retain(|p| {
        !p.id.trim().is_empty() && !p.name.trim().is_empty() && !builtin_ids.contains(&p.id)
    });
    for p in settings.user_presets.iter_mut() {
        p.appearance.sanitize();
    }

    // A dangling active_preset (deleted preset) must not leave the menu
    // highlighting nothing while claiming a selection.
    if let Some(id) = settings.active_preset.clone()
        && !settings.all_presets().iter().any(|p| p.id == id)
    {
        settings.active_preset = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::appearance::BaseMode;

    #[test]
    fn settings_round_trip() {
        let mut s = Settings::default();
        s.appearance.base = BaseMode::Dark;
        s.appearance.tint_hue = 200;
        s.appearance.tint_strength = 40;
        s.default_zoom = 1.25;
        s.last_path = Some("/tmp/a.pdf".to_string());
        let json = serde_json::to_string(&s).unwrap();
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn applying_a_preset_sets_both_look_and_selection() {
        let mut s = Settings::default();
        s.apply_preset("green");
        assert_eq!(s.active_preset.as_deref(), Some("green"));
        assert_eq!(s.appearance.tint_hue, 104);
    }

    #[test]
    fn editing_a_slider_detaches_from_the_preset() {
        let mut s = Settings::default();
        s.apply_preset("sepia");
        s.appearance.tint_hue = 210;
        s.touch_appearance();
        assert_eq!(s.active_preset, None, "an edited preset is no longer that preset");
    }

    #[test]
    fn editing_back_onto_a_preset_reselects_it() {
        // Nice-to-have that avoids a lying UI: if you dial the sliders to
        // exactly Green, the menu should say Green.
        let mut s = Settings::default();
        s.apply_preset("sepia");
        s.appearance = builtin_presets().into_iter().find(|p| p.id == "green").unwrap().appearance;
        s.touch_appearance();
        assert_eq!(s.active_preset.as_deref(), Some("green"));
    }

    #[test]
    fn user_presets_cannot_shadow_builtins_or_be_nameless() {
        let mut s = Settings {
            user_presets: vec![
                Preset { id: "sepia".into(), name: "Mine".into(), group: String::new(), appearance: Appearance::default() },
                Preset { id: "ok".into(), name: "  ".into(), group: String::new(), appearance: Appearance::default() },
                Preset { id: "good".into(), name: "Good".into(), group: "G".into(), appearance: Appearance::default() },
            ],
            ..Settings::default()
        };
        sanitize(&mut s);
        let ids: Vec<String> = s.user_presets.iter().map(|p| p.id.clone()).collect();
        assert_eq!(ids, vec!["good".to_string()]);
    }

    #[test]
    fn a_stale_plain_base_selection_is_dropped_not_dangled() {
        // Settings persisted while Light/Dark/Dim were presets carry their
        // ids as `active_preset`; the sanitizer must clear the selection
        // (the look itself lives in `appearance` and survives untouched)
        // rather than leave the menu highlighting a swatch that no longer
        // exists.
        let mut s = Settings {
            active_preset: Some("light".to_string()),
            ..Settings::default()
        };
        sanitize(&mut s);
        assert_eq!(s.active_preset, None);
    }

    #[test]
    fn a_deleted_active_preset_does_not_dangle() {
        let mut s = Settings {
            active_preset: Some("gone".to_string()),
            ..Settings::default()
        };
        sanitize(&mut s);
        assert_eq!(s.active_preset, None);
    }

    #[test]
    fn missing_fields_default() {
        let s: Settings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.appearance, Appearance::default());
        assert!(s.user_presets.is_empty());
    }

    #[test]
    fn layout_settings_default() {
        let s = LayoutSettings::default();
        assert_eq!(s.page_margin, 0.0);
        assert!(s.auto_scale);
        assert!(s.auto_resize);
        assert!(s.page_shadow);
        assert!(!s.sidebar_overlay);
        assert!(!s.blend_mode);
        assert_eq!(s.blend_area, PaperArea::WholePage);
        assert!(!s.floating_label_persist);
        assert_eq!(s.floating_label_max_pct, 100.0);
        // Startup fit defaults to Fit Page.
        assert_eq!(s.default_fit, crate::zoom_math::FitMode::Page);

        // Deserializing empty JSON layout object fills in the defaults. A
        // blob saved BEFORE `auto_resize` existed is exactly this shape, so
        // the assertion below is also the promise that an existing install
        // keeps the behaviour it had rather than losing its refit.
        let s: LayoutSettings = serde_json::from_str("{}").unwrap();
        assert_eq!(s.page_margin, 0.0);
        assert!(s.auto_scale);
        assert!(s.auto_resize);
        assert!(s.page_shadow);
        assert!(!s.sidebar_overlay);
        assert!(!s.blend_mode);
        assert_eq!(s.blend_area, PaperArea::WholePage);
        assert_eq!(s.default_fit, crate::zoom_math::FitMode::Page);
        assert!(!s.floating_label_persist);
        assert_eq!(s.floating_label_max_pct, 100.0);
    }

    #[test]
    fn a_startup_fit_of_none_is_reset_to_page() {
        let mut s = Settings::default();
        s.layout.default_fit = crate::zoom_math::FitMode::None;
        sanitize(&mut s);
        assert_eq!(s.layout.default_fit, crate::zoom_math::FitMode::Page);
    }

    #[test]
    fn the_detection_area_round_trips() {
        let s = LayoutSettings {
            blend_area: PaperArea::Edges,
            ..LayoutSettings::default()
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(json.contains("\"blend_area\":\"edges\""), "{json}");
        let back: LayoutSettings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.blend_area, PaperArea::Edges);
    }

    #[test]
    fn a_blob_from_the_fixed_mode_era_still_loads() {
        // Older builds persisted a paper mode and a scan budget alongside
        // the switch. Both are gone; a blob that still carries them must
        // load cleanly with the switch and area it named.
        let s: LayoutSettings = serde_json::from_str(
            r#"{"blend_mode":true,"blend_scope":"fixed","blend_area":"edges","blend_scan_pages":100}"#,
        )
        .unwrap();
        assert!(s.blend_mode);
        assert_eq!(s.blend_area, PaperArea::Edges);
    }

    #[test]
    fn every_animation_is_on_until_told_otherwise() {
        let a = AnimationSettings::default();
        assert!(a.enabled);
        assert!(a.sidebar_slide && a.canvas_resize);
        assert!(a.zoom && a.scroll_jumps);

        // A blob saved before this group existed is `Settings` with the key
        // missing, so it deserialises exactly like `{}`: the reader must keep
        // animating across an update rather than freeze.
        let s: Settings = serde_json::from_str("{}").unwrap();
        assert!(s.animations.enabled && s.animations.zoom);
        // A half-written group defaults the fields it does not carry, one by
        // one — a stored master-off must not silently turn the details on.
        let a: AnimationSettings = serde_json::from_str(r#"{"enabled":false}"#).unwrap();
        assert!(!a.enabled);
        assert!(a.zoom && a.sidebar_slide && a.canvas_resize);
    }

    #[test]
    fn label_width_limit_is_clamped() {
        let mut s = Settings::default();
        s.layout.floating_label_max_pct = 420.0;
        sanitize(&mut s);
        assert_eq!(s.layout.floating_label_max_pct, 100.0);

        s.layout.floating_label_max_pct = 0.0;
        sanitize(&mut s);
        assert_eq!(s.layout.floating_label_max_pct, 10.0);
    }

    #[test]
    fn the_column_width_dial_is_clamped() {
        let mut s = Settings::default();
        assert_eq!(s.layout.column_width_pct, DEFAULT_COLUMN_WIDTH_PCT);

        s.layout.column_width_pct = 400.0;
        sanitize(&mut s);
        assert_eq!(s.layout.column_width_pct, MAX_COLUMN_WIDTH_PCT);

        s.layout.column_width_pct = 5.0;
        sanitize(&mut s);
        assert_eq!(s.layout.column_width_pct, MIN_COLUMN_WIDTH_PCT);
    }
}
