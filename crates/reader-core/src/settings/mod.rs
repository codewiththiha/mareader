//! Persisted user settings.
//!
//! CONTRACT: field names below are the serde schema persisted to localStorage
//! under `pdfreader.settings.v1`. Do not rename fields.
//!
//! SCHEMA EVOLUTION. The appearance model changed shape (six fixed themes ->
//! base mode + computed tint + presets), but the storage KEY did not: bumping
//! it to `.v2` would silently reset everyone's last-opened file and zoom too.
//! Instead the old fields are kept as `Option`s and migrated on load, so an
//! existing install lands on the preset that reproduces the theme it had. The
//! old fields are dropped when writing, so the migration runs at most once.

use serde::{Deserialize, Serialize};

use crate::appearance::{Appearance, BaseMode, NoiseMode, TextureMode};
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
pub use layout::{FloatingLabelStyle, LayoutSettings, PageIndicatorStyle, RenderPipeline};
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

    // --- legacy fields, read once then dropped -------------------------------
    #[serde(skip_serializing, default)]
    pub theme_id: Option<String>,
    #[serde(skip_serializing, default)]
    pub texture: Option<TextureMode>,
    #[serde(skip_serializing, default)]
    pub noise_enabled: Option<bool>,
    #[serde(skip_serializing, default)]
    pub noise_intensity: Option<u8>,
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
            theme_id: None,
            texture: None,
            noise_enabled: None,
            noise_intensity: None,
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

/// The preset that reproduces a retired theme id, for the themes that
/// became presets. The plain bases (light / dark / dim) answer `None` —
/// they are the Mode section's buttons now, and their migration is
/// [`legacy_theme_base`]'s job instead.
fn legacy_theme_to_preset(id: &str) -> Option<&'static str> {
    match id {
        "sepia" => Some("sepia"),
        "green" => Some("green"),
        "night" => Some("night"),
        _ => None,
    }
}

/// The base a retired plain theme id stood for. `None` for "light" (the
/// default base already IS light) and for anything the model never knew.
fn legacy_theme_base(id: &str) -> Option<BaseMode> {
    match id {
        "dark" => Some(BaseMode::Dark),
        "dim" => Some(BaseMode::Dim),
        _ => None,
    }
}

/// Ensures a persisted `Settings` is internally valid, and migrates the
/// pre-preset schema.
pub fn sanitize(settings: &mut Settings) {
    // --- migration -----------------------------------------------------------
    // Presence of a legacy `theme_id` means this blob predates the appearance
    // model. Rebuild the look from it so the user's chosen theme survives the
    // upgrade instead of snapping back to Light.
    if let Some(old) = settings.theme_id.take() {
        if let Some(id) = legacy_theme_to_preset(&old)
            && let Some(p) = builtin_presets().into_iter().find(|p| p.id == id)
        {
            settings.appearance = p.appearance;
            settings.active_preset = Some(p.id);
        } else if let Some(base) = legacy_theme_base(&old) {
            // A plain base is not a preset: restore the look and leave the
            // selection empty, which the Mode section's buttons express
            // better than a swatch ever did.
            settings.appearance = Appearance { base, ..Default::default() };
            settings.active_preset = None;
        }
        // Texture and grain were independent of the theme before, so carry them
        // across on top of the reconstructed look rather than letting the
        // preset's own values overwrite what the user had set.
        if let Some(t) = settings.texture.take() {
            settings.appearance.texture = t;
        }
        if let Some(on) = settings.noise_enabled.take() {
            settings.appearance.noise = if on { NoiseMode::Static } else { NoiseMode::Off };
        }
        if let Some(i) = settings.noise_intensity.take() {
            settings.appearance.noise_intensity = i;
        }
        // Those carried-over values almost certainly no longer match the
        // preset, so re-derive whether one is really active.
        settings.touch_appearance();
    }
    settings.texture = None;
    settings.noise_enabled = None;
    settings.noise_intensity = None;

    // --- validation ----------------------------------------------------------
    settings.appearance.sanitize();
    typography::sanitize(&mut settings.text);
    settings.default_zoom = settings.default_zoom.clamp(0.25, 5.0);
    settings.gloss_opacity = settings.gloss_opacity.clamp(0.1, 1.0);
    settings.layout.page_margin = settings.layout.page_margin.clamp(0.0, 64.0);
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
    fn legacy_themes_migrate_to_the_matching_preset() {
        // The upgrade path that matters: someone reading in Sepia must still be
        // in Sepia after the update, not thrown back to Light.
        for old in ["sepia", "green", "night"] {
            let json = format!(r#"{{"theme_id":"{old}"}}"#);
            let mut s: Settings = serde_json::from_str(&json).unwrap();
            sanitize(&mut s);
            assert_eq!(s.active_preset.as_deref(), Some(old), "migrating {old}");
        }
    }

    #[test]
    fn legacy_plain_themes_migrate_to_their_base_not_a_preset() {
        // Light/Dark/Dim were the plain bases; they are the Mode section's
        // buttons now, so the look survives as a bare base with no preset
        // claiming it. "light" is the default base already and needs no
        // branch at all.
        for old in ["dark", "dim"] {
            let json = format!(r#"{{"theme_id":"{old}"}}"#);
            let mut s: Settings = serde_json::from_str(&json).unwrap();
            sanitize(&mut s);
            assert_eq!(s.active_preset, None, "migrating {old}");
            let want = if old == "dark" { BaseMode::Dark } else { BaseMode::Dim };
            assert_eq!(s.appearance.base, want, "migrating {old}");
        }
        let mut s: Settings = serde_json::from_str(r#"{"theme_id":"light"}"#).unwrap();
        sanitize(&mut s);
        assert_eq!(s.appearance.base, BaseMode::Light);
        assert_eq!(s.active_preset, None);
    }

    #[test]
    fn migration_reconstructs_the_actual_look_not_just_the_id() {
        let mut s: Settings = serde_json::from_str(r#"{"theme_id":"night"}"#).unwrap();
        sanitize(&mut s);
        assert_eq!(s.appearance.base, BaseMode::Dark);
        assert!(s.appearance.has_tint(), "Night had a green cast");
    }

    #[test]
    fn migration_carries_texture_and_grain_across() {
        // Texture/noise were independent of the theme, so they must survive the
        // move even though the preset they land on has its own values.
        let json = r#"{"theme_id":"sepia","texture":"lined","noise_enabled":true,"noise_intensity":70}"#;
        let mut s: Settings = serde_json::from_str(json).unwrap();
        sanitize(&mut s);
        assert_eq!(s.appearance.texture, TextureMode::Lined);
        assert_eq!(s.appearance.noise, NoiseMode::Static);
        assert_eq!(s.appearance.noise_intensity, 70);
        // Sepia + a lined texture is no longer the stock Sepia preset.
        assert_eq!(s.active_preset, None, "modified look must read as Custom");
    }

    #[test]
    fn legacy_fields_are_not_written_back() {
        let mut s: Settings = serde_json::from_str(r#"{"theme_id":"sepia"}"#).unwrap();
        sanitize(&mut s);
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("theme_id"), "migration must run once: {json}");
        // ...and re-reading keeps the migrated look.
        let back: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(back.active_preset.as_deref(), Some("sepia"));
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
}
